//! The native macOS blur the Glass material draws behind the overlay.
//!
//! One view (the "glass view") is installed once, below the webview, and
//! toggled with `setHidden`/`alphaValue` for the app's life. A second install,
//! or Tauri's `set_effects`, stacks views only a Rust-owned toggle can remove,
//! so install is one-time and everything after it is a property change on it.
//!
//! Two engines, chosen once from a class lookup. macOS 26 ships
//! `NSGlassEffectView`, Liquid Glass, used by [`install`] when the class
//! exists; older macOS gets the `NSVisualEffectView` this feature shipped with.
//! [`engine_for`] decides and [`GlassSupport::engine`] reports, so the
//! Appearance tab offers the token that engine honours, `glass_style` on Liquid
//! Glass and `glass_material` on the fallback. Both are live setters, so a
//! token change is never a re-install.
//!
//! Liquid Glass is also handed the card's surface tint, composed from
//! `surface`/`glass_tint` by `overlay_theme::liquid_tint` (macOS-only, so not
//! an intra-doc link), so the glass lenses it rather than having it painted
//! flat on top. The card still paints its own surface. Measured on macOS 26, a
//! card left to `tintColor` alone came out dark under a Light app theme, with
//! the transcript on it unreadable.
//!
//! Off macOS, and whenever install fails, every function is a no-op and
//! [`support`] reports Glass unavailable. The app degrades to Flat rather than
//! reason about a half-installed state, which is why `available` folds
//! `INSTALLED` in.
//!
//! Every function that touches AppKit hops to the main thread itself via
//! `AppHandle::run_on_main_thread` (a no-op hop when already on it), so callers
//! in `overlay.rs` never need to.
//!
//! [`glass_action`] decides whether the view is on screen, and is the one rule
//! this module enforces. The blur exists only while the effective Material is
//! Glass. Every entry point that could change its visibility takes that
//! Material and routes through it, so a caller cannot forget the check, only
//! pass a stale answer. Hence each call site in `overlay.rs` resolves the
//! Material on the thread about to act.

use crate::overlay_theme::{
    GlassEngine, GlassMaterial, GlassStyle, GlassSupport, Material, OverlayTheme,
};
use tauri::AppHandle;

/// What a caller wants from the glass view, before the Material has a say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassRequest {
    /// Bring the view in line with the theme a show or reposition will render
    /// in. Never reveals. The blur may only come up once the card has painted.
    ApplyMaterial,
    /// Put the blur on screen. The card has painted, or the window was just
    /// resized under it.
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

/// What the installed glass view takes from the resolved theme in one argument:
/// the two engine-specific tokens and the surface the liquid engine tints
/// itself with. Carried whole, not per engine, so a call site cannot pick the
/// wrong half. Which field each engine reads is this module's business, not
/// `overlay.rs`'s.
#[derive(Debug, Clone, PartialEq)]
pub struct GlassAppearance {
    /// The fallback engine's macOS material.
    pub glass_material: GlassMaterial,
    /// The liquid engine's Liquid Glass style.
    pub glass_style: GlassStyle,
    /// The `surface` token, or `None` to inherit the app background.
    pub surface: Option<crate::overlay_theme::HexColor>,
    /// The `glass_tint` token, or `None` to inherit. It is Glass's own tint
    /// strength. `surface_opacity` is Flat's control and is not read here.
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
/// Pure, so the version gate is a table rather than a runtime accident. A
/// machine with the class but a failed install reports `None`, not a Liquid
/// Glass nobody can see. [`install`] and [`support`] pass the same lookup, so
/// what is created and what is reported agree.
pub fn engine_for(installed: bool, liquid_class_available: bool) -> GlassEngine {
    match (installed, liquid_class_available) {
        (false, _) => GlassEngine::None,
        (true, true) => GlassEngine::Liquid,
        (true, false) => GlassEngine::VisualEffect,
    }
}

/// The one rule for whether the native blur may be on screen, which is only
/// while the effective Material is Glass.
///
/// Under Glass the blur is the window. `overlay_geometry`'s
/// `CardMetrics::window_size` sizes the window to the card exactly under Glass
/// and gives it slack again under Flat, so a blur left visible after a switch
/// to Flat shows as a lighter translucent capsule at window size with the Flat
/// card inside it. Every path that could put the view on screen (a first
/// card-shape report, the delayed fallback reveal, a reposition of a mapped
/// window) hands its Material to [`show_glass`] or [`morph_frame`], which both
/// come back through here, so no reveal skips the check. Under Flat a reveal
/// hides rather than doing nothing.
pub fn glass_action(material: Material, request: GlassRequest) -> GlassAction {
    match (material, request) {
        (Material::Flat, _) => GlassAction::HideNow,
        (Material::Glass, GlassRequest::ApplyMaterial) => GlassAction::SetMaterialOnly,
        (Material::Glass, GlassRequest::Reveal) => GlassAction::RevealNow,
    }
}

/// Whether the overlay window casts macOS's own drop shadow.
///
/// Only under Glass, and a rule rather than a build-time flag because the two
/// Materials size the window differently. Under Glass the window *is* the card,
/// so the window server's shadow traces the card's rounded rectangle, as every
/// macOS glass panel does. Spotlight's capsule on macOS 26 darkens a white
/// desktop by about 10 % just under its edge and fades out 14 pt away; the
/// overlay measures 15 pt and macOS draws the same shadow for it. Under Flat
/// the window is deliberately larger than the card and transparent around it,
/// so that shadow would trace a rectangle nobody can see, floating away from
/// the card's corners. Flat has never had one.
///
/// Pure, so "Flat stays shadowless" is a test rather than a call-site reading.
pub fn window_shadow(material: Material) -> bool {
    matches!(material, Material::Glass)
}

/// Where a session has got to, as far as the window's shadow is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowPhase {
    /// A card is on screen, or about to paint into the window, in this
    /// Material: a show, a Material switch, a reveal, a frame morph.
    Showing(Material),
    /// The hide has started. Card and blur fade out together and the window
    /// stays mapped 300 ms more, empty (`overlay::hide_recording_overlay`).
    Leaving,
}

/// Whether the overlay window casts macOS's own drop shadow at this point in a
/// session.
///
/// [`window_shadow`] is the Material half; the hide is the other. AppKit's
/// shadow is a shape cached from what the window paints, not something that
/// follows an alpha ramp, so a Glass window keeping `hasShadow` through its
/// exit would draw a full-strength shadow around a window the card has already
/// faded out of, for the rest of the 300 ms it stays mapped. The shadow goes
/// out when the fade-out starts and the next show turns it on.
///
/// Pure, so both halves are a test rather than a call-site reading.
pub fn window_shadow_at(phase: ShadowPhase) -> bool {
    match phase {
        ShadowPhase::Showing(material) => window_shadow(material),
        ShadowPhase::Leaving => false,
    }
}

/// Install the single glass view behind the webview, hidden. The class is
/// whichever engine [`engine_for`] picks: `NSGlassEffectView` from macOS 26,
/// `NSVisualEffectView` below it.
///
/// Called from both `create_recording_overlay` paths, right after the window
/// exists and before its first `hide()`; off macOS it does nothing, so the call
/// site carries no `cfg`. Idempotent: a second call logs and returns without
/// touching the view hierarchy.
///
/// It stays hidden until [`show_glass`] or [`morph_frame`] reveals it, so a
/// Glass session cannot flash a blurred rectangle before the card paints.
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

/// Bring the glass view's visibility and its engine-specific appearance in line
/// with the theme a show is about to render in.
///
/// Under Flat it hides the view at once, cancelling any fade still running,
/// and unconditionally, in case a previous Glass session left it visible over
/// a Flat card whose slack is back, showing a stale translucent margin. Under
/// Glass it writes the appearance onto the one installed view, the material on
/// the fallback engine and the style and tint on the liquid one, and nothing
/// else. Visibility is untouched, so a first show cannot reveal it before the
/// card paints. Only [`show_glass`] and [`morph_frame`] reveal it, once the
/// window is sized and positioned for the card.
///
/// Every property is a live setter, so a token switch writes to the existing
/// view, never re-creating it, which is why a second view can never appear.
#[cfg(target_os = "macos")]
pub fn apply_material(app: &AppHandle, material: Material, appearance: GlassAppearance) {
    native::apply_material(app, material, appearance);
}

/// Off macOS there is no glass view to hide or re-dress.
#[cfg(not(target_os = "macos"))]
pub fn apply_material(_app: &AppHandle, _material: Material, _appearance: GlassAppearance) {}

/// Re-write the installed view's appearance from the theme as it resolves now,
/// because the app appearance changed under it.
///
/// The liquid engine's tint is composed from the overlay window's effective
/// appearance (`overlay_theme::liquid_tint`) when written, and nothing re-reads
/// it after. So a theme switch while the card is on screen (the Appearance
/// tab's preview, the only way to see it) repaints the webview from
/// `theme-changed` while the glass keeps the old tint. Both app-theme paths,
/// the setting and the system's `ThemeChanged`, call this so the two halves of
/// the surface move together.
///
/// Resolves cache-only, so it is safe on the main thread, and the AppKit work
/// happens there regardless (see [`apply_material`]).
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

/// Reveal the glass view under `material`: set its corner radius and fade alpha
/// 0 -> 1 over the card's own fade duration (`--ov-fade-ms`) if it was not
/// already fully visible.
///
/// `material` is the Material in effect now, not when the reveal was decided,
/// and it decides what happens. Under Flat this hides instead (see
/// [`glass_action`]). The delayed fallback reveal and a card-shape report
/// crossing to the main thread can both land late, so both resolve `material`
/// as late as they can.
///
/// Idempotent: a call while already visible only updates the radius. A no-op
/// when Glass is not installed.
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
/// `size` is a Glass window, the card exactly with no slack, so this only moves
/// the frame while `material` is still Glass. Under Flat it neither resizes nor
/// reveals. The window belongs to `update_overlay_position` then and the view
/// goes off screen (see [`glass_action`]).
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

/// Fade the glass view out over `duration_ms` so it does not outlive the card,
/// or hide it at once when `duration_ms` is 0 (the Live card is unmounted
/// rather than faded, so its blur must go with it).
///
/// The window's shadow goes at once: the window stays mapped 300 ms after the
/// fade and AppKit's shadow is a cached shape, not something that follows an
/// alpha ramp (see [`window_shadow_at`]).
///
/// Otherwise only the alpha changes. The view is deliberately left unhidden,
/// because a deferred `setHidden` would need the same generation guard the
/// delayed window unmap carries and buy nothing. A hidden window paints nothing
/// and the next reveal fades alpha up from 0 anyway.
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

    use block2::RcBlock;
    use log::{debug, error, warn};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, NSObjectProtocol};
    use objc2::Message;
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

    use super::{
        engine_for, glass_action, window_shadow_at, GlassAction, GlassAppearance, GlassRequest,
        ShadowPhase,
    };
    use crate::overlay_geometry::CARD_FADE_MS;
    use crate::overlay_theme::{
        liquid_tint, GlassEngine, GlassMaterial, GlassStyle, GlassSupport, Material, TintColor,
    };
    use crate::settings::OverlayPosition;

    /// The retained glass view, disguised as `usize` so the static can be
    /// `Sync`. `None` until [`install`] succeeds. Touched only on the main
    /// thread, through [`on_window`]. Which class it points at is [`engine`]'s
    /// answer, which cannot change within a process.
    static GLASS_VIEW: Mutex<Option<usize>> = Mutex::new(None);

    /// Set once [`install`] has added the view to the window. Folded into
    /// [`GlassSupport::available`], so a failed install renders Flat everywhere
    /// at once, with no half-installed state to reason about.
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
            // Decided once here from the class lookup and reported unchanged by
            // `support`; both ask `engine_for` the same question.
            let engine = engine_for(true, liquid_class_available());
            let glass_view: Retained<NSView> = match engine {
                GlassEngine::Liquid => {
                    let view = NSGlassEffectView::initWithFrame(mtm.alloc(), bounds);
                    // A starting value; every show and reposition writes the
                    // resolved `glass_style` and tint here via
                    // `apply_material`.
                    view.setStyle(native_style(GlassStyle::default()));
                    Retained::into_super(view)
                }
                // `engine_for(true, ..)` never answers `None` while a view is
                // being installed, and the fallback is safe either way. Spelled
                // out, not `_ =>`, so a third engine must be answered.
                GlassEngine::VisualEffect | GlassEngine::None => {
                    let view = NSVisualEffectView::initWithFrame(mtm.alloc(), bounds);
                    // Likewise a starting value; `glass_material` follows.
                    view.setMaterial(native_material(GlassMaterial::default()));
                    view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                    // Mandatory. This panel can never become key
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
            // Hidden from creation. Only a reveal may show it, and a reveal
            // runs only once the window is sized and positioned for the card.
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

    /// Whether this macOS has `NSGlassEffectView`, the runtime gate for Liquid
    /// Glass, which arrived in macOS 26. A class lookup, not a version
    /// comparison, because the class is what the code needs and cannot go stale
    /// the way a version table does. `objc_getClass` walks a hash table, so it
    /// is cheap on every `support()` call, no second static needed.
    fn liquid_class_available() -> bool {
        AnyClass::get(c"NSGlassEffectView").is_some()
    }

    pub(super) fn apply_material(app: &AppHandle, material: Material, appearance: GlassAppearance) {
        on_window(app, move |window, _mtm| {
            if let Some(view) = glass_view() {
                match glass_action(material, GlassRequest::ApplyMaterial) {
                    // Off screen unconditionally, in case a previous Glass
                    // session left it visible behind a card that now has slack.
                    GlassAction::HideNow => hide_now(view.as_view()),
                    // Live property writes on the installed view; this arm
                    // deliberately leaves visibility alone, so the blur cannot
                    // appear before the card has painted. `ApplyMaterial` never
                    // asks for a reveal, so `RevealNow` cannot reach this arm.
                    _ => apply_appearance(window, &view, &appearance),
                }
            }
            // After the view work, so the invalidation re-derives the shadow
            // from what the window paints now. Outside the lookup, so a failed
            // install still stops casting a Glass shadow when it falls to Flat.
            sync_window_shadow(window, ShadowPhase::Showing(material));
        });
    }

    /// Write the engine's own appearance onto the installed view: the macOS
    /// material on the fallback, the Liquid Glass style and surface tint on the
    /// liquid engine. The other engine's token is not read. The tokens are
    /// independent, so a theme carries both and each machine honours its own.
    fn apply_appearance(window: &NSWindow, view: &GlassView, appearance: &GlassAppearance) {
        match view {
            GlassView::VisualEffect(effect) => {
                effect.setMaterial(native_material(appearance.glass_material))
            }
            GlassView::Liquid(liquid) => {
                liquid.setStyle(native_style(appearance.glass_style));
                // The window's effective appearance, not the settings' theme
                // enum. `apply_window_theme` sets `NSApp.appearance` app-wide
                // and this panel follows it, so the window knows the answer for
                // System, Light and Dark. Read here and nowhere else, so the
                // tint is only as fresh as the last write. The app-theme paths
                // call `reapply_appearance` when the appearance changes under a
                // card on screen.
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

    /// A composed tint as an sRGB `NSColor`, not `colorWithRed:`'s device
    /// space, because the token's hex is the sRGB value the webview paints.
    fn native_tint(tint: TintColor) -> Retained<NSColor> {
        NSColor::colorWithSRGBRed_green_blue_alpha(tint.red, tint.green, tint.blue, tint.alpha)
    }

    /// Whether the overlay window is currently drawing dark, which is what the
    /// inherited surface tint is picked from. Asked of the window rather than
    /// of the settings so `System` needs no resolution of its own.
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

    /// The token's AppKit counterpart. `NSVisualEffectMaterial` is a plain enum
    /// of raw values, so this is the whole of the mapping.
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
        on_window(app, move |window, _mtm| {
            if let Some(view) = glass_view() {
                match glass_action(material, GlassRequest::Reveal) {
                    // A reveal that finds Flat was decided under Glass and
                    // landed late. Ignoring it is not enough. Whatever hid the
                    // view may have run before this was queued, so hide again.
                    GlassAction::HideNow => hide_now(view.as_view()),
                    _ => reveal(&view, radius),
                }
            }
            // Last, because the shape the shadow traces has just changed (a
            // reveal put content into an empty window, or the radius moved),
            // and the invalidation inside has to see the new one.
            sync_window_shadow(window, ShadowPhase::Showing(material));
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
            // A Glass-sized frame must not land on a window the Material has
            // already handed back to Flat, so this drops the whole morph, frame
            // and reveal alike, and takes the view off screen instead.
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
            // Horizontal centre fixed, and the anchored screen edge (bottom for
            // a bottom overlay, top for a top one) fixed. Same relative rule as
            // `calculate_overlay_position`, equivalent at every size, so this
            // attempts no Tauri-to-AppKit coordinate conversion.
            let origin = NSPoint::new(
                old.origin.x + (old.size.width - width) / 2.0,
                match overlay_position {
                    OverlayPosition::Bottom => old.origin.y,
                    OverlayPosition::Top => old.origin.y + old.size.height - height,
                },
            );
            let new_frame = NSRect::new(origin, NSSize::new(width, height));

            let snaps = duration_ms == 0;
            if snaps {
                window.setFrame_display(new_frame, true);
            } else {
                NSAnimationContext::beginGrouping();
                let context = NSAnimationContext::currentContext();
                context.setDuration(duration_ms as f64 / 1000.0);
                context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithControlPoints(
                    0.22, 1.0, 0.36, 1.0,
                )));
                // The shadow is re-derived when the animation lands, not when
                // queued. The window spends the duration between two shapes, so
                // invalidating now would trace the one it leaves. AppKit copies
                // the block and runs it on the main thread when the group ends.
                let settled = window.retain();
                let invalidate: RcBlock<dyn Fn()> = RcBlock::new(move || {
                    sync_window_shadow(&settled, ShadowPhase::Showing(material));
                });
                context.setCompletionHandler(Some(&invalidate));
                // A second `animator().setFrame_display:` on the same window
                // while one is already running retargets from the current
                // frame. An in-flight morph superseded by a newer shape is
                // deliberately left to AppKit rather than cancelled by hand.
                window.animator().setFrame_display(new_frame, true);
                NSAnimationContext::endGrouping();
            }

            if let Some(view) = glass_view() {
                reveal(&view, radius);
            }
            // Last again, and only for the snap, since frame and radius are
            // both where they will stay. An animated morph handed the same call
            // to its completion handler above.
            if snaps {
                sync_window_shadow(window, ShadowPhase::Showing(material));
            }
        });
    }

    pub(super) fn fade_out(app: &AppHandle, duration_ms: u32) {
        on_window(app, move |window, _mtm| {
            // First, whatever the glass view is doing. The window stays mapped
            // 300 ms more and AppKit's cached shadow does not follow the fade,
            // so it comes off here rather than outline an emptying window (see
            // [`window_shadow_at`], which the next show reads to turn it on).
            sync_window_shadow(window, ShadowPhase::Leaving);
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

    /// Give the overlay window the drop shadow this point in a session calls
    /// for, and re-derive the shape that shadow traces.
    ///
    /// **Called after the change it is reporting, never before.** The window is
    /// borderless and transparent, so AppKit caches the shadow from what the
    /// window paints; an invalidation before the reveal, the frame move or the
    /// fade would re-cache the shape being left and the next would never be
    /// asked for. Under Flat it paints nothing outside the card and
    /// [`window_shadow_at`] answers `false` anyway. Under Glass it is the glass
    /// view's rounded rectangle, filling the window.
    ///
    /// Unconditional, because every caller reaches here exactly when that shape
    /// changes: a Material switch, a reveal, a settled morph, a hide.
    fn sync_window_shadow(window: &NSWindow, phase: ShadowPhase) {
        let wanted = window_shadow_at(phase);
        if window.hasShadow() != wanted {
            window.setHasShadow(wanted);
        }
        window.invalidateShadow();
    }

    /// Take the blur off screen in this frame, cancelling any fade still
    /// running. A zero-duration animation rather than a bare `setAlphaValue`,
    /// because a `fade_out` may still be driving the alpha through the same
    /// `animator()` proxy and would otherwise keep writing over it; landing at
    /// 0 also means the next [`reveal`] fades up from clear instead of popping
    /// in.
    fn hide_now(view: &NSView) {
        animate_alpha(view, 0.0, 0);
        view.setHidden(true);
    }

    /// Set the glass view's corner radius and leave the view fully visible. It
    /// fades in from clear when the view starts hidden, and finishes an
    /// interrupted fade-out (alpha heading toward 0, view never hidden), which
    /// is how a new session started inside a previous one's fade recovers. A
    /// steady-state call (visible, alpha already 1) only updates the radius,
    /// which is what makes both public callers idempotent.
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
    /// It snaps by default. The native window and its blur reach the new shape
    /// a frame or two before WebKit repaints the card into it, showing as a
    /// bare blurred rim along the growing edge, measured at up to 17 pt for
    /// about 200 ms of the 460 ms Live open. Snapping has no rim in any frame.
    ///
    /// `HANDY_GLASS_MORPH=1` opts the animation back in, so the two can be
    /// compared on real hardware. macOS "Reduce motion" snaps either way,
    /// because a native window animation did not exist before Glass and a user
    /// asking for less motion should not gain one.
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

    /// The installed glass view, as whichever class the engine chose. Hide,
    /// fade and alpha are the same on both, so they take the `NSView` this
    /// derefs to. Only the corner radius and the appearance differ, and those
    /// match on the variant.
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
        // The class is re-derived from the same lookup `install` used, not from
        // `engine()`, so the cast cannot disagree with what was created even
        // between storing the pointer and setting `INSTALLED`.
        //
        // SAFETY: `ptr` came from `Retained::into_raw` in `install` and is
        // never freed elsewhere in this module; `retain` bumps the count for
        // this call's own `Retained` rather than consuming the stored pointer,
        // and every use of the result happens on the main thread via
        // `on_window`.
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

    /// Which classes count as a glass view here, given whether the liquid class
    /// exists on this machine.
    ///
    /// Names rather than types, because the typed binding's
    /// `ClassType::class()` resolves eagerly and panics when the class is
    /// absent, as `NSGlassEffectView` is on every macOS before 26. It was
    /// called from the install log line, whose argument is evaluated whatever
    /// the log level filters out, so it took the app down at startup.
    ///
    /// Pure, with the same single input [`engine_for`] takes, so the "no Liquid
    /// Glass on this machine" answer is testable on a macOS 26 machine, where
    /// the lookup itself can only ever succeed.
    fn glass_view_class_names(liquid_class_available: bool) -> &'static [&'static CStr] {
        if liquid_class_available {
            &[FALLBACK_CLASS_NAME, LIQUID_CLASS_NAME]
        } else {
            &[FALLBACK_CLASS_NAME]
        }
    }

    /// Count of glass views, of either class, in `window`'s content view. For
    /// the install log line above and on-screen verification only, never a
    /// behavioural signal, since installation is structurally one-shot (the
    /// only `addSubview_positioned_relativeTo` call here is gated on
    /// `INSTALLED`).
    fn count_glass_views(window: &NSWindow) -> usize {
        let Some(content) = window.contentView() else {
            return 0;
        };
        // Looked up by name, and only for the classes this macOS has. An absent
        // class is not counted, never asked for.
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
    /// one function, because the panel can never become key and so nothing here
    /// may run off the main thread. `run_on_main_thread` runs the closure
    /// inline when already on it, so this never deadlocks.
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

        /// The startup crash this seam exists for. `NSGlassEffectView` does not
        /// exist before macOS 26 and the typed binding's `class()` panics
        /// rather than returning `None`, inside an argument to the install log
        /// line that is evaluated whatever the log level is. So without Liquid
        /// Glass the count must never name that class. Only this half runs
        /// here. This machine is macOS 26, so `liquid_class_available()` is
        /// true and the absent branch is reachable only through the seam.
        #[test]
        fn the_view_count_never_names_a_class_this_macos_lacks() {
            assert_eq!(glass_view_class_names(false), &[c"NSVisualEffectView"]);
            assert_eq!(
                glass_view_class_names(true),
                &[c"NSVisualEffectView", c"NSGlassEffectView"]
            );
        }

        /// Every name handed out for this machine resolves, so the count is a
        /// real count rather than a silent zero.
        #[test]
        fn every_counted_class_resolves_on_this_machine() {
            let names = glass_view_class_names(liquid_class_available());
            assert!(!names.is_empty());
            for name in names {
                assert!(AnyClass::get(name).is_some(), "{name:?} did not resolve");
            }
        }

        /// Every token value maps to a distinct AppKit material, the default to
        /// `HUDWindow`. A swapped pair would be invisible in every other test.
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

        /// The two Liquid Glass styles, and which one an unset token means. The
        /// AppKit enum is two adjacent raw values, so a swap would be invisible
        /// elsewhere, the Clear card just looking like the Regular one.
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

        /// Snapping is the default, so window and blur reach the new shape in
        /// the same frame the card does, with no bare blurred rim along the
        /// growing edge.
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
        /// motion outranks the opt-in.
        #[test]
        fn reduce_motion_snaps_even_when_the_animation_is_opted_into() {
            assert_eq!(morph_duration_ms(CARD_MORPH_MS, true, true), 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        engine_for, glass_action, window_shadow, window_shadow_at, GlassAction, GlassRequest,
        ShadowPhase,
    };
    use crate::overlay_theme::{GlassEngine, Material};

    /// The version gate as a table: Liquid Glass wherever `NSGlassEffectView`
    /// exists, the old blur everywhere else, nothing until a view is installed.
    /// Off macOS and a failed install both look like that last case.
    #[test]
    fn the_engine_follows_the_class_lookup_once_a_view_is_installed() {
        assert_eq!(engine_for(true, true), GlassEngine::Liquid);
        assert_eq!(engine_for(true, false), GlassEngine::VisualEffect);
    }

    /// A machine with the class but no installed view must not advertise Liquid
    /// Glass, or the tab offers a Glass style that changes nothing. `available`
    /// is already false for the same reason.
    #[test]
    fn nothing_is_reported_before_a_view_is_installed() {
        assert_eq!(engine_for(false, true), GlassEngine::None);
        assert_eq!(engine_for(false, false), GlassEngine::None);
    }

    /// The bug this table was written for. A Glass session's reveal landing
    /// after the user switched to Flat put the blur back on screen at window
    /// size, with the window's Flat slack back, showing a lighter translucent
    /// capsule around the card. So under Flat a reveal hides.
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

    /// Under Glass, only an explicit reveal reveals. Applying the Material on a
    /// show must leave the view as hidden as it was, or the blur appears before
    /// the card has painted.
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

    /// Nothing but Glass ever leaves the view on screen. Stated once over the
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

    /// The drop shadow follows the blur, for the same reason. Only under Glass
    /// is the window the card, so only there does a window shadow trace
    /// something visible. Flat's window is bigger than its card and transparent
    /// around it, and has been shadowless since long before this feature.
    #[test]
    fn only_glass_casts_a_window_shadow() {
        assert!(window_shadow(Material::Glass));
        assert!(!window_shadow(Material::Flat));
    }

    /// The other half of the same rule, and the defect it was written for. A
    /// hide fades card and blur out but leaves the window mapped another 300
    /// ms, so a shadow surviving the fade would outline an empty window. No
    /// Material carries one out.
    #[test]
    fn the_shadow_goes_out_with_the_card() {
        assert!(window_shadow_at(ShadowPhase::Showing(Material::Glass)));
        assert!(!window_shadow_at(ShadowPhase::Showing(Material::Flat)));
        assert!(!window_shadow_at(ShadowPhase::Leaving));
    }
}
