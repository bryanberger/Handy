//! Glass material: a native macOS blur behind the recording overlay.
//!
//! One view (the "glass view") is installed once, below the webview, and
//! toggled with `setHidden`/`alphaValue` for the life of the app. Installing
//! it more than once, or through Tauri's own `set_effects`, stacks views that
//! nothing but a Rust-owned toggle can remove, so every function here treats
//! installation as a one-time event and everything after it as a property
//! change on the same view.
//!
//! **Two engines, chosen once from a class lookup.** macOS 26 ships
//! `NSGlassEffectView` — Liquid Glass — which is what [`install`] uses when
//! the class exists; anything older gets the `NSVisualEffectView` this
//! feature shipped with. [`engine_for`] is the whole decision, and
//! [`GlassSupport::engine`] reports the answer so the Appearance tab offers
//! the token that engine actually honours: `glass_style` on Liquid Glass,
//! `glass_material` on the fallback. Both are live setters on the one
//! installed view, so a token change is a property write, never a re-install.
//!
//! Liquid Glass is also handed the card's surface tint, composed from
//! `surface`/`glass_tint` by `overlay_theme::liquid_tint` (which only
//! exists on macOS, so this is not an intra-doc link), so
//! that the glass can lens it rather than have it painted flat on top. The
//! card keeps painting its own surface as well: measured on macOS 26, a card
//! that left the tint to `tintColor` alone came out dark under a Light app
//! theme, with the transcript on it unreadable.
//!
//! Off macOS — and whenever installation fails — every function is a no-op
//! and [`support`] reports Glass as unavailable, so the rest of the app
//! always degrades to Flat rather than reasoning about a half-installed
//! state (`available` folds `INSTALLED` in for exactly this reason).
//!
//! Every function that touches AppKit hops to the main thread itself via
//! `AppHandle::run_on_main_thread` (a no-op hop when already on it), so
//! callers in `overlay.rs` never need to.
//!
//! Whether the view is on screen at all is not decided by its callers but by
//! [`glass_action`], the one rule this module enforces: the blur exists only
//! while the effective Material is Glass. Every entry point that could change
//! its visibility takes that Material as an argument and routes through that
//! function, so a caller cannot forget the check — it can only pass a stale
//! answer, which is why each call site in `overlay.rs` resolves the Material
//! on the thread that is about to act rather than earlier.

use crate::overlay_theme::{
    GlassEngine, GlassMaterial, GlassStyle, GlassSupport, Material, OverlayTheme,
};
use tauri::AppHandle;

/// What a caller wants from the glass view, before the Material has a say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassRequest {
    /// Bring the view in line with the theme a show or a reposition is about
    /// to render in. Never reveals: the blur may only come up once the card
    /// has painted into the window.
    ApplyMaterial,
    /// Put the blur on screen — the card has painted, or the window has just
    /// been resized under it.
    Reveal,
}

/// What the glass view must actually do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassAction {
    /// Off screen in this frame, with any fade still running cancelled.
    HideNow,
    /// Write the macOS material onto the view; leave its visibility alone.
    SetMaterialOnly,
    /// Set the corner radius and fade the view in.
    RevealNow,
}

/// Everything the installed glass view takes from the resolved theme, as one
/// argument: the two engine-specific tokens and the surface the liquid engine
/// tints itself with.
///
/// Carried whole rather than per engine so a call site cannot pick the wrong
/// half — which engine reads which field is this module's business, not
/// `overlay.rs`'s.
#[derive(Debug, Clone, PartialEq)]
pub struct GlassAppearance {
    /// The fallback engine's macOS material.
    pub glass_material: GlassMaterial,
    /// The liquid engine's Liquid Glass style.
    pub glass_style: GlassStyle,
    /// The `surface` token, or `None` to inherit the app background.
    pub surface: Option<crate::overlay_theme::HexColor>,
    /// The `glass_tint` token, or `None` to inherit. Glass's own tint
    /// strength: `surface_opacity` is Flat's control and is not read here.
    pub glass_tint: Option<f64>,
}

impl GlassAppearance {
    /// Read the glass-relevant tokens out of a resolved theme.
    pub fn from_theme(theme: &OverlayTheme) -> Self {
        GlassAppearance {
            glass_material: theme.glass_material(),
            glass_style: theme.glass_style(),
            surface: theme.surface.clone(),
            glass_tint: theme.glass_tint,
        }
    }
}

/// Which engine is drawing: `None` until a view is installed, Liquid Glass
/// wherever `NSGlassEffectView` exists (macOS 26 and later), and the
/// `NSVisualEffectView` blur everywhere else.
///
/// Pure, with the class lookup as its only input, so the version gate is a
/// table rather than a runtime accident — and so a machine that has the class
/// but failed to install still reports `None` rather than a Liquid Glass the
/// user cannot see. Both callers pass the same lookup: [`install`] to decide
/// what to create, [`support`] to report what was created.
pub fn engine_for(installed: bool, liquid_class_available: bool) -> GlassEngine {
    match (installed, liquid_class_available) {
        (false, _) => GlassEngine::None,
        (true, true) => GlassEngine::Liquid,
        (true, false) => GlassEngine::VisualEffect,
    }
}

/// The one rule for whether the native blur may be on screen: only while the
/// **effective** Material is Glass.
///
/// It is a single rule because the blur is not a decoration on the card, it is
/// the window: `overlay_dimensions` sizes the window to the card exactly under
/// Glass and gives it slack again under Flat, so a blur left visible after a
/// switch to Flat shows as a lighter translucent capsule at window size with
/// the Flat card sitting inside it. Every path that could put the view on
/// screen — a first card-shape report, the delayed fallback reveal, a
/// reposition of an already-mapped window — hands its Material to
/// [`show_glass`] or [`morph_frame`], and both come back through here, so
/// there is no reveal that skips the check. Under Flat a *reveal* request is
/// therefore not ignored but inverted: it hides.
pub fn glass_action(material: Material, request: GlassRequest) -> GlassAction {
    match (material, request) {
        (Material::Flat, _) => GlassAction::HideNow,
        (Material::Glass, GlassRequest::ApplyMaterial) => GlassAction::SetMaterialOnly,
        (Material::Glass, GlassRequest::Reveal) => GlassAction::RevealNow,
    }
}

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

/// Off macOS, Glass is neither offerable nor available, and no engine draws.
#[cfg(not(target_os = "macos"))]
pub fn support(_app: &AppHandle) -> GlassSupport {
    GlassSupport {
        supported: false,
        available: false,
        engine: GlassEngine::None,
    }
}

/// Bring the glass view's visibility and its engine-specific appearance in
/// line with the theme a show is about to render in.
///
/// Under Flat it hides the view outright and at once, cancelling any fade
/// still running — unconditionally, in case a previous Glass session left it
/// visible; a stale translucent margin around a Flat card (which has slack
/// again) would otherwise show through. Under Glass it writes the appearance
/// onto the one installed view — the material on the fallback engine, the
/// style and the tint on the liquid one — and changes nothing else: the view
/// stays exactly as visible as it was, so a first show cannot reveal it
/// before the card paints. [`show_glass`] and [`morph_frame`] are the only
/// functions that reveal it, once the window is sized and positioned for the
/// card.
///
/// Every property here is a live setter, so switching engines' tokens is a
/// property write on the existing view — the view is never re-created, which
/// is the whole reason a second one can never appear.
#[cfg(target_os = "macos")]
pub fn apply_material(app: &AppHandle, material: Material, appearance: GlassAppearance) {
    native::apply_material(app, material, appearance);
}

/// Off macOS there is no glass view to hide or re-dress.
#[cfg(not(target_os = "macos"))]
pub fn apply_material(_app: &AppHandle, _material: Material, _appearance: GlassAppearance) {}

/// Re-write the installed view's appearance from the theme as it resolves
/// **now**, because the app appearance changed under it.
///
/// The liquid engine's tint is composed from the overlay window's effective
/// appearance (`overlay_theme::liquid_tint`) at the moment it is written, and
/// nothing re-reads that appearance afterwards. A theme switch
/// while the card is on screen — which is the Appearance tab's preview, and
/// the only way to see it — therefore repaints the webview from the
/// `theme-changed` event while the glass keeps the tint it was handed under
/// the old appearance. This is the other half: both app-theme paths, the
/// setting and the system's own `ThemeChanged`, call it so the two halves of
/// the surface move together.
///
/// Resolves cache-only, so it is safe on the main thread, and the AppKit work
/// happens on the main thread regardless (see [`apply_material`]).
#[cfg(target_os = "macos")]
pub fn reapply_appearance(app: &AppHandle) {
    let resolved = crate::overlay_theme::resolve(app);
    apply_material(
        app,
        resolved.effective_material,
        GlassAppearance::from_theme(&resolved.theme),
    );
}

/// Off macOS no native surface carries a tint, so the app theme reaches the
/// card through CSS alone.
#[cfg(not(target_os = "macos"))]
pub fn reapply_appearance(_app: &AppHandle) {}

/// Reveal the glass view under `material`: set its corner radius and fade
/// alpha 0 -> 1 over the card's own fade duration (`--ov-fade-ms`) if it was
/// not already fully visible.
///
/// `material` is the Material in effect **now**, not when the reveal was
/// decided on, and it is what actually happens: under Flat this hides the view
/// instead of revealing it (see [`glass_action`]). Callers whose reveal can
/// land late — the delayed fallback reveal, a card-shape report crossing to
/// the main thread — therefore resolve it as late as they can.
///
/// Idempotent: calling it again while already visible only updates the radius.
/// A no-op when Glass is not installed.
#[cfg(target_os = "macos")]
pub fn show_glass(app: &AppHandle, material: Material, radius: f64) {
    native::show_glass(app, material, radius);
}

/// Off macOS there is no glass view to reveal.
#[cfg(not(target_os = "macos"))]
pub fn show_glass(_app: &AppHandle, _material: Material, _radius: f64) {}

/// Move the panel frame to `size`, keeping the anchored screen edge and the
/// horizontal centre fixed, set the radius, and reveal the glass view.
///
/// `size` is a Glass window — the card exactly, with no slack — so this only
/// moves the frame while `material` is still Glass. Under Flat it neither
/// resizes nor reveals: the window belongs to `update_overlay_position` then,
/// and the view goes off screen (see [`glass_action`]).
///
/// Snaps by default; `duration_ms` only animates when `HANDY_GLASS_MORPH=1`
/// opts the native animation in. See `morph_duration_ms`.
#[cfg(target_os = "macos")]
pub fn morph_frame(
    app: &AppHandle,
    material: Material,
    size: (f64, f64),
    radius: f64,
    duration_ms: u32,
) {
    native::morph_frame(app, material, size, radius, duration_ms);
}

/// Off macOS there is no native frame to animate.
#[cfg(not(target_os = "macos"))]
pub fn morph_frame(
    _app: &AppHandle,
    _material: Material,
    _size: (f64, f64),
    _radius: f64,
    _duration_ms: u32,
) {
}

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
    use std::ffi::CStr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use log::{debug, error, warn};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, NSObjectProtocol};
    use objc2_app_kit::{
        NSAnimatablePropertyContainer, NSAnimationContext, NSAppearanceCustomization,
        NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions, NSColor,
        NSGlassEffectView, NSGlassEffectViewStyle, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
        NSWindowOrderingMode, NSWorkspace,
    };
    use objc2_foundation::{MainThreadMarker, NSArray, NSPoint, NSRect, NSSize};
    use objc2_quartz_core::CAMediaTimingFunction;
    use tauri::{AppHandle, Manager};

    use super::{engine_for, glass_action, GlassAction, GlassAppearance, GlassRequest};
    use crate::overlay_geometry::CARD_FADE_MS;
    use crate::overlay_theme::{
        liquid_tint, GlassEngine, GlassMaterial, GlassStyle, GlassSupport, Material, TintColor,
    };
    use crate::settings::OverlayPosition;

    /// The retained glass view, disguised as `usize` so the static can be
    /// `Sync`. `None` until [`install`] succeeds. Touched only on the main
    /// thread, through [`on_window`]. Which class it points at is
    /// [`engine`]'s answer, which cannot change within a process.
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
            // The engine is decided here, once, from the class lookup, and
            // reported unchanged by `support` — the two agree because they
            // ask `engine_for` the same question.
            let engine = engine_for(true, liquid_class_available());
            let glass_view: Retained<NSView> = match engine {
                GlassEngine::Liquid => {
                    let view = NSGlassEffectView::initWithFrame(mtm.alloc(), bounds);
                    // Starting values only: every show and every reposition
                    // writes the resolved `glass_style` and tint onto this
                    // same view through `apply_material`.
                    view.setStyle(native_style(GlassStyle::default()));
                    Retained::into_super(view)
                }
                // `engine_for(true, ..)` never answers `None` — a view is
                // being installed right here — and the fallback blur is the
                // safe reading of it either way. Spelled out rather than
                // `_ =>` so a third engine has to be answered here.
                GlassEngine::VisualEffect | GlassEngine::None => {
                    let view = NSVisualEffectView::initWithFrame(mtm.alloc(), bounds);
                    // Likewise a starting value; `glass_material` follows.
                    view.setMaterial(native_material(GlassMaterial::default()));
                    view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                    // Mandatory: this panel can never become key
                    // (`can_become_key_window: false`, overlay.rs), so the
                    // default FollowsWindowActiveState would render the
                    // inactive, flat look forever (window-vibrancy#88, #93).
                    view.setState(NSVisualEffectState::Active);
                    // Liquid Glass rounds itself through `cornerRadius`; the
                    // fallback needs a masked backing layer to do the same.
                    view.setWantsLayer(true);
                    match view.layer() {
                        Some(layer) => layer.setMasksToBounds(true),
                        None => warn!(
                            "Glass: glass view has no backing layer; corner radius will not apply"
                        ),
                    }
                    Retained::into_super(view)
                }
            };
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
                "Glass: {engine:?} view installed ({} in the content view)",
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
            engine: engine(),
        }
    }

    /// The engine actually drawing, from the same pure rule the install used.
    fn engine() -> GlassEngine {
        engine_for(INSTALLED.load(Ordering::SeqCst), liquid_class_available())
    }

    /// Whether this macOS has `NSGlassEffectView` — the runtime gate for
    /// Liquid Glass, which arrived in macOS 26. A class lookup rather than a
    /// version comparison: the class is the thing the code needs, and asking
    /// for it directly cannot go stale the way a version table does.
    /// `objc_getClass` walks a hash table, so this is cheap enough to answer
    /// on every `support()` call rather than caching a second static.
    fn liquid_class_available() -> bool {
        AnyClass::get(c"NSGlassEffectView").is_some()
    }

    pub(super) fn apply_material(app: &AppHandle, material: Material, appearance: GlassAppearance) {
        on_window(app, move |window, _mtm| {
            let Some(view) = glass_view() else {
                return;
            };
            match glass_action(material, GlassRequest::ApplyMaterial) {
                // Off screen unconditionally, in case a previous Glass session
                // left the view visible behind a card that now has slack.
                GlassAction::HideNow => hide_now(view.as_view()),
                // Live property writes on the installed view; visibility is
                // deliberately untouched, so the blur cannot appear before
                // the card has painted. `ApplyMaterial` never asks for a
                // reveal, so `RevealNow` cannot reach this arm.
                _ => apply_appearance(window, &view, &appearance),
            }
        });
    }

    /// Write the engine's own appearance onto the installed view: the macOS
    /// material on the fallback, the Liquid Glass style and the surface tint
    /// on the liquid engine. The other engine's token is not read at all —
    /// the tokens are independent, so a theme carries both and each machine
    /// honours the one it can.
    fn apply_appearance(window: &NSWindow, view: &GlassView, appearance: &GlassAppearance) {
        match view {
            GlassView::VisualEffect(effect) => {
                effect.setMaterial(native_material(appearance.glass_material))
            }
            GlassView::Liquid(liquid) => {
                liquid.setStyle(native_style(appearance.glass_style));
                // The window's effective appearance, not the settings' theme
                // enum: `apply_window_theme` sets `NSApp.appearance` app-wide
                // and this panel follows it, so the window already knows the
                // answer for System, Light and Dark alike. It is read here
                // and nowhere else, though, so the composed tint is only as
                // fresh as the last write: the app-theme paths call
                // `reapply_appearance` to recompose it when the appearance
                // itself changes under a card already on screen.
                let tint = liquid_tint(
                    appearance.surface.as_ref(),
                    appearance.glass_tint,
                    is_dark(window),
                );
                liquid.setTintColor(tint.map(native_tint).as_deref());
            }
        }
    }

    /// The `glass_style` token's AppKit counterpart.
    fn native_style(glass_style: GlassStyle) -> NSGlassEffectViewStyle {
        match glass_style {
            GlassStyle::Regular => NSGlassEffectViewStyle::Regular,
            GlassStyle::Clear => NSGlassEffectViewStyle::Clear,
        }
    }

    /// A composed tint as an sRGB `NSColor`. sRGB explicitly, not
    /// `colorWithRed:`'s device space, because the hex the token carries is
    /// the same sRGB value the webview paints with.
    fn native_tint(tint: TintColor) -> Retained<NSColor> {
        NSColor::colorWithSRGBRed_green_blue_alpha(tint.red, tint.green, tint.blue, tint.alpha)
    }

    /// Whether the overlay window is currently drawing dark, which is what
    /// the inherited surface tint is picked from. Asked of the window rather
    /// than of the settings so `System` needs no resolution of its own.
    fn is_dark(window: &NSWindow) -> bool {
        // SAFETY: both are AppKit's own appearance-name constants.
        let (aqua, dark_aqua) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
        let names = NSArray::from_slice(&[aqua, dark_aqua]);
        window
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&names)
            .map(|name| *name == *dark_aqua)
            .unwrap_or(false)
    }

    /// The token's AppKit counterpart. `NSVisualEffectMaterial` is a plain
    /// enum of raw values, so this is the whole of the mapping.
    fn native_material(glass_material: GlassMaterial) -> NSVisualEffectMaterial {
        match glass_material {
            GlassMaterial::HudWindow => NSVisualEffectMaterial::HUDWindow,
            GlassMaterial::Popover => NSVisualEffectMaterial::Popover,
            GlassMaterial::Menu => NSVisualEffectMaterial::Menu,
            GlassMaterial::Sidebar => NSVisualEffectMaterial::Sidebar,
            GlassMaterial::UnderWindowBackground => NSVisualEffectMaterial::UnderWindowBackground,
            GlassMaterial::Sheet => NSVisualEffectMaterial::Sheet,
            GlassMaterial::Tooltip => NSVisualEffectMaterial::ToolTip,
            GlassMaterial::ContentBackground => NSVisualEffectMaterial::ContentBackground,
        }
    }

    pub(super) fn show_glass(app: &AppHandle, material: Material, radius: f64) {
        on_window(app, move |_window, _mtm| {
            let Some(view) = glass_view() else {
                return;
            };
            match glass_action(material, GlassRequest::Reveal) {
                // A reveal that finds Flat in effect is a reveal that was
                // decided under Glass and landed late. It must not be merely
                // ignored: whatever hid the view may itself have run before
                // this one was queued, so the safe answer is to hide again.
                GlassAction::HideNow => hide_now(view.as_view()),
                _ => reveal(&view, radius),
            }
        });
    }

    pub(super) fn morph_frame(
        app: &AppHandle,
        material: Material,
        size: (f64, f64),
        radius: f64,
        duration_ms: u32,
    ) {
        let overlay_position = crate::settings::get_settings(app).overlay_position;
        on_window(app, move |window, _mtm| {
            // A Glass-sized frame must not be applied to a window the Material
            // has already handed back to Flat, so the whole morph — frame and
            // reveal alike — is dropped and the view goes off screen instead.
            if glass_action(material, GlassRequest::Reveal) == GlassAction::HideNow {
                if let Some(view) = glass_view() {
                    hide_now(view.as_view());
                }
                return;
            }
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
            let view = view.as_view();
            if view.isHidden() {
                return;
            }
            if duration_ms == 0 {
                view.setAlphaValue(0.0);
            } else {
                animate_alpha(view, 0.0, duration_ms);
            }
        });
    }

    /// Take the blur off screen in this frame, cancelling any fade still
    /// running.
    ///
    /// A zero-duration animation rather than a bare `setAlphaValue`, because a
    /// `fade_out` may still be driving the alpha through the same `animator()`
    /// proxy and would otherwise keep writing over it; landing at 0 also means
    /// the next [`reveal`] fades up from clear instead of popping in.
    fn hide_now(view: &NSView) {
        animate_alpha(view, 0.0, 0);
        view.setHidden(true);
    }

    /// Set the glass view's corner radius and make sure it ends up fully
    /// visible: fading in from clear when it starts fully hidden, and also
    /// finishing an interrupted fade-out (alpha heading toward 0 but the view
    /// never hidden) — which is what a new session started inside a previous
    /// one's fade recovers through. A steady-state call (already visible,
    /// alpha already 1) only updates the radius, which is what makes both
    /// public callers idempotent.
    fn reveal(view: &GlassView, radius: f64) {
        match view {
            // Liquid Glass rounds the glass itself, edge highlight included;
            // masking its layer instead would cut the highlight off.
            GlassView::Liquid(liquid) => liquid.setCornerRadius(radius),
            GlassView::VisualEffect(effect) => {
                if let Some(layer) = effect.layer() {
                    layer.setCornerRadius(radius);
                }
            }
        }
        let view = view.as_view();
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
    fn animate_alpha(view: &NSView, target: f64, duration_ms: u32) {
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

    /// The installed glass view, as whichever class the engine chose.
    ///
    /// Every operation that is the same on both — hide, fade, alpha — takes
    /// the `NSView` this derefs to; only the corner radius and the
    /// appearance differ, and those match on the variant.
    enum GlassView {
        VisualEffect(Retained<NSVisualEffectView>),
        Liquid(Retained<NSGlassEffectView>),
    }

    impl GlassView {
        fn as_view(&self) -> &NSView {
            match self {
                GlassView::VisualEffect(view) => view,
                GlassView::Liquid(view) => view,
            }
        }
    }

    /// A new strong reference to the installed glass view, or `None` before
    /// [`install`] succeeds.
    fn glass_view() -> Option<GlassView> {
        let guard = GLASS_VIEW.lock().unwrap_or_else(|poisoned| {
            warn!("Glass: glass view mutex was poisoned, recovering");
            poisoned.into_inner()
        });
        let ptr = (*guard)?;
        // The class is re-derived from the same lookup `install` used, not
        // from `engine()`, so the cast cannot disagree with what was created
        // even in the instant between storing the pointer and setting
        // `INSTALLED`.
        //
        // SAFETY: `ptr` was produced by `Retained::into_raw` in `install` and
        // is never freed anywhere else in this module; `retain` bumps the
        // reference count for this call's own `Retained` rather than
        // consuming the stored pointer, and every use of the result happens
        // on the main thread via `on_window`.
        if liquid_class_available() {
            unsafe { Retained::retain(ptr as *mut NSGlassEffectView) }.map(GlassView::Liquid)
        } else {
            unsafe { Retained::retain(ptr as *mut NSVisualEffectView) }.map(GlassView::VisualEffect)
        }
    }

    /// The fallback engine's view class, present on every macOS.
    const FALLBACK_CLASS_NAME: &CStr = c"NSVisualEffectView";
    /// The liquid engine's view class, present only from macOS 26.
    const LIQUID_CLASS_NAME: &CStr = c"NSGlassEffectView";

    /// Which classes count as a glass view here, given whether the liquid
    /// class exists on this machine.
    ///
    /// Names rather than types, because the typed binding's
    /// `ClassType::class()` resolves the class eagerly and **panics** when it
    /// is absent — which `NSGlassEffectView` is on every macOS before 26. It
    /// was called from the install log line below, whose argument is
    /// evaluated whatever the log level filters out, so it took the app down
    /// at startup there.
    ///
    /// Pure, with the same single input [`engine_for`] takes, so the
    /// "no Liquid Glass on this machine" answer is testable on a macOS 26
    /// machine, where the lookup itself can only ever succeed.
    fn glass_view_class_names(liquid_class_available: bool) -> &'static [&'static CStr] {
        if liquid_class_available {
            &[FALLBACK_CLASS_NAME, LIQUID_CLASS_NAME]
        } else {
            &[FALLBACK_CLASS_NAME]
        }
    }

    /// Count of glass views — of either class — in `window`'s content view.
    /// Used only for the install log line above and for on-screen
    /// verification — never a behavioural signal, since installation is
    /// structurally one-shot (the only `addSubview_positioned_relativeTo`
    /// call in this module is gated on `INSTALLED`).
    fn count_glass_views(window: &NSWindow) -> usize {
        let Some(content) = window.contentView() else {
            return 0;
        };
        // Looked up by name, and only for the classes this macOS actually
        // has: a class that is absent is simply not counted, never asked for.
        let classes: Vec<&AnyClass> = glass_view_class_names(liquid_class_available())
            .iter()
            .filter_map(|name| AnyClass::get(name))
            .collect();
        content
            .subviews()
            .iter()
            .filter(|view| {
                classes
                    .iter()
                    .copied()
                    .any(|class| view.isKindOfClass(class))
            })
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
        use super::{
            glass_view_class_names, liquid_class_available, morph_duration_ms, native_material,
            native_style, AnyClass, GlassMaterial, GlassStyle,
        };
        use crate::overlay_geometry::CARD_MORPH_MS;
        use objc2_app_kit::{NSGlassEffectViewStyle, NSVisualEffectMaterial};

        /// The startup crash this seam exists for: `NSGlassEffectView` does
        /// not exist before macOS 26, and the typed binding's `class()`
        /// panics rather than returning `None` — inside an argument to the
        /// install log line, which is evaluated whatever the log level is.
        /// So on a machine without Liquid Glass the count must never name
        /// that class at all.
        ///
        /// Only this half can be exercised here: this test machine runs
        /// macOS 26, so `liquid_class_available()` is true and the absent
        /// branch is reachable only through the seam.
        #[test]
        fn the_view_count_never_names_a_class_this_macos_lacks() {
            assert_eq!(glass_view_class_names(false), &[c"NSVisualEffectView"]);
            assert_eq!(
                glass_view_class_names(true),
                &[c"NSVisualEffectView", c"NSGlassEffectView"]
            );
        }

        /// …and every name it does hand out for *this* machine resolves, so
        /// the count is a real count rather than a silent zero.
        #[test]
        fn every_counted_class_resolves_on_this_machine() {
            let names = glass_view_class_names(liquid_class_available());
            assert!(!names.is_empty());
            for name in names {
                assert!(AnyClass::get(name).is_some(), "{name:?} did not resolve");
            }
        }

        /// Every token value maps to a distinct AppKit material, and the
        /// default is the one the comparison sheet chose — a swapped pair
        /// here would be invisible in every other test.
        #[test]
        fn every_glass_material_maps_to_its_appkit_counterpart() {
            assert_eq!(
                native_material(GlassMaterial::default()),
                NSVisualEffectMaterial::HUDWindow
            );
            assert_eq!(
                native_material(GlassMaterial::Popover),
                NSVisualEffectMaterial::Popover
            );
            assert_eq!(
                native_material(GlassMaterial::UnderWindowBackground),
                NSVisualEffectMaterial::UnderWindowBackground
            );

            let mut mapped: Vec<isize> = GlassMaterial::ALL
                .into_iter()
                .map(|material| native_material(material).0)
                .collect();
            mapped.sort_unstable();
            mapped.dedup();
            assert_eq!(mapped.len(), GlassMaterial::ALL.len());
        }

        /// The two Liquid Glass styles, and which one an unset token means.
        /// The AppKit enum is two adjacent raw values, so a swapped pair here
        /// would be invisible everywhere else — the Clear card would simply
        /// look like the Regular one.
        #[test]
        fn every_glass_style_maps_to_its_appkit_counterpart() {
            assert_eq!(
                native_style(GlassStyle::default()),
                NSGlassEffectViewStyle::Regular
            );
            assert_eq!(
                native_style(GlassStyle::Clear),
                NSGlassEffectViewStyle::Clear
            );

            let mut mapped: Vec<isize> = GlassStyle::ALL
                .into_iter()
                .map(|style| native_style(style).0)
                .collect();
            mapped.sort_unstable();
            mapped.dedup();
            assert_eq!(mapped.len(), GlassStyle::ALL.len());
        }

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

#[cfg(test)]
mod tests {
    use super::{engine_for, glass_action, GlassAction, GlassRequest};
    use crate::overlay_theme::{GlassEngine, Material};

    /// The version gate, stated as a table: Liquid Glass wherever
    /// `NSGlassEffectView` exists, the old blur everywhere else, and nothing
    /// at all until a view is actually installed — which is what off macOS
    /// and a failed install both look like from here.
    #[test]
    fn the_engine_follows_the_class_lookup_once_a_view_is_installed() {
        assert_eq!(engine_for(true, true), GlassEngine::Liquid);
        assert_eq!(engine_for(true, false), GlassEngine::VisualEffect);
    }

    /// A machine that has the class but never installed a view must not
    /// advertise Liquid Glass: the tab would offer a Glass style that changes
    /// nothing, and `available` is already false for the same reason.
    #[test]
    fn nothing_is_reported_before_a_view_is_installed() {
        assert_eq!(engine_for(false, true), GlassEngine::None);
        assert_eq!(engine_for(false, false), GlassEngine::None);
    }

    /// The bug this table was written for: a Glass session's reveal landing
    /// after the user switched to Flat put the blur back on screen at window
    /// size, and the window had its Flat slack again — a lighter translucent
    /// capsule around the card. So under Flat a reveal does not merely do
    /// nothing, it hides.
    #[test]
    fn flat_takes_the_glass_view_off_screen_whatever_was_asked_for() {
        assert_eq!(
            glass_action(Material::Flat, GlassRequest::ApplyMaterial),
            GlassAction::HideNow
        );
        assert_eq!(
            glass_action(Material::Flat, GlassRequest::Reveal),
            GlassAction::HideNow
        );
    }

    /// Under Glass, only an explicit reveal reveals: applying the Material on
    /// a show must leave the view as hidden as it was, or the blur appears
    /// before the card has painted into it.
    #[test]
    fn glass_reveals_only_when_a_reveal_was_asked_for() {
        assert_eq!(
            glass_action(Material::Glass, GlassRequest::ApplyMaterial),
            GlassAction::SetMaterialOnly
        );
        assert_eq!(
            glass_action(Material::Glass, GlassRequest::Reveal),
            GlassAction::RevealNow
        );
    }

    /// Nothing but Glass ever leaves the view on screen — stated once over the
    /// whole input space, so a Material added later has to answer this too.
    #[test]
    fn the_view_is_on_screen_only_under_glass() {
        for material in [Material::Flat, Material::Glass] {
            for request in [GlassRequest::ApplyMaterial, GlassRequest::Reveal] {
                let stays_visible = glass_action(material, request) != GlassAction::HideNow;
                assert_eq!(
                    stays_visible,
                    material == Material::Glass,
                    "{material:?} + {request:?} must only keep the blur when the Material is Glass"
                );
            }
        }
    }
}
