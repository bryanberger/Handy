use crate::input;
use crate::overlay_theme::Material;
use crate::settings;
use crate::settings::{OverlayPosition, OverlayStyle};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

#[cfg(not(target_os = "macos"))]
use log::debug;

#[cfg(not(target_os = "macos"))]
use tauri::WebviewWindowBuilder;

#[cfg(target_os = "macos")]
use tauri::WebviewUrl;

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelBuilder, PanelLevel, StyleMask};

#[cfg(target_os = "linux")]
use crate::utils;

#[cfg(target_os = "linux")]
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(RecordingOverlayPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

// Native overlay window geometry (logical points). One window is reused for
// every state and resized on every show, from
//
//     window(form, scale) = ceil(card footprint × scale) + slack(form)
//
// where the card footprint is the card's largest footprint in that form at
// size_scale 1 (border included) and the slack is the transparent margin that
// keeps the card's morph animations inside the window. The card is CSS-anchored
// flush to the screen edge, so window size doesn't move where the card sits —
// only OVERLAY_TOP_OFFSET / OVERLAY_BOTTOM_OFFSET do.
//
// The card constants mirror the `--ov-*` block in RecordingOverlay.css; the
// `overlay_window_constants_match_overlay_css` test parses that file and fails
// if either side drifts.

/// The card's border at size_scale 1, both sides: `.scard` is content-box and
/// draws a 1 px hairline that scales with everything else, so the card's
/// footprint is `(content + CARD_BORDER) × scale`.
const CARD_BORDER: f64 = 2.0;
/// Widest compact card (Minimal / transcribing / processing) at size_scale 1:
/// `--ov-work-w` 216, the working pill, plus the border. The pill animates its
/// width from `--ov-rest-w` 172 and expands from its centre, so the window must
/// fit this widest state.
const CARD_COMPACT_W: f64 = 216.0 + CARD_BORDER;
/// Compact card height at size_scale 1: `--ov-base-h` 40, the control row, plus
/// the border.
const CARD_COMPACT_H: f64 = 40.0 + CARD_BORDER;
/// Resting compact card (Minimal at rest) at size_scale 1: `--ov-rest-w` 172
/// plus the border. Only [`OverlayCardShape::CompactRest`] uses this — under
/// Flat the window still covers [`CARD_COMPACT_W`], the form's widest card.
const CARD_COMPACT_REST_W: f64 = 172.0 + CARD_BORDER;
/// Widest Live card at size_scale 1: `--ov-open-w` 392, the open panel, plus
/// the border. Live opens from `--ov-pill-w` 184, so this is again the widest
/// state.
const CARD_LIVE_W: f64 = 392.0 + CARD_BORDER;
/// Live pill before it opens or collapses, at size_scale 1: `--ov-pill-w` 184
/// plus the border. Only [`OverlayCardShape::LivePill`] uses this — under
/// Flat the window still covers [`CARD_LIVE_W`], the form's widest card.
const CARD_LIVE_PILL_W: f64 = 184.0 + CARD_BORDER;
/// Tallest Live card at size_scale 1: the control row `--ov-base-h` 40, the
/// live-text region `--ov-cap-max-h` 64 and its `--ov-cap-pad-y` 12, plus the
/// border.
const CARD_LIVE_H: f64 = 40.0 + 64.0 + 12.0 + CARD_BORDER;

/// How long the card's morph between two shapes takes, in milliseconds:
/// `--ov-morph-ms`, the duration of `.scard`'s width and border-radius
/// transitions. The overlay webview reads the same custom property and sends
/// it with every card-shape report, so the native window-frame animation
/// under Glass runs for exactly as long as the CSS morph would have.
pub(crate) const CARD_MORPH_MS: u32 = 460;
/// How long the card fades, in milliseconds: `--ov-fade-ms`, the duration of
/// `.ov-fade`'s opacity transition. Under Glass the native blur has to fade
/// over the same span, in and out, or it reads as a separate object.
pub(crate) const CARD_FADE_MS: u32 = 200;
/// The largest morph duration a card-shape report may ask for, in
/// milliseconds — roughly four times [`CARD_MORPH_MS`]. Anything beyond it is
/// a bug or a hostile call rather than a slower animation, and would pin a
/// native window animation on screen long after the card had settled.
pub(crate) const MAX_CARD_MORPH_MS: u32 = 2000;

/// Window slack for the compact states, in logical points: 218 + 38 = 256 wide,
/// 42 + 4 = 46 tall, i.e. exactly the window this overlay has always used.
const COMPACT_SLACK: (f64, f64) = (38.0, 4.0);
/// Window slack for the Live panel: 394 + 6 = 400 wide, 118 + 2 = 120 tall —
/// again today's window.
const LIVE_SLACK: (f64, f64) = (6.0, 2.0);

// The four windows this overlay has always used, kept as named scale-1
// fixtures for the tests below. They are `#[cfg(test)]` because no production
// path reads a fixed size any more: every window is computed from the card and
// the resolved size scale.
/// Compact window width at size_scale 1.
#[cfg(test)]
const OVERLAY_WIDTH: f64 = CARD_COMPACT_W + COMPACT_SLACK.0;
/// Compact window height at size_scale 1.
#[cfg(test)]
const OVERLAY_HEIGHT: f64 = CARD_COMPACT_H + COMPACT_SLACK.1;
/// Live window width at size_scale 1.
#[cfg(test)]
const OVERLAY_STREAM_WIDTH: f64 = CARD_LIVE_W + LIVE_SLACK.0;
/// Live window height at size_scale 1.
#[cfg(test)]
const OVERLAY_STREAM_HEIGHT: f64 = CARD_LIVE_H + LIVE_SLACK.1;

/// Which card the overlay draws: the compact pill (Minimal, and the working
/// states of both styles) or the Live panel.
///
/// The only distinction the native geometry makes — every other difference
/// between the UI states happens inside the card, at a size the window already
/// covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayForm {
    /// The pill: Minimal in every state, and Live before it opens.
    Compact,
    /// The Live panel, sized for its open footprint.
    Live,
}

impl OverlayForm {
    /// The card's largest footprint in this form at size_scale 1, borders
    /// included. What the window covers under Flat, where the CSS morph
    /// happens inside a window sized for the state's widest card; under
    /// Glass the window instead equals the *exact* current
    /// [`OverlayCardShape`], never a form's max.
    fn card_footprint(self) -> (f64, f64) {
        match self {
            Self::Compact => (CARD_COMPACT_W, CARD_COMPACT_H),
            Self::Live => (CARD_LIVE_W, CARD_LIVE_H),
        }
    }
}

/// Which of the five card shapes the overlay is currently drawing.
///
/// Under Flat this is bookkeeping only: the window is sized for the widest
/// card the overlay style can reach, and the CSS morph happens inside it.
/// Under Glass it is the unit the native window is sized from, because the
/// window slack is zero and the window rectangle is the card — and because
/// the Live panel's open/collapsed morph is a pure webview decision (driven
/// by streamed text and phase) that Rust cannot see any other way.
///
/// One variant per distinct `.scard` class combination in
/// `RecordingOverlay.tsx`; the footprints mirror the `--ov-*` block in
/// `RecordingOverlay.css` and are pinned to it by
/// `overlay_window_constants_match_overlay_css`. Must agree with
/// `cardShape()` in `src/overlay/cardShape.ts`; pinned by
/// `initial_card_shape_matches_card_shape_ts`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OverlayCardShape {
    /// `.scard.compact` — the resting Minimal pill.
    CompactRest,
    /// `.scard.compact.cworking` — the Minimal working pill, at the same
    /// footprint as Live's own collapsed working pill.
    CompactWorking,
    /// `.scard` — the Live pill before it opens or collapses.
    LivePill,
    /// `.scard.working` — the Live panel collapsed to its working pill.
    LiveWorking,
    /// `.scard.open` — the Live panel, expanded.
    LiveOpen,
}

impl OverlayCardShape {
    /// Declaration order, mirroring the `#[repr(u8)]` discriminants — the
    /// table [`Self::from_u8`] indexes into.
    const ALL: [OverlayCardShape; 5] = [
        Self::CompactRest,
        Self::CompactWorking,
        Self::LivePill,
        Self::LiveWorking,
        Self::LiveOpen,
    ];

    /// The coarse form this shape belongs to — what [`overlay_slack`] and the
    /// Flat-material footprint (the state's widest card) key on.
    fn form(self) -> OverlayForm {
        match self {
            Self::CompactRest | Self::CompactWorking => OverlayForm::Compact,
            Self::LivePill | Self::LiveWorking | Self::LiveOpen => OverlayForm::Live,
        }
    }

    /// This shape's own exact footprint at size_scale 1, border included —
    /// what the window must equal under Glass, where the window is the card.
    /// Every number comes from the `--ov-*` block in RecordingOverlay.css.
    fn card_footprint(self) -> (f64, f64) {
        match self {
            Self::CompactRest => (CARD_COMPACT_REST_W, CARD_COMPACT_H),
            Self::CompactWorking => (CARD_COMPACT_W, CARD_COMPACT_H),
            Self::LivePill => (CARD_LIVE_PILL_W, CARD_COMPACT_H),
            Self::LiveWorking => (CARD_COMPACT_W, CARD_COMPACT_H),
            Self::LiveOpen => (CARD_LIVE_W, CARD_LIVE_H),
        }
    }

    /// The corner-radius factor CSS applies for this shape's `.scard...`
    /// class: the pill and the two compact states are the full radius token,
    /// the Live working pill is 3/4, the open panel is 2/3. Both CSS and the
    /// native `CALayer` radius clamp visually to half the shorter side, so
    /// they agree by construction without either side rounding first — this
    /// value is deliberately left unrounded.
    fn radius_factor(self) -> f64 {
        match self {
            Self::CompactRest | Self::CompactWorking | Self::LivePill => 1.0,
            Self::LiveWorking => 0.75,
            Self::LiveOpen => 2.0 / 3.0,
        }
    }

    /// Recover a shape from the byte [`OVERLAY_CARD_SHAPE`] stores. Any value
    /// outside the five real discriminants (never produced by this module)
    /// falls back to [`Self::CompactRest`] rather than panicking.
    fn from_u8(value: u8) -> Self {
        Self::ALL
            .get(value as usize)
            .copied()
            .unwrap_or(Self::CompactRest)
    }
}

/// The shape the card takes on the first frame of `state`. Must agree with
/// `cardShape()` in `src/overlay/cardShape.ts`; pinned by
/// `initial_card_shape_matches_card_shape_ts`.
///
/// Verified against the frontend, state by state:
/// `"recording"` renders `.scard.compact` with `working = false` ->
/// [`OverlayCardShape::CompactRest`]; `"transcribing"`/`"processing"` render
/// `.scard.compact.cworking` -> [`OverlayCardShape::CompactWorking`];
/// `"streaming"` resets its stream state before showing, so it always starts
/// at `open = false, collapsed = false` -> [`OverlayCardShape::LivePill`].
fn initial_card_shape(state: &str) -> OverlayCardShape {
    match state {
        "transcribing" | "processing" => OverlayCardShape::CompactWorking,
        "streaming" => OverlayCardShape::LivePill,
        _ => OverlayCardShape::CompactRest, // "recording"
    }
}

/// The shape the overlay window is sized for right now: the shape most
/// recently shown or reported by the webview, or [`OverlayCardShape::CompactRest`]
/// before the first show (and again once the overlay is fully hidden — see
/// `hide_recording_overlay`).
///
/// Replaces the old `OVERLAY_SHOWS_LIVE` bool: under zero-slack Glass the
/// window must track the *exact* shape, not merely compact-vs-Live, and
/// "is streaming" is still recoverable from it (`shape.form() ==
/// OverlayForm::Live`), so this is one atomic, not two.
static OVERLAY_CARD_SHAPE: AtomicU8 = AtomicU8::new(OverlayCardShape::CompactRest as u8);

/// The shape the overlay window is currently sized for.
fn current_card_shape() -> OverlayCardShape {
    OverlayCardShape::from_u8(OVERLAY_CARD_SHAPE.load(Ordering::SeqCst))
}

/// Store a new shape, returning the one it replaced — used by
/// [`set_card_shape`] to decide whether a report actually changed anything
/// (coalescing by identity, never by time).
fn set_current_card_shape(shape: OverlayCardShape) -> OverlayCardShape {
    OverlayCardShape::from_u8(OVERLAY_CARD_SHAPE.swap(shape as u8, Ordering::SeqCst))
}

/// The transparent margin between the card's footprint and the edge of the
/// native overlay window, in logical points.
///
/// It exists so a card mid-morph is never clipped by the overlay page's
/// `overflow: hidden`. Zero under Glass: the window rectangle *is* the card,
/// because the native glass view fills the whole window and any slack would
/// paint blur outside it. No `#[cfg(target_os = "macos")]` is
/// needed here — the effective Material is never Glass off macOS (only a
/// `support()` with `available: true` can produce it, and that only exists on
/// macOS), so this reduces to today's per-form slack everywhere but a Mac
/// with Glass actually available.
fn overlay_slack(form: OverlayForm, material: Material) -> (f64, f64) {
    if material == Material::Glass {
        return (0.0, 0.0);
    }
    match form {
        OverlayForm::Compact => COMPACT_SLACK,
        OverlayForm::Live => LIVE_SLACK,
    }
}

/// Overlay window size (logical points) for a card shape at a given size
/// scale and Material.
///
/// Under Glass the window equals the shape's own exact footprint — the
/// window IS the card. Under Flat the window covers the *state's* widest
/// card (`shape.form().card_footprint()`), exactly as before
/// [`OverlayCardShape`] existed, because the CSS width/height morph happens
/// inside it.
///
/// The scaled card is rounded up before the slack is added, so the window is
/// never a fraction of a point short of the card it hosts and every result is a
/// whole number of points. `scale` is re-clamped here — the geometry boundary
/// must never trust a number that reached it unclamped — using the same bounds
/// as [`crate::overlay_theme::OverlayTheme::size_scale`], so the window and the
/// card can never disagree about how far a token was allowed to go.
fn overlay_dimensions(shape: OverlayCardShape, scale: f64, material: Material) -> (f64, f64) {
    let scale = if scale.is_finite() {
        scale.clamp(
            crate::overlay_theme::SIZE_SCALE_MIN,
            crate::overlay_theme::SIZE_SCALE_MAX,
        )
    } else {
        1.0
    };
    let (card_width, card_height) = match material {
        Material::Glass => shape.card_footprint(),
        Material::Flat => shape.form().card_footprint(),
    };
    let (slack_width, slack_height) = overlay_slack(shape.form(), material);

    (
        (card_width * scale).ceil() + slack_width,
        (card_height * scale).ceil() + slack_height,
    )
}

/// The resolved `radius` token in px at `size_scale` 1: the persisted or
/// inherited value, defaulting to the CSS token's own default (`--ov-radius:
/// 24px`, `RecordingOverlay.css`) when unset. Read from the public field
/// directly — `OverlayTheme` does not carry this as a method, which is out of
/// this module's edit scope.
fn radius_token_px(theme: &crate::overlay_theme::OverlayTheme) -> f64 {
    const DEFAULT_RADIUS_PX: f64 = 24.0;
    theme.radius.map(f64::from).unwrap_or(DEFAULT_RADIUS_PX)
}

/// A shape's corner radius in px at `scale`, mirroring the CSS
/// `calc(var(--ov-radius) * var(--ov-scale) * factor)` — unrounded, like the
/// CSS, so CALayer and CSS agree by construction (both clamp visually to
/// half the shorter side).
fn shape_radius(shape: OverlayCardShape, radius_px: f64, scale: f64) -> f64 {
    radius_px * scale * shape.radius_factor()
}

/// The size scale in effect, clamped, from the resolved overlay theme.
///
/// Resolves from the theme-file cache — no filesystem IO — so it is safe on the
/// main thread, where the geometry runs. The show path resolves with a fresh
/// file read instead, off the main thread.
fn resolved_size_scale(app_handle: &AppHandle) -> f64 {
    crate::overlay_theme::resolve(app_handle).theme.size_scale()
}

/// The compact window at the scale in effect: the size the overlay window is
/// created at, before any card has been shown.
///
/// Always sized for Flat: at window-creation time `overlay_glass::install`
/// has not run yet (it needs the window this function's own caller is about
/// to build), so Glass can never be *available* here regardless of what is
/// persisted. The very first real show (`show_overlay_state_on_main`)
/// resolves fresh and resizes correctly once Glass is installed — the same
/// "cosmetic first-show resize" tradeoff the theme file's launch-time read
/// already accepts.
fn initial_overlay_dimensions(app_handle: &AppHandle) -> (f64, f64) {
    overlay_dimensions(
        OverlayCardShape::CompactRest,
        resolved_size_scale(app_handle),
        Material::Flat,
    )
}

static LAST_MIC_LEVEL_EMIT: AtomicU64 = AtomicU64::new(0);
const EMIT_THROTTLE_MS: u64 = 33; // ~30 FPS

#[cfg(target_os = "macos")]
const OVERLAY_TOP_OFFSET: f64 = 46.0;
#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_TOP_OFFSET: f64 = 4.0;

#[cfg(target_os = "macos")]
const OVERLAY_BOTTOM_OFFSET: f64 = 15.0;

#[cfg(any(target_os = "windows", target_os = "linux"))]
const OVERLAY_BOTTOM_OFFSET: f64 = 40.0;

/// Configures the edge and offset of a GTK layer surface. gtk-layer-shell
/// commits anchor and margin changes itself, including while the surface is
/// mapped, so changing position does not require a manual hide/show cycle.
#[cfg(target_os = "linux")]
fn configure_layer_shell_position(gtk_window: &gtk::ApplicationWindow, position: OverlayPosition) {
    let (edge, opposite_edge, margin) = match position {
        OverlayPosition::Top => (Edge::Top, Edge::Bottom, OVERLAY_TOP_OFFSET),
        OverlayPosition::Bottom => (Edge::Bottom, Edge::Top, OVERLAY_BOTTOM_OFFSET),
    };

    gtk_window.set_anchor(edge, true);
    gtk_window.set_anchor(opposite_edge, false);
    gtk_window.set_layer_shell_margin(edge, margin.round() as i32);
    gtk_window.set_layer_shell_margin(opposite_edge, 0);
}

/// Configures a GTK layer surface: its size, and its edge and offset.
///
/// Tauri's normal `set_size` path calls `gtk_window_resize`, but layer surfaces
/// derive their dimensions from GTK's size request. gtk-layer-shell documents
/// the `set_size_request` + `resize(1, 1)` sequence for forcing a new size, and
/// commits the new size request itself, including while the surface is mapped —
/// so this is also how a visible overlay follows a size-scale change.
#[cfg(target_os = "linux")]
fn configure_layer_shell_surface(
    gtk_window: &gtk::ApplicationWindow,
    position: OverlayPosition,
    width: f64,
    height: f64,
) {
    use gtk::prelude::{GtkWindowExt, WidgetExt};

    configure_layer_shell_position(gtk_window, position);

    gtk_window.set_size_request(
        width.round().max(1.0) as i32,
        height.round().max(1.0) as i32,
    );
    gtk_window.resize(1, 1);
}

/// Initializes GTK layer shell for Linux overlay window
/// Returns true if layer shell was successfully initialized, false otherwise
#[cfg(target_os = "linux")]
fn init_gtk_layer_shell(overlay_window: &tauri::webview::WebviewWindow) -> bool {
    if utils::env_flag_enabled("HANDY_NO_GTK_LAYER_SHELL") {
        debug!("Skipping GTK layer shell init (HANDY_NO_GTK_LAYER_SHELL is enabled)");
        return false;
    }

    if !gtk_layer_shell::is_supported() {
        return false;
    }

    // Try to get the GTK window from the Tauri webview
    if let Ok(gtk_window) = overlay_window.gtk_window() {
        gtk_window.init_layer_shell();
        gtk_window.set_layer(Layer::Overlay);
        gtk_window.set_keyboard_mode(KeyboardMode::None);
        gtk_window.set_exclusive_zone(0);

        let app_handle = overlay_window.app_handle();
        let overlay_position = settings::get_settings(app_handle).overlay_position;
        let (width, height) = initial_overlay_dimensions(app_handle);
        configure_layer_shell_surface(&gtk_window, overlay_position, width, height);

        let initialized = gtk_window.is_layer_window();
        LAYER_SHELL_ACTIVE.store(initialized, Ordering::SeqCst);
        return initialized;
    }
    false
}

/// Forces a window to be topmost using Win32 API (Windows only)
/// This is more reliable than Tauri's set_always_on_top which can be overridden
#[cfg(target_os = "windows")]
fn force_overlay_topmost(overlay_window: &tauri::webview::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };

    // Clone because run_on_main_thread takes 'static
    let overlay_clone = overlay_window.clone();

    // Make sure the Win32 call happens on the UI thread
    let _ = overlay_clone.clone().run_on_main_thread(move || {
        if let Ok(hwnd) = overlay_clone.hwnd() {
            unsafe {
                // Force Z-order: make this window topmost without changing size/pos or stealing focus
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    });
}

fn get_monitor_with_cursor(app_handle: &AppHandle) -> Option<tauri::Monitor> {
    if let Some(mouse_location) = input::get_cursor_position(app_handle) {
        if let Ok(monitors) = app_handle.available_monitors() {
            for monitor in monitors {
                // On Windows both the cursor (enigo -> GetCursorPos) and the
                // monitor bounds are physical pixels, so compare them directly.
                #[cfg(target_os = "windows")]
                if is_mouse_within_monitor(mouse_location, monitor.position(), monitor.size()) {
                    return Some(monitor);
                }

                // macOS/Linux: enigo returns logical coords, so scale the bounds down.
                #[cfg(not(target_os = "windows"))]
                {
                    let scale = monitor.scale_factor();
                    let pos = PhysicalPosition::new(
                        (monitor.position().x as f64 / scale) as i32,
                        (monitor.position().y as f64 / scale) as i32,
                    );
                    let size = PhysicalSize::new(
                        (monitor.size().width as f64 / scale) as u32,
                        (monitor.size().height as f64 / scale) as u32,
                    );
                    if is_mouse_within_monitor(mouse_location, &pos, &size) {
                        return Some(monitor);
                    }
                }
            }
        }
    }

    app_handle.primary_monitor().ok().flatten()
}

fn is_mouse_within_monitor(
    mouse_pos: (i32, i32),
    monitor_pos: &PhysicalPosition<i32>,
    monitor_size: &PhysicalSize<u32>,
) -> bool {
    let (mouse_x, mouse_y) = mouse_pos;
    let PhysicalPosition {
        x: monitor_x,
        y: monitor_y,
    } = *monitor_pos;
    let PhysicalSize {
        width: monitor_width,
        height: monitor_height,
    } = *monitor_size;

    mouse_x >= monitor_x
        && mouse_x < (monitor_x + monitor_width as i32)
        && mouse_y >= monitor_y
        && mouse_y < (monitor_y + monitor_height as i32)
}

/// Returns overlay position in logical coordinates (points on macOS).
///
/// The Bottom anchor uses the macOS work area (visibleFrame) so the overlay
/// tracks the Dock — above it when shown, at the screen edge when hidden.
/// This relies on tauri 2.11's work_area.position.y fix (#14655), the same
/// bug that led PR #969 to abandon work_area for full monitor bounds. Top and
/// the other platforms keep full monitor bounds plus the fixed offsets
/// (work_area is unreliable on Wayland; Windows' offset clears the taskbar).
///
/// We must use LogicalPosition (not PhysicalPosition) because Tauri/tao
/// converts PhysicalPosition using the scale factor of the monitor the window
/// is *currently* on, which is wrong when moving cross-monitor. Windows uses
/// `place_windows_overlay` instead (no single logical space across mixed DPI).
fn calculate_overlay_position(
    app_handle: &AppHandle,
    width: f64,
    height: f64,
) -> Option<(f64, f64)> {
    let monitor = get_monitor_with_cursor(app_handle)?;
    let scale = monitor.scale_factor();
    let monitor_x = monitor.position().x as f64 / scale;
    let monitor_y = monitor.position().y as f64 / scale;
    let monitor_width = monitor.size().width as f64 / scale;

    let settings = settings::get_settings(app_handle);

    let x = monitor_x + (monitor_width - width) / 2.0;
    let y = match settings.overlay_position {
        OverlayPosition::Top => monitor_y + OVERLAY_TOP_OFFSET,
        OverlayPosition::Bottom => {
            // work_area.position shares monitor.position's global coordinate
            // space, so no monitor offset is added.
            #[cfg(target_os = "macos")]
            let bottom = {
                let wa = monitor.work_area();
                (wa.position.y as f64 + wa.size.height as f64) / scale
            };
            #[cfg(not(target_os = "macos"))]
            let bottom = monitor_y + monitor.size().height as f64 / scale;

            bottom - height - OVERLAY_BOTTOM_OFFSET
        }
    };

    Some((x, y))
}

/// Overlay rectangle in the destination monitor's physical pixels, so nothing
/// is converted through the window's previous-monitor DPI.
#[cfg(target_os = "windows")]
fn windows_overlay_bounds(
    monitor_position: PhysicalPosition<i32>,
    monitor_size: PhysicalSize<u32>,
    scale: f64,
    logical_width: f64,
    logical_height: f64,
    overlay_position: OverlayPosition,
) -> (i32, i32, i32, i32) {
    let width = (logical_width * scale).round().max(1.0) as i32;
    let height = (logical_height * scale).round().max(1.0) as i32;
    let x = (monitor_position.x as f64 + (monitor_size.width as f64 - width as f64) / 2.0).round()
        as i32;
    let y = match overlay_position {
        OverlayPosition::Top => {
            (monitor_position.y as f64 + OVERLAY_TOP_OFFSET * scale).round() as i32
        }
        OverlayPosition::Bottom => (monitor_position.y as f64 + monitor_size.height as f64
            - height as f64
            - OVERLAY_BOTTOM_OFFSET * scale)
            .round() as i32,
    };

    (x, y, width, height)
}

/// Moves and sizes the overlay in one native SetWindowPos, bypassing tao's
/// current-DPI logical conversion that mislands cross-monitor moves.
#[cfg(target_os = "windows")]
fn place_windows_overlay(
    app_handle: &AppHandle,
    overlay_window: &tauri::webview::WebviewWindow,
    logical_width: f64,
    logical_height: f64,
) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};

    let monitor = get_monitor_with_cursor(app_handle)
        .ok_or_else(|| "failed to determine the monitor containing the cursor".to_string())?;
    let (x, y, width, height) = windows_overlay_bounds(
        *monitor.position(),
        *monitor.size(),
        monitor.scale_factor(),
        logical_width,
        logical_height,
        settings::get_settings(app_handle).overlay_position,
    );
    let hwnd = overlay_window
        .hwnd()
        .map_err(|error| format!("failed to get overlay window handle: {error}"))?;

    unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .map_err(|error| format!("failed to set overlay bounds: {error}"))?;
    }

    log::debug!(
        "windows overlay bounds: x={} y={} width={} height={} scale={}",
        x,
        y,
        width,
        height,
        monitor.scale_factor()
    );
    Ok(())
}

/// Creates the recording overlay window and keeps it hidden by default
#[cfg(not(target_os = "macos"))]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    // Created at the compact size for the scale in effect. Every show resizes
    // the window anyway; starting at the right size saves the first show one
    // pointless resize.
    let (width, height) = initial_overlay_dimensions(app_handle);

    // On Linux (Wayland), monitor detection often fails, but we don't need exact coordinates
    // for Layer Shell as we use anchors. On other platforms, we require a monitor.
    #[cfg(not(target_os = "linux"))]
    {
        let position = calculate_overlay_position(app_handle, width, height);
        if position.is_none() {
            debug!("Failed to determine overlay position, not creating overlay window");
            return;
        }
    }

    // Position starts unset — update_overlay_position() sets the correct
    // LogicalPosition before the overlay is shown.
    let mut builder = WebviewWindowBuilder::new(
        app_handle,
        "recording_overlay",
        tauri::WebviewUrl::App("src/overlay/index.html".into()),
    )
    .title("Recording")
    .resizable(false)
    .inner_size(width, height)
    .shadow(false)
    .maximizable(false)
    .minimizable(false)
    .closable(false)
    .accept_first_mouse(true)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .transparent(true)
    .focusable(false)
    .focused(false)
    .visible(false);

    if let Some(data_dir) = crate::portable::data_dir() {
        builder = builder.data_directory(data_dir.join("webview"));
    }

    #[allow(unused_variables)]
    match builder.build() {
        Ok(window) => {
            // Installs the native blur behind the webview, hidden until a
            // Glass show reveals it. A no-op on every platform this branch
            // compiles for, called anyway so the Glass module has exactly one
            // install site per creation path.
            crate::overlay_glass::install(app_handle);

            #[cfg(target_os = "linux")]
            {
                // Try to initialize GTK layer shell, ignore errors if compositor doesn't support it
                if init_gtk_layer_shell(&window) {
                    debug!("GTK layer shell initialized for overlay window");
                } else {
                    debug!("GTK layer shell not available, falling back to regular window");
                }
            }

            debug!("Recording overlay window created successfully (hidden)");
        }
        Err(e) => {
            debug!("Failed to create recording overlay window: {}", e);
        }
    }
}

/// Creates the recording overlay panel and keeps it hidden by default (macOS)
#[cfg(target_os = "macos")]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    // Created at the compact size for the scale in effect. Every show resizes
    // the panel anyway; starting at the right size saves the first show one
    // pointless resize.
    let (width, height) = initial_overlay_dimensions(app_handle);

    if let Some((x, y)) = calculate_overlay_position(app_handle, width, height) {
        // PanelBuilder creates a Tauri window then converts it to NSPanel.
        // The window remains registered, so get_webview_window() still works.
        match PanelBuilder::<_, RecordingOverlayPanel>::new(app_handle, "recording_overlay")
            .url(WebviewUrl::App("src/overlay/index.html".into()))
            .title("Recording")
            .position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .level(PanelLevel::Status)
            .size(tauri::Size::Logical(tauri::LogicalSize { width, height }))
            .has_shadow(false)
            .transparent(true)
            .no_activate(true)
            .corner_radius(0.0)
            .style_mask(StyleMask::empty().borderless().nonactivating_panel())
            .with_window(|w| w.decorations(false).transparent(true).focusable(false))
            .collection_behavior(
                CollectionBehavior::new()
                    .can_join_all_spaces()
                    .full_screen_auxiliary(),
            )
            .build()
        {
            Ok(panel) => {
                // Installs the native blur behind the webview. It is created
                // hidden and stays hidden until a Glass show reveals it, so
                // the first frame of a session can never be a blurred
                // rectangle without a card on it (nspanel#94).
                crate::overlay_glass::install(app_handle);
                panel.hide();
            }
            Err(e) => {
                log::error!("Failed to create recording overlay panel: {}", e);
            }
        }
    }
}

fn show_overlay_state(app_handle: &AppHandle, state: &str) {
    // Whether the overlay shows at all is governed by overlay_style; position
    // only chooses Top vs Bottom placement. Checked here (off the main thread)
    // so the common overlay-disabled case never pays for a main-thread hop.
    let settings = settings::get_settings(app_handle);
    if settings.overlay_style == OverlayStyle::None {
        return;
    }

    // How much room the card needs, and which Material to render it in, both
    // come from the resolved overlay theme. Resolving re-reads the theme
    // file, so it happens here, on the calling thread, and only the result
    // crosses to the main thread; it is handed the tokens from the settings
    // just read above, so this path deserializes the store once, not twice.
    // Every show re-resolves, so a scale or Material changed since the last
    // one is in effect on the first frame.
    let resolved = crate::overlay_theme::resolve_reloading_for(app_handle, settings.overlay_theme);

    // The rest queries monitors and the cursor and mutates window geometry. On
    // Linux the monitor/cursor lookups hit GDK/Xlib on the process's shared X11
    // connection, which is only safe from the GTK main thread — running them on
    // a background thread corrupts the connection and hard-crashes the app
    // (issue #227). Hop to the main thread on every platform to keep the
    // geometry path uniform (a no-op cost on Windows, and it also keeps macOS's
    // NSScreen access main-thread-correct). run_on_main_thread runs the closure
    // inline when already on the main thread, so this never deadlocks.
    let handle = app_handle.clone();
    let state = state.to_string();
    let _ = app_handle
        .run_on_main_thread(move || show_overlay_state_on_main(&handle, &state, resolved));
}

fn show_overlay_state_on_main(
    app_handle: &AppHandle,
    state: &str,
    resolved: crate::overlay_theme::ResolvedOverlayTheme,
) {
    let material = resolved.effective_material;
    let scale = resolved.theme.size_scale();

    // The shape is recorded before anything else — including under Flat,
    // where nothing reads it until a possible later Glass switch — so a
    // reposition landing mid-session always has the card actually on screen
    // to size from, never a stale default.
    let shape = initial_card_shape(state);
    set_current_card_shape(shape);
    // Hides the glass view immediately if this show is Flat (in case a
    // previous Glass session left it visible); under Glass it changes
    // nothing, so the blur cannot appear before the card paints.
    crate::overlay_glass::apply_material(app_handle, material);

    let (width, height) = overlay_dimensions(shape, scale, material);
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Invalidate any delayed hide still in flight from a previous session
        // (see `hide_recording_overlay`).
        OVERLAY_SHOW_GENERATION.fetch_add(1, Ordering::SeqCst);
        OVERLAY_SESSION_ACTIVE.store(true, Ordering::SeqCst);

        #[cfg(target_os = "linux")]
        let shown_with_layer_shell = if LAYER_SHELL_ACTIVE.load(Ordering::SeqCst) {
            let position = settings::get_settings(app_handle).overlay_position;
            match overlay_window.gtk_window() {
                Ok(gtk_window) => {
                    configure_layer_shell_surface(&gtk_window, position, width, height)
                }
                Err(error) => log::error!("Failed to access GTK overlay window: {error}"),
            }
            let _ = overlay_window.show();
            true
        } else {
            false
        };
        #[cfg(not(target_os = "linux"))]
        let shown_with_layer_shell = false;

        if !shown_with_layer_shell {
            let size_started = std::time::Instant::now();
            #[cfg(not(target_os = "windows"))]
            let _ =
                overlay_window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
            let size_elapsed = size_started.elapsed();

            let pos_started = std::time::Instant::now();
            #[cfg(not(target_os = "windows"))]
            let set_pos_elapsed =
                if let Some((x, y)) = calculate_overlay_position(app_handle, width, height) {
                    let set_pos_started = std::time::Instant::now();
                    let _ = overlay_window
                        .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
                    set_pos_started.elapsed()
                } else {
                    std::time::Duration::ZERO
                };
            #[cfg(target_os = "windows")]
            let set_pos_elapsed = {
                let set_pos_started = std::time::Instant::now();
                if let Err(error) =
                    place_windows_overlay(app_handle, &overlay_window, width, height)
                {
                    log::error!("Failed to place recording overlay: {error}");
                }
                set_pos_started.elapsed()
            };
            let pos_calc_elapsed = pos_started.elapsed() - set_pos_elapsed;

            let show_started = std::time::Instant::now();
            let _ = overlay_window.show();
            let show_elapsed = show_started.elapsed();

            // On Windows, aggressively re-assert "topmost" in the native Z-order after showing
            #[cfg(target_os = "windows")]
            force_overlay_topmost(&overlay_window);

            // Re-assert bounds after show(): the pre-show move crosses the DPI
            // boundary, and tao's WM_DPICHANGED reflow clobbers the first placement.
            #[cfg(target_os = "windows")]
            if let Err(error) = place_windows_overlay(app_handle, &overlay_window, width, height) {
                log::error!("Failed to re-assert recording overlay position: {error}");
            }

            log::debug!(
                "overlay '{}': set_size={:?} pos_calc={:?} set_pos={:?} show={:?}",
                state,
                size_elapsed,
                pos_calc_elapsed,
                set_pos_elapsed,
                show_elapsed
            );
        }

        let _ = overlay_window.emit("show-overlay", state);

        // The glass view is revealed by the webview's first card-shape report
        // for this session, not here: at this point the webview has only just
        // been handed `show-overlay` and is still fetching the resolved theme,
        // so revealing now would put a blurred rectangle on screen before the
        // card painted into it. This only arms the fallback for a webview that
        // never reports at all. A no-op under Flat and off macOS.
        if material == Material::Glass {
            let radius = shape_radius(shape, radius_token_px(&resolved.theme), scale);
            schedule_glass_fallback_reveal(app_handle, radius);
        }
    }
}

/// True from the moment a show maps the overlay window until the hide that
/// ends that session is requested. Read by the Glass fallback reveal, which
/// must not fade a blur in on a card that is already fading out — the show
/// generation alone cannot tell that apart, because a hide does not start a
/// new session.
static OVERLAY_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// How long the show path waits for the overlay webview's first card-shape
/// report before revealing the glass view itself: twice the card's own fade,
/// which is well past the webview's first paint on any machine that renders
/// the card at all.
const GLASS_FALLBACK_REVEAL_MS: u64 = CARD_FADE_MS as u64 * 2;

/// Reveal the glass view a short while after a Glass show, unless the webview
/// has already had it revealed by reporting its card shape.
///
/// The reveal belongs to the first card-shape report, which is the only
/// moment Rust knows the card has painted. This is the safety net for a
/// webview that never reports — an overlay page left over from an older
/// build, or one whose script failed — which would otherwise show a
/// completely transparent window under Glass.
///
/// Guarded by [`OVERLAY_SHOW_GENERATION`], exactly like the delayed hide is,
/// so a newer session's reveal cannot be undone by an older one's; and by
/// [`OVERLAY_SESSION_ACTIVE`], so a session that ended inside the delay never
/// has a blur faded in on its way out. Revealing twice is harmless:
/// [`crate::overlay_glass::show_glass`] only updates the radius when the view
/// is already fully visible.
fn schedule_glass_fallback_reveal(app_handle: &AppHandle, radius: f64) {
    let scheduled_at = OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst);
    let app_handle = app_handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(GLASS_FALLBACK_REVEAL_MS));
        if OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst) != scheduled_at
            || !OVERLAY_SESSION_ACTIVE.load(Ordering::SeqCst)
        {
            log::debug!("Skipping stale Glass reveal: this session is no longer on screen");
            return;
        }
        crate::overlay_glass::show_glass(&app_handle, radius);
    });
}

/// Notify the visible recording overlay that the input stream has delivered its
/// first sample chunk. Audio feedback uses the same backend readiness signal,
/// but this targeted event is skipped when overlays are disabled.
pub fn emit_recording_ready(app_handle: &AppHandle) {
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Showing the overlay is also queued onto the main thread. Queue readiness
    // there as well so a very fast always-on stream cannot overtake show-overlay
    // and then get reset back to the arming state by the frontend.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let _ = handle.emit_to("recording_overlay", "recording-ready", ());
    });
}

/// Shows the recording overlay window with fade-in animation
pub fn show_recording_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "recording");
}

/// Shows the larger streaming overlay that displays live transcription text
pub fn show_streaming_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "streaming");
}

/// Shows the transcribing overlay window
pub fn show_transcribing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "transcribing");
}

/// Shows the processing overlay window
pub fn show_processing_overlay(app_handle: &AppHandle) {
    show_overlay_state(app_handle, "processing");
}

/// Updates the overlay window position and size from the current settings.
///
/// For callers that changed something other than the overlay theme — the
/// position and style commands — and therefore have no resolved scale in hand.
pub fn update_overlay_position(app_handle: &AppHandle) {
    update_overlay_window(app_handle, None);
}

/// [`update_overlay_position`] with the size scale the caller already resolved.
///
/// The overlay-theme delivery path holds the resolved theme, so passing the
/// scale here keeps the main-thread hop from resolving it a second time.
pub fn update_overlay_position_with_scale(app_handle: &AppHandle, scale: f64) {
    update_overlay_window(app_handle, Some(scale));
}

fn update_overlay_window(app_handle: &AppHandle, scale: Option<f64>) {
    // Positioning queries monitors/cursor (GDK/Xlib on Linux) and moves the
    // window, so it must run on the main thread — see show_overlay_state.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || update_overlay_position_on_main(&handle, scale));
}

fn update_overlay_position_on_main(app_handle: &AppHandle, scale: Option<f64>) {
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Resolved fresh on every reposition (cache-only, no filesystem IO,
        // safe on the main thread) so the Material is always current — this
        // path is reached from the position and style commands too, which
        // hand in no scale and have no resolved theme of their own to pass.
        let resolved = crate::overlay_theme::resolve(app_handle);
        let material = resolved.effective_material;
        // Hides the glass view when this reposition lands on Flat (in case a
        // previous Glass session left it visible); a no-op otherwise.
        crate::overlay_glass::apply_material(app_handle, material);

        // Every platform recomputes the size from the card on screen and the
        // size scale, rather than reading the window's current size back from
        // the OS. A scale change therefore resizes the window even while it is
        // visible — without it the card would repaint larger inside the old
        // window and be clipped.
        let scale = scale.unwrap_or_else(|| resolved.theme.size_scale());
        let shape = current_card_shape();
        let (width, height) = overlay_dimensions(shape, scale, material);

        #[cfg(target_os = "linux")]
        if LAYER_SHELL_ACTIVE.load(Ordering::SeqCst) {
            let position = settings::get_settings(app_handle).overlay_position;
            match overlay_window.gtk_window() {
                // Layer surfaces size themselves from GTK's size request, so
                // the full configure (size request + anchors) is what applies a
                // new size here.
                Ok(gtk_window) => {
                    configure_layer_shell_surface(&gtk_window, position, width, height)
                }
                Err(error) => log::error!("Failed to access GTK overlay window: {error}"),
            }
            return;
        }

        #[cfg(target_os = "windows")]
        if let Err(error) = place_windows_overlay(app_handle, &overlay_window, width, height) {
            log::error!("Failed to update recording overlay position: {error}");
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ =
                overlay_window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
            if let Some((x, y)) = calculate_overlay_position(app_handle, width, height) {
                let _ = overlay_window
                    .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
            }
        }

        // The window now matches `shape` at `scale`; bring the glass view's
        // corner radius in line with it — but only while the card is actually
        // on screen. Revealing the blur on a hidden window would leave it at
        // full alpha for the next show to flash before the card paints, and a
        // reposition is common while the overlay is down (every theme, style
        // and position change makes one). The radius is recomputed by the
        // first card-shape report of the next session anyway. This whole
        // block is a no-op under Flat and off macOS; `is_visible` is an
        // AppKit read, and this function already runs on the main thread.
        if material == Material::Glass && overlay_window.is_visible().unwrap_or(false) {
            let radius = shape_radius(shape, radius_token_px(&resolved.theme), scale);
            crate::overlay_glass::show_glass(app_handle, radius);
        }
    }
}

/// Record a card-shape report from the overlay webview and, under Glass,
/// animate or reveal the native blur to match.
///
/// This is also where a Glass session's blur is first revealed: the report
/// that arrives on the first frame of a session repeats the shape the show
/// path seeded, so it takes the reveal branch below — the earliest moment
/// Rust knows the card has actually painted.
///
/// Coalesced by shape identity, never by time: a report that repeats the
/// shape already on screen only refreshes the radius and the reveal, and an
/// in-flight animation superseded by a new shape is left to AppKit's own
/// `animator()` to retarget rather than cancelled by hand. Under Flat — and
/// off macOS, where the effective Material is never Glass — this only updates
/// the stored shape, which `update_overlay_position_on_main` reads back on
/// the next reposition.
pub fn set_card_shape(app_handle: &AppHandle, shape: OverlayCardShape, duration_ms: u32) {
    let previous = set_current_card_shape(shape);

    let resolved = crate::overlay_theme::resolve(app_handle);
    if resolved.effective_material != Material::Glass {
        return;
    }

    let scale = resolved.theme.size_scale();
    let size = overlay_dimensions(shape, scale, resolved.effective_material);
    let radius = shape_radius(shape, radius_token_px(&resolved.theme), scale);

    // Whether the window is mapped decides between animating the frame and
    // snapping it, and `is_visible()` is an AppKit read on macOS — so it is
    // taken on the main thread, in the same hop that acts on it, rather than
    // on whichever thread the command handler landed on.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let visible = handle
            .get_webview_window("recording_overlay")
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        if previous != shape && visible {
            crate::overlay_glass::morph_frame(&handle, size, radius, duration_ms);
        } else {
            crate::overlay_glass::show_glass(&handle, radius);
        }
    });
}

/// How long the glass view fades out for when the overlay hides, matched to
/// the way this card actually leaves the screen: the compact pill fades with
/// its container over `--ov-fade-ms`, while the Live card is unmounted
/// outright, so its blur has to go at once rather than linger over an empty
/// window.
fn glass_fade_out_ms(shape: OverlayCardShape) -> u32 {
    match shape.form() {
        OverlayForm::Compact => CARD_FADE_MS,
        OverlayForm::Live => 0,
    }
}

/// Generation counter bumped every time the overlay is shown. The delayed
/// `hide()` below only unmaps the window if no show happened after it was
/// scheduled, so a hide left over from a finished transcription can never
/// take down the overlay of a session that started in the meantime — e.g. a
/// press the coordinator remembered while the pipeline was busy and started
/// the instant it drained, well inside the 300 ms hide delay.
static OVERLAY_SHOW_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Hides the recording overlay window with fade-out animation
pub fn hide_recording_overlay(app_handle: &AppHandle) {
    // Always hide the overlay regardless of settings - if setting was changed while recording,
    // we still want to hide it properly
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Snapshot before doing anything observable, so any show that lands
        // after this point invalidates the delayed hide below.
        let scheduled_at = OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst);
        // This session is over as far as the Glass reveal is concerned, even
        // though the window stays mapped for the fade below.
        OVERLAY_SESSION_ACTIVE.store(false, Ordering::SeqCst);
        // Emit event to trigger fade-out animation
        let _ = overlay_window.emit("hide-overlay", ());
        // Under Glass the blur is a native layer that takes no part in the
        // card's own exit, so it has to be driven out on the same timing or
        // it sits on screen with nothing in it. A no-op under Flat and when
        // the blur was never revealed.
        crate::overlay_glass::fade_out(app_handle, glass_fade_out_ms(current_card_shape()));
        // Hide the window after a short delay to allow animation to complete,
        // unless a newer session has shown the overlay again by then.
        let window_clone = overlay_window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            if OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst) != scheduled_at {
                log::debug!("Skipping stale overlay hide: a newer session is showing the overlay");
                return;
            }
            // Nothing is on screen any more, so the window has no shape to
            // keep. Cleared here rather than when the hide is scheduled, so a
            // show that lands inside the 300 ms delay keeps the shape it just
            // set (ticket 13's slice 3 review note: under zero slack, a stale
            // shape left over from a finished Live session must not be what
            // the next reposition sizes the window from).
            set_current_card_shape(OverlayCardShape::CompactRest);
            let _ = window_clone.hide();
        });
    }
}

// Cached "overlay is enabled" flag, kept in sync with overlay_style. Avoids
// reading the Tauri store on every audio callback (~24 Hz during recording).
// Defaults to false so the audio path doesn't emit until lib.rs::setup
// populates the cache from initial settings.
static OVERLAY_ENABLED: AtomicBool = AtomicBool::new(false);

/// Tracks whether gtk-layer-shell was successfully initialized (Linux only).
/// Used to skip layer-shell calls when the window is a regular fallback.
#[cfg(target_os = "linux")]
static LAYER_SHELL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Update the cached overlay-enabled flag. Called from `lib.rs` at
/// startup after settings load, and from `change_overlay_style_setting`
/// whenever the user changes whether the overlay is shown.
pub fn update_overlay_enabled_cache(enabled: bool) {
    OVERLAY_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn emit_levels(app_handle: &AppHandle, levels: &[f32]) {
    // Skip emission when the overlay is disabled. The recording_overlay
    // window is created at boot regardless of overlay_style, so without this
    // guard a hidden overlay's WebKit subprocess still
    // processes every event. Each event drives some kind of WebKit
    // C++ allocation that accumulates without bound (mechanism not
    // directly characterized; see issue #1279 for the investigation).
    // For users with `overlay_style: none` (the Linux default) this skip
    // eliminates the upstream driver of that accumulation.
    if !OVERLAY_ENABLED.load(Ordering::Relaxed) {
        return;
    }

    // Throttle to ~30 FPS. Even with the overlay enabled, the raw audio
    // callback fires far faster than the UI needs; capping emission rate
    // cuts the per-frame `eval_script`/IPC volume that drives the wry
    // memory growth in issue #1279 (upstream tauri-apps/wry#1489).
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let last = LAST_MIC_LEVEL_EMIT.load(Ordering::Relaxed);
    if now.saturating_sub(last) < EMIT_THROTTLE_MS {
        return;
    }
    LAST_MIC_LEVEL_EMIT.store(now, Ordering::Relaxed);

    // Target only the overlay window. In Tauri 2 both `AppHandle::emit`
    // and `WebviewWindow::emit` broadcast to all webviews; Tauri's
    // listener filter then skips webviews with no registered listener
    // for the event, so the settings webview never received `mic-level`.
    // But the previous dual-call pattern still produced two `eval_script`
    // calls to the overlay per audio callback (one from each .emit()).
    // `emit_to` with the overlay's window label produces a single
    // eval_script call per callback, cutting the per-callback WebKit
    // dispatch work in half.
    let _ = app_handle.emit_to("recording_overlay", "mic-level", levels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_hit_test_uses_half_open_physical_bounds() {
        let position = PhysicalPosition::new(-2560, -200);
        let size = PhysicalSize::new(2560, 1440);

        assert!(is_mouse_within_monitor((-2560, -200), &position, &size));
        assert!(is_mouse_within_monitor((-1, 1239), &position, &size));
        assert!(!is_mouse_within_monitor((0, 0), &position, &size));
        assert!(!is_mouse_within_monitor((-1, 1240), &position, &size));
    }

    /// The "defaults reproduce today's overlay exactly" pin: with no size token
    /// set, every state gets the window this overlay has always used. The
    /// literals are the sizes overlay.rs hardcoded before the token existed.
    #[test]
    fn overlay_dimensions_at_scale_one_match_todays_windows() {
        for (state, expected_shape) in [
            ("recording", OverlayCardShape::CompactRest),
            ("transcribing", OverlayCardShape::CompactWorking),
            ("processing", OverlayCardShape::CompactWorking),
        ] {
            let shape = initial_card_shape(state);
            assert_eq!(shape, expected_shape, "{state}");
            assert_eq!(shape.form(), OverlayForm::Compact, "{state}");
            assert_eq!(
                overlay_dimensions(shape, 1.0, Material::Flat),
                (256.0, 46.0),
                "{state}"
            );
        }
        assert_eq!(initial_card_shape("streaming"), OverlayCardShape::LivePill);
        assert_eq!(OverlayCardShape::LivePill.form(), OverlayForm::Live);
        assert_eq!(
            overlay_dimensions(OverlayCardShape::LivePill, 1.0, Material::Flat),
            (400.0, 120.0)
        );

        // The same four numbers as named fixtures, so the Windows bounds tests
        // below keep exercising today's sizes.
        assert_eq!((OVERLAY_WIDTH, OVERLAY_HEIGHT), (256.0, 46.0));
        assert_eq!(
            (OVERLAY_STREAM_WIDTH, OVERLAY_STREAM_HEIGHT),
            (400.0, 120.0)
        );
    }

    /// The card scales, the slack does not. Under Flat every shape in a form
    /// produces the same window, so which exact shape is passed is immaterial
    /// — chosen for variety.
    #[test]
    fn overlay_dimensions_scale_with_the_token() {
        assert_eq!(
            overlay_dimensions(OverlayCardShape::CompactRest, 1.5, Material::Flat),
            (365.0, 67.0)
        );
        assert_eq!(
            overlay_dimensions(OverlayCardShape::LivePill, 1.5, Material::Flat),
            (597.0, 179.0)
        );
        assert_eq!(
            overlay_dimensions(OverlayCardShape::CompactRest, 0.8, Material::Flat),
            (213.0, 38.0)
        );
        assert_eq!(
            overlay_dimensions(OverlayCardShape::LivePill, 0.8, Material::Flat),
            (322.0, 97.0)
        );
    }

    /// A scale that reached the geometry unclamped is treated as the nearest
    /// bound, and a number that is no scale at all falls back to 1.
    #[test]
    fn overlay_dimensions_clamp_out_of_range_and_non_finite() {
        assert_eq!(
            overlay_dimensions(OverlayCardShape::LiveOpen, 3.0, Material::Flat),
            (597.0, 179.0)
        );
        assert_eq!(
            overlay_dimensions(OverlayCardShape::CompactWorking, 0.1, Material::Flat),
            (213.0, 38.0)
        );
        for broken in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                overlay_dimensions(OverlayCardShape::CompactRest, broken, Material::Flat),
                (256.0, 46.0)
            );
        }
    }

    /// The invariant Glass must not break when it sets the slack to zero: a
    /// Flat window covers the card at every scale. The card footprints are
    /// the token contract's own numbers (02 §7, 06 §4), not a repeat of the
    /// arithmetic under test.
    #[test]
    fn overlay_window_always_covers_the_card() {
        for (shape, scale, card_width, card_height) in [
            (OverlayCardShape::CompactWorking, 0.80, 175.0, 34.0),
            (OverlayCardShape::LiveOpen, 0.80, 316.0, 95.0),
            (OverlayCardShape::CompactWorking, 1.00, 218.0, 42.0),
            (OverlayCardShape::LiveOpen, 1.00, 394.0, 118.0),
            (OverlayCardShape::CompactWorking, 1.50, 327.0, 63.0),
            (OverlayCardShape::LiveOpen, 1.50, 591.0, 177.0),
        ] {
            let (width, height) = overlay_dimensions(shape, scale, Material::Flat);
            assert!(
                width >= card_width,
                "{shape:?} at {scale}: window {width} is narrower than the card's {card_width}"
            );
            assert!(
                height >= card_height,
                "{shape:?} at {scale}: window {height} is shorter than the card's {card_height}"
            );
        }
    }

    /// Under Glass the window equals the card exactly — zero slack — at
    /// every shape and every 0.05 step of scale from 0.80 to 1.50. The
    /// footprints are spelled out rather than read from `card_footprint()`,
    /// so a constant that drifted cannot agree with itself, and no
    /// expectation here ever adds a slack term.
    #[test]
    fn glass_window_equals_card_at_every_scale() {
        // The Material specification's own table: content plus the 1px
        // hairline on each side, at size_scale 1.
        const CARDS: [(OverlayCardShape, f64, f64); 5] = [
            (OverlayCardShape::CompactRest, 174.0, 42.0),
            (OverlayCardShape::CompactWorking, 218.0, 42.0),
            (OverlayCardShape::LivePill, 186.0, 42.0),
            (OverlayCardShape::LiveWorking, 218.0, 42.0),
            (OverlayCardShape::LiveOpen, 394.0, 118.0),
        ];
        // The same five cards at the token's maximum, also as literals: the
        // one scale where every width lands on a whole point without a
        // rounding step to hide a mistake.
        const CARDS_AT_1_5: [(OverlayCardShape, f64, f64); 5] = [
            (OverlayCardShape::CompactRest, 261.0, 63.0),
            (OverlayCardShape::CompactWorking, 327.0, 63.0),
            (OverlayCardShape::LivePill, 279.0, 63.0),
            (OverlayCardShape::LiveWorking, 327.0, 63.0),
            (OverlayCardShape::LiveOpen, 591.0, 177.0),
        ];

        for (shape, width, height) in CARDS {
            assert_eq!(
                overlay_dimensions(shape, 1.0, Material::Glass),
                (width, height),
                "{shape:?} at 1.0"
            );
        }
        for (shape, width, height) in CARDS_AT_1_5 {
            assert_eq!(
                overlay_dimensions(shape, 1.5, Material::Glass),
                (width, height),
                "{shape:?} at 1.5"
            );
        }

        // Every 0.05 step in between, built from integers so the last step is
        // exactly the 1.50 the geometry clamps to.
        for step in 0..=14 {
            let scale = f64::from(80 + step * 5) / 100.0;
            for (shape, width, height) in CARDS {
                assert_eq!(
                    overlay_dimensions(shape, scale, Material::Glass),
                    ((width * scale).ceil(), (height * scale).ceil()),
                    "{shape:?} at {scale}"
                );
            }
        }
    }

    /// The card shape survives the round trip through the byte the atomic
    /// stores, for every variant, and a byte nothing in this module can
    /// produce falls back instead of panicking.
    #[test]
    fn card_shape_round_trips_through_its_byte() {
        for (index, shape) in OverlayCardShape::ALL.into_iter().enumerate() {
            assert_eq!(OverlayCardShape::from_u8(shape as u8), shape, "{shape:?}");
            // ALL is the table from_u8 indexes into, so it has to stay in
            // discriminant order.
            assert_eq!(shape as usize, index, "{shape:?}");
        }
        for unknown in [OverlayCardShape::ALL.len() as u8, u8::MAX] {
            assert_eq!(
                OverlayCardShape::from_u8(unknown),
                OverlayCardShape::CompactRest,
                "{unknown}"
            );
        }
    }

    /// The blur leaves the screen the way the card does: with the compact
    /// pill's fade, and at once for the Live card, which is unmounted.
    #[test]
    fn glass_fades_out_with_the_card_it_sits_under() {
        assert_eq!(
            glass_fade_out_ms(OverlayCardShape::CompactRest),
            CARD_FADE_MS
        );
        assert_eq!(
            glass_fade_out_ms(OverlayCardShape::CompactWorking),
            CARD_FADE_MS
        );
        assert_eq!(glass_fade_out_ms(OverlayCardShape::LivePill), 0);
        assert_eq!(glass_fade_out_ms(OverlayCardShape::LiveWorking), 0);
        assert_eq!(glass_fade_out_ms(OverlayCardShape::LiveOpen), 0);
    }

    /// Must agree with `cardShape()` in `src/overlay/cardShape.ts` for every
    /// state string the show path can carry.
    #[test]
    fn initial_card_shape_matches_card_shape_ts() {
        assert_eq!(
            initial_card_shape("recording"),
            OverlayCardShape::CompactRest
        );
        assert_eq!(
            initial_card_shape("transcribing"),
            OverlayCardShape::CompactWorking
        );
        assert_eq!(
            initial_card_shape("processing"),
            OverlayCardShape::CompactWorking
        );
        assert_eq!(initial_card_shape("streaming"), OverlayCardShape::LivePill);
    }

    /// The card constants above and the `--ov-*` block in RecordingOverlay.css
    /// are two copies of the same geometry. This test is what keeps them one
    /// number: it reads the CSS the overlay actually ships with and fails
    /// naming the variable that drifted, instead of shipping a clipped card.
    #[test]
    fn overlay_window_constants_match_overlay_css() {
        const CSS: &str = include_str!("../../src/overlay/RecordingOverlay.css");

        /// The number a `--ov-*` custom property is declared with, in the
        /// unit it carries. The needle carries the colon, so `var(--ov-work-w)`
        /// usages never match — only the `:root` declaration does.
        fn css_value(css: &str, name: &str, unit: &str) -> f64 {
            let needle = format!("{name}:");
            let start = css
                .find(&needle)
                .unwrap_or_else(|| panic!("{name} is not declared in RecordingOverlay.css"));
            let rest = &css[start + needle.len()..];
            let end = rest
                .find(unit)
                .unwrap_or_else(|| panic!("{name} is not declared in {unit}"));
            rest[..end]
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("{name} is not a number"))
        }
        fn css_px(css: &str, name: &str) -> f64 {
            css_value(css, name, "px")
        }
        fn css_ms(css: &str, name: &str) -> f64 {
            css_value(css, name, "ms")
        }

        // The card's hairline scales with the rest of it, so a footprint is the
        // declared length plus CARD_BORDER, all times the scale.
        assert_eq!(css_px(CSS, "--ov-work-w") + CARD_BORDER, CARD_COMPACT_W);
        assert_eq!(css_px(CSS, "--ov-base-h") + CARD_BORDER, CARD_COMPACT_H);
        assert_eq!(css_px(CSS, "--ov-open-w") + CARD_BORDER, CARD_LIVE_W);
        assert_eq!(
            css_px(CSS, "--ov-base-h")
                + css_px(CSS, "--ov-cap-max-h")
                + css_px(CSS, "--ov-cap-pad-y")
                + CARD_BORDER,
            CARD_LIVE_H
        );
        // The two shapes only Glass sizes to exactly: the resting compact
        // pill and the Live pill before it opens or collapses.
        assert_eq!(
            css_px(CSS, "--ov-rest-w") + CARD_BORDER,
            CARD_COMPACT_REST_W
        );
        assert_eq!(css_px(CSS, "--ov-pill-w") + CARD_BORDER, CARD_LIVE_PILL_W);

        // The card's own timings. The CSS transitions read these two
        // properties, the overlay webview reads --ov-morph-ms at runtime to
        // tell the backend how long to animate the window frame, and the
        // native reveal and fade-out read the constants below — so all three
        // have to be one number.
        assert_eq!(css_ms(CSS, "--ov-morph-ms"), f64::from(CARD_MORPH_MS));
        assert_eq!(css_ms(CSS, "--ov-fade-ms"), f64::from(CARD_FADE_MS));

        // Both morphs are a grow, so the widest card per form is the one the
        // window is sized from above.
        assert!(css_px(CSS, "--ov-rest-w") <= css_px(CSS, "--ov-work-w"));
        assert!(css_px(CSS, "--ov-pill-w") <= css_px(CSS, "--ov-open-w"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_cursor_hit_test_does_not_scale_physical_monitor_bounds() {
        let position = PhysicalPosition::new(1920, 0);
        let size = PhysicalSize::new(3840, 2160);
        let cursor = (5000, 1000);

        assert!(is_mouse_within_monitor(cursor, &position, &size));

        // This is the old mixed-coordinate comparison. It excludes a cursor
        // that is visibly inside a secondary display running at 150%.
        let scale = 1.5;
        let logical_position = PhysicalPosition::new(
            (position.x as f64 / scale) as i32,
            (position.y as f64 / scale) as i32,
        );
        let logical_size = PhysicalSize::new(
            (size.width as f64 / scale) as u32,
            (size.height as f64 / scale) as u32,
        );
        assert!(!is_mouse_within_monitor(
            cursor,
            &logical_position,
            &logical_size
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_use_destination_monitor_scale() {
        let monitor_position = PhysicalPosition::new(1920, 0);
        let monitor_size = PhysicalSize::new(3840, 2160);

        assert_eq!(
            windows_overlay_bounds(
                monitor_position,
                monitor_size,
                1.5,
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT,
                OverlayPosition::Bottom,
            ),
            (3648, 2031, 384, 69)
        );
        assert_eq!(
            windows_overlay_bounds(
                monitor_position,
                monitor_size,
                1.5,
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT,
                OverlayPosition::Top,
            ),
            (3648, 6, 384, 69)
        );

        // A scaled window converts to physical pixels the same way and still
        // lands on the bottom offset: 2160 - 269 - 40 * 1.5 = 1831.
        let (width, height) = overlay_dimensions(OverlayCardShape::LiveOpen, 1.5, Material::Flat);
        assert_eq!(
            windows_overlay_bounds(
                monitor_position,
                monitor_size,
                1.5,
                width,
                height,
                OverlayPosition::Bottom,
            ),
            (3392, 1831, 896, 269)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_overlay_bounds_support_negative_monitor_origins() {
        assert_eq!(
            windows_overlay_bounds(
                PhysicalPosition::new(-2560, -200),
                PhysicalSize::new(2560, 1440),
                1.25,
                OVERLAY_STREAM_WIDTH,
                OVERLAY_STREAM_HEIGHT,
                OverlayPosition::Bottom,
            ),
            (-1530, 1040, 500, 150)
        );
    }
}
