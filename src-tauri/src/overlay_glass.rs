//! Glass material: a native macOS blur behind the recording overlay.
//!
//! One `NSVisualEffectView` (the "glass view") is installed once, below the
//! webview, and toggled with `setHidden`/`alphaValue` for the life of the
//! app. Installing it more than once, or through Tauri's own `set_effects`,
//! stacks views that nothing but a Rust-owned toggle can remove, so every
//! function here treats installation as a one-time event and everything after
//! it as a property change on the same view.
//!
//! Off macOS — and whenever installation fails — every function is a no-op
//! and [`support`] reports Glass as unavailable, so the rest of the app
//! always degrades to Flat rather than reasoning about a half-installed
//! state (`available` folds `INSTALLED` in for exactly this reason).
//!
//! Every function that touches AppKit hops to the main thread itself via
//! `AppHandle::run_on_main_thread` (a no-op hop when already on it), so
//! callers in `overlay.rs` never need to.

use crate::overlay_theme::{GlassSupport, Material};
use tauri::AppHandle;

/// Install the single `NSVisualEffectView` behind the webview, hidden.
///
/// Called from both `create_recording_overlay` paths, right after the window
/// exists and before its first `hide()`; off macOS it does nothing, so the
/// call site carries no `cfg`. Idempotent: a second call logs and returns
/// without touching the view hierarchy again.
///
/// The view is created hidden and stays hidden until [`show_glass`] or
/// [`morph_frame`] reveals it, which is what keeps a Glass session from
/// flashing a blurred rectangle before the card has painted.
#[cfg(target_os = "macos")]
pub fn install(app: &AppHandle) {
    native::install(app);
}

/// Off macOS there is no native effect to install.
#[cfg(not(target_os = "macos"))]
pub fn install(_app: &AppHandle) {}

/// Whether Glass can be rendered on this machine, and whether it can render
/// right now.
#[cfg(target_os = "macos")]
pub fn support(app: &AppHandle) -> GlassSupport {
    native::support(app)
}

/// Off macOS, Glass is neither offerable nor available.
#[cfg(not(target_os = "macos"))]
pub fn support(_app: &AppHandle) -> GlassSupport {
    GlassSupport {
        supported: false,
        available: false,
    }
}

/// Bring the glass view's visibility in line with the Material a show is
/// about to render in.
///
/// Under Flat it hides the view outright — unconditionally, in case a
/// previous Glass session left it visible; a stale translucent margin around
/// a Flat card (which has slack again) would otherwise show through. Under
/// Glass it changes nothing at all: the view stays exactly as visible as it
/// was, so a first show cannot reveal it before the card paints. [`show_glass`]
/// and [`morph_frame`] are the only functions that reveal it, once the window
/// is sized and positioned for the card.
#[cfg(target_os = "macos")]
pub fn apply_material(app: &AppHandle, material: Material) {
    native::apply_material(app, material);
}

/// Off macOS there is no glass view to hide.
#[cfg(not(target_os = "macos"))]
pub fn apply_material(_app: &AppHandle, _material: Material) {}

/// Set the glass view's corner radius and reveal it, fading alpha 0 -> 1 over
/// the card's own fade duration (`--ov-fade-ms`) if it was not already fully
/// visible. Idempotent: calling it again while already visible only updates
/// the radius. A no-op when Glass is not installed.
#[cfg(target_os = "macos")]
pub fn show_glass(app: &AppHandle, radius: f64) {
    native::show_glass(app, radius);
}

/// Off macOS there is no glass view to reveal.
#[cfg(not(target_os = "macos"))]
pub fn show_glass(_app: &AppHandle, _radius: f64) {}

/// Move the panel frame to `size`, keeping the anchored screen edge and the
/// horizontal centre fixed, set the radius, and reveal the glass view.
///
/// Snaps by default; `duration_ms` only animates when `HANDY_GLASS_MORPH=1`
/// opts the native animation in. See `morph_duration_ms`.
#[cfg(target_os = "macos")]
pub fn morph_frame(app: &AppHandle, size: (f64, f64), radius: f64, duration_ms: u32) {
    native::morph_frame(app, size, radius, duration_ms);
}

/// Off macOS there is no native frame to animate.
#[cfg(not(target_os = "macos"))]
pub fn morph_frame(_app: &AppHandle, _size: (f64, f64), _radius: f64, _duration_ms: u32) {}

/// Fade the glass view out over `duration_ms` so it does not outlive the
/// card, or hide it at once when `duration_ms` is 0 (the Live card is
/// unmounted rather than faded, so its blur must go with it).
///
/// Only the alpha changes: the view is deliberately left unhidden, because a
/// deferred `setHidden` would need the same generation guard the delayed
/// window unmap carries, and buys nothing — a hidden window paints nothing,
/// and the next reveal fades the alpha back up from 0 anyway.
#[cfg(target_os = "macos")]
pub fn fade_out(app: &AppHandle, duration_ms: u32) {
    native::fade_out(app, duration_ms);
}

/// Off macOS there is no glass view to fade.
#[cfg(not(target_os = "macos"))]
pub fn fade_out(_app: &AppHandle, _duration_ms: u32) {}

#[cfg(target_os = "macos")]
mod native {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use log::{debug, error, warn};
    use objc2::rc::Retained;
    use objc2::runtime::NSObjectProtocol;
    use objc2::ClassType;
    use objc2_app_kit::{
        NSAnimatablePropertyContainer, NSAnimationContext, NSAutoresizingMaskOptions,
        NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
        NSVisualEffectView, NSWindow, NSWindowOrderingMode, NSWorkspace,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
    use objc2_quartz_core::CAMediaTimingFunction;
    use tauri::{AppHandle, Manager};

    use crate::overlay::CARD_FADE_MS;
    use crate::overlay_theme::{GlassSupport, Material};
    use crate::settings::OverlayPosition;

    /// The retained glass view, disguised as `usize` so the static can be
    /// `Sync`. `None` until [`install`] succeeds. Touched only on the main
    /// thread, through [`on_window`].
    static GLASS_VIEW: Mutex<Option<usize>> = Mutex::new(None);

    /// Set once [`install`] has actually added the view to the window.
    /// Folded into [`GlassSupport::available`]: a failed install makes Glass
    /// render Flat everywhere at once, with no half-installed state to
    /// reason about.
    static INSTALLED: AtomicBool = AtomicBool::new(false);

    pub(super) fn install(app: &AppHandle) {
        if INSTALLED.load(Ordering::SeqCst) {
            debug!("Glass: install called again; the glass view is already installed");
            return;
        }

        on_window(app, |window, mtm| {
            if INSTALLED.load(Ordering::SeqCst) {
                return; // a concurrent call already won
            }
            let Some(content) = window.contentView() else {
                error!("Glass: recording_overlay has no content view; Glass stays unavailable");
                return;
            };

            let bounds = content.bounds();
            let glass_view = NSVisualEffectView::initWithFrame(mtm.alloc(), bounds);
            glass_view.setMaterial(NSVisualEffectMaterial::HUDWindow);
            glass_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            // Mandatory: this panel can never become key
            // (`can_become_key_window: false`, overlay.rs), so the default
            // FollowsWindowActiveState would render the inactive, flat look
            // forever (window-vibrancy#88, #93).
            glass_view.setState(NSVisualEffectState::Active);
            glass_view.setWantsLayer(true);
            match glass_view.layer() {
                Some(layer) => layer.setMasksToBounds(true),
                None => {
                    warn!("Glass: glass view has no backing layer; corner radius will not apply")
                }
            }
            glass_view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            content.addSubview_positioned_relativeTo(
                &glass_view,
                NSWindowOrderingMode::Below,
                None,
            );
            // Hidden from creation: nothing but a reveal, which only ever runs
            // once the window is sized and positioned for the card, may show it.
            glass_view.setHidden(true);

            let ptr = Retained::into_raw(glass_view) as usize;
            store_glass_view(ptr);
            INSTALLED.store(true, Ordering::SeqCst);
            debug!(
                "Glass: NSVisualEffectView installed ({} in the content view)",
                count_glass_views(window)
            );
        });
    }

    pub(super) fn support(_app: &AppHandle) -> GlassSupport {
        let installed = INSTALLED.load(Ordering::SeqCst);
        // The objc2 binding takes `&self` with no `MainThreadMarker`, so this
        // is a safe, cheap read from any thread.
        let available = installed
            && !NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency();
        GlassSupport {
            supported: true,
            available,
        }
    }

    pub(super) fn apply_material(app: &AppHandle, material: Material) {
        // Only Flat has anything to do, and it is a single `setHidden` — so
        // Glass does not pay for a main-thread hop that would change nothing.
        if material != Material::Flat {
            return;
        }
        on_window(app, move |_window, _mtm| {
            if let Some(view) = glass_view() {
                view.setHidden(true);
            }
        });
    }

    pub(super) fn show_glass(app: &AppHandle, radius: f64) {
        on_window(app, move |_window, _mtm| {
            if let Some(view) = glass_view() {
                reveal(&view, radius);
            }
        });
    }

    pub(super) fn morph_frame(app: &AppHandle, size: (f64, f64), radius: f64, duration_ms: u32) {
        let overlay_position = crate::settings::get_settings(app).overlay_position;
        on_window(app, move |window, _mtm| {
            let duration_ms = morph_duration_ms(
                duration_ms,
                morph_opted_in(),
                NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion(),
            );
            let (width, height) = size;
            let old = window.frame();
            // Horizontal centre fixed; the anchored screen edge (bottom for a
            // bottom overlay, top for a top one) fixed. Same relative rule as
            // `calculate_overlay_position`, which is provably equivalent at
            // every size, so no Tauri-to-AppKit coordinate conversion is
            // attempted here.
            let origin = NSPoint::new(
                old.origin.x + (old.size.width - width) / 2.0,
                match overlay_position {
                    OverlayPosition::Bottom => old.origin.y,
                    OverlayPosition::Top => old.origin.y + old.size.height - height,
                },
            );
            let new_frame = NSRect::new(origin, NSSize::new(width, height));

            if duration_ms == 0 {
                window.setFrame_display(new_frame, true);
            } else {
                NSAnimationContext::beginGrouping();
                let context = NSAnimationContext::currentContext();
                context.setDuration(duration_ms as f64 / 1000.0);
                context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(
                    0.22, 1.0, 0.36, 1.0,
                )));
                // A second `animator().setFrame_display:` on the same window
                // while one is already running retargets from the current
                // frame — an in-flight morph superseded by a newer shape is
                // deliberately left to AppKit rather than cancelled by hand.
                window.animator().setFrame_display(new_frame, true);
                NSAnimationContext::endGrouping();
            }

            if let Some(view) = glass_view() {
                reveal(&view, radius);
            }
        });
    }

    pub(super) fn fade_out(app: &AppHandle, duration_ms: u32) {
        on_window(app, move |_window, _mtm| {
            let Some(view) = glass_view() else {
                return;
            };
            if view.isHidden() {
                return;
            }
            if duration_ms == 0 {
                view.setAlphaValue(0.0);
            } else {
                animate_alpha(&view, 0.0, duration_ms);
            }
        });
    }

    /// Set the glass view's corner radius and make sure it ends up fully
    /// visible: fading in from clear when it starts fully hidden, and also
    /// finishing an interrupted fade-out (alpha heading toward 0 but the view
    /// never hidden) — which is what a new session started inside a previous
    /// one's fade recovers through. A steady-state call (already visible,
    /// alpha already 1) only updates the radius, which is what makes both
    /// public callers idempotent.
    fn reveal(view: &NSVisualEffectView, radius: f64) {
        if let Some(layer) = view.layer() {
            layer.setCornerRadius(radius);
        }
        let was_hidden = view.isHidden();
        if was_hidden {
            view.setAlphaValue(0.0);
            view.setHidden(false);
        }
        if was_hidden || view.alphaValue() < 1.0 {
            animate_alpha(view, 1.0, CARD_FADE_MS);
        }
    }

    /// Animate `view`'s `alphaValue` to `target` over `duration_ms`, via the
    /// same `animator()` implicit-animation proxy the frame morph uses.
    fn animate_alpha(view: &NSVisualEffectView, target: f64, duration_ms: u32) {
        NSAnimationContext::beginGrouping();
        let context = NSAnimationContext::currentContext();
        context.setDuration(duration_ms as f64 / 1000.0);
        view.animator().setAlphaValue(target);
        NSAnimationContext::endGrouping();
    }

    /// How long the frame morph may actually take.
    ///
    /// It snaps by default. The native window and its blur reach the new
    /// shape a frame or two before WebKit repaints the card into it, which
    /// shows as a bare blurred rim along the growing edge — measured at up to
    /// 17 pt for about 200 ms of the 460 ms Live open. Snapping has no rim in
    /// any frame.
    ///
    /// `HANDY_GLASS_MORPH=1` opts the animation back in, so the two can be
    /// compared on real hardware. macOS "Reduce motion" snaps either way: a
    /// native *window* animation did not exist before Glass, so a user who
    /// has asked for less motion should not get one from it.
    fn morph_duration_ms(requested_ms: u32, morph_opted_in: bool, reduce_motion: bool) -> u32 {
        if morph_opted_in && !reduce_motion {
            requested_ms
        } else {
            0
        }
    }

    /// Whether this run has opted into the native frame animation.
    fn morph_opted_in() -> bool {
        crate::utils::env_flag_enabled("HANDY_GLASS_MORPH")
    }

    fn store_glass_view(ptr: usize) {
        let mut guard = GLASS_VIEW.lock().unwrap_or_else(|poisoned| {
            warn!("Glass: glass view mutex was poisoned, recovering");
            poisoned.into_inner()
        });
        *guard = Some(ptr);
    }

    /// A new strong reference to the installed glass view, or `None` before
    /// [`install`] succeeds.
    fn glass_view() -> Option<Retained<NSVisualEffectView>> {
        let guard = GLASS_VIEW.lock().unwrap_or_else(|poisoned| {
            warn!("Glass: glass view mutex was poisoned, recovering");
            poisoned.into_inner()
        });
        let ptr = (*guard)?;
        // SAFETY: `ptr` was produced by `Retained::into_raw` in `install` and
        // is never freed anywhere else in this module; `retain` bumps the
        // reference count for this call's own `Retained` rather than
        // consuming the stored pointer, and every use of the result happens
        // on the main thread via `on_window`.
        unsafe { Retained::retain(ptr as *mut NSVisualEffectView) }
    }

    /// Count of `NSVisualEffectView` instances in `window`'s content view.
    /// Used only for the install log line above and for on-screen
    /// verification — never a behavioural signal, since installation is
    /// structurally one-shot (the only `addSubview_positioned_relativeTo`
    /// call in this module is gated on `INSTALLED`).
    fn count_glass_views(window: &NSWindow) -> usize {
        let Some(content) = window.contentView() else {
            return 0;
        };
        content
            .subviews()
            .iter()
            .filter(|view| view.isKindOfClass(NSVisualEffectView::class()))
            .count()
    }

    /// Run `f` on the main thread with the overlay panel's `NSWindow`, if the
    /// window exists yet. Every AppKit call in this module goes through this
    /// — the panel can never become key, so nothing here may run off the
    /// main thread — and `run_on_main_thread` runs the closure inline when
    /// already on it, so this never deadlocks.
    fn on_window<F>(app: &AppHandle, f: F)
    where
        F: FnOnce(&NSWindow, MainThreadMarker) + Send + 'static,
    {
        let Some(window) = app.get_webview_window("recording_overlay") else {
            warn!("Glass: no recording_overlay window");
            return;
        };
        let Ok(ns_window) = window.ns_window() else {
            warn!("Glass: recording_overlay has no native window handle");
            return;
        };
        let ptr = ns_window as usize;
        let _ = app.run_on_main_thread(move || {
            let Some(mtm) = MainThreadMarker::new() else {
                warn!("Glass: run_on_main_thread callback observed off the main thread");
                return;
            };
            // SAFETY: `ptr` came from `ns_window()` on this same, still-open
            // window immediately above, and every AppKit call reachable from
            // `f` happens inside this main-thread closure, so nothing can
            // deallocate the window concurrently with it.
            let window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
            f(window, mtm);
        });
    }

    #[cfg(test)]
    mod tests {
        use super::morph_duration_ms;
        use crate::overlay::CARD_MORPH_MS;

        /// The default the visual sign-off settled on: the window and its blur
        /// reach the new shape in the same frame the card does, with no bare
        /// frosted rim along the growing edge.
        #[test]
        fn the_frame_snaps_unless_the_animation_is_opted_into() {
            assert_eq!(morph_duration_ms(CARD_MORPH_MS, false, false), 0);
            assert_eq!(morph_duration_ms(2000, false, false), 0);
        }

        #[test]
        fn opting_in_uses_the_duration_the_card_reported() {
            assert_eq!(morph_duration_ms(CARD_MORPH_MS, true, false), CARD_MORPH_MS);
            // A report asking for a snap still snaps.
            assert_eq!(morph_duration_ms(0, true, false), 0);
        }

        /// A native window animation did not exist before Glass, so Reduce
        /// motion outranks the opt-in rather than being overridden by it.
        #[test]
        fn reduce_motion_snaps_even_when_the_animation_is_opted_into() {
            assert_eq!(morph_duration_ms(CARD_MORPH_MS, true, true), 0);
        }
    }
}
