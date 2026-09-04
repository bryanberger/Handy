use crate::input;
use crate::overlay_geometry::{
    native_update_needed, MonitorBounds, OverlayCardShape, OverlayWindowState, CARD_FADE_MS,
};
use crate::overlay_theme::Material;
use crate::settings;
use crate::settings::{OverlayPosition, OverlayStyle};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Listener, Manager, PhysicalPosition, PhysicalSize};

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

/// The shape the overlay window is sized for: the one most recently shown or
/// reported by the webview, or [`OverlayCardShape::CompactRest`] before the
/// first show and once the overlay is fully hidden (see `hide_recording_overlay`).
///
/// Under zero-slack Glass the window tracks the exact shape, not just compact
/// versus Live. Which card is showing is recoverable, so one atomic covers both.
static OVERLAY_CARD_SHAPE: AtomicU8 = AtomicU8::new(OverlayCardShape::CompactRest as u8);

/// The shape the overlay window is currently sized for.
fn current_card_shape() -> OverlayCardShape {
    OverlayCardShape::from_u8(OVERLAY_CARD_SHAPE.load(Ordering::SeqCst))
}

/// Store a new shape, returning the one it replaced. [`set_card_shape`] uses
/// that to coalesce reports by identity rather than by time.
fn set_current_card_shape(shape: OverlayCardShape) -> OverlayCardShape {
    OverlayCardShape::from_u8(OVERLAY_CARD_SHAPE.swap(shape as u8, Ordering::SeqCst))
}

/// The compact window at the scale in effect: the overlay window's created
/// size, and its reach past the card towards the anchored screen edge, before
/// any card has been shown.
///
/// Always sized for Flat, because at window-creation time
/// `overlay_glass::install` has not run yet (it needs the window this
/// function's caller is about to build), so Glass can never be available here
/// whatever is persisted. The first real show (`show_overlay_state_on_main`)
/// resolves fresh and resizes correctly once Glass is installed, the same
/// "cosmetic first-show resize" tradeoff the theme file's launch-time read
/// already accepts.
///
/// Resolves from the theme-file cache with no filesystem IO, so it is safe on
/// the main thread, where the geometry runs. The show path re-reads the file
/// instead, off the main thread.
///
/// Refreshes the cached anchored edge first, because the resolve measures
/// `edge_margin` against it: a window created before any show would otherwise
/// be placed at the other edge's inherited margin.
fn initial_overlay_window(app_handle: &AppHandle) -> OverlayWindowState {
    let position = settings::get_settings(app_handle).overlay_position;
    update_overlay_position_cache(position);
    OverlayWindowState::initial(&crate::overlay_theme::resolve(app_handle), position)
}

static LAST_MIC_LEVEL_EMIT: AtomicU64 = AtomicU64::new(0);
const EMIT_THROTTLE_MS: u64 = 33; // ~30 FPS

/// The screen edge the overlay is anchored to, cached from the
/// `overlay_position` setting.
///
/// The theme-draft path reads it once per animation frame while the Appearance
/// tab's sliders are dragged, too often for the full settings deserialize
/// `settings::get_settings` costs. Same bargain as [`OVERLAY_ENABLED`], read by
/// the audio callback at ~24 Hz. Seeded from the store in `lib.rs`'s setup,
/// kept honest by `shortcut::change_overlay_position_setting` and refreshed on
/// every placement, so a stale value corrects itself at the next show.
static OVERLAY_POSITION: AtomicU8 = AtomicU8::new(OverlayPosition::Bottom as u8);

/// The edge the overlay is anchored to right now.
pub fn current_overlay_position() -> OverlayPosition {
    // Only the two discriminants below are ever stored.
    if OVERLAY_POSITION.load(Ordering::Relaxed) == OverlayPosition::Top as u8 {
        OverlayPosition::Top
    } else {
        OverlayPosition::Bottom
    }
}

/// Update the cached anchored edge, from `lib.rs`'s setup after the settings
/// load, from `change_overlay_position_setting`, and from both placement paths,
/// which have read the setting anyway.
pub fn update_overlay_position_cache(position: OverlayPosition) {
    OVERLAY_POSITION.store(position as u8, Ordering::Relaxed);
}

/// The gtk-layer-shell edge a screen edge names.
#[cfg(target_os = "linux")]
fn layer_shell_edge(edge: crate::overlay_geometry::ScreenEdge) -> Edge {
    match edge {
        crate::overlay_geometry::ScreenEdge::Top => Edge::Top,
        crate::overlay_geometry::ScreenEdge::Bottom => Edge::Bottom,
    }
}

/// Configures the edge and margin of a GTK layer surface. gtk-layer-shell
/// commits anchor and margin changes itself, including while the surface is
/// mapped, so changing position does not require a manual hide/show cycle.
///
/// The compositor measures the anchored margin from the area other clients'
/// exclusive zones leave free, so `margin` is a distance from the *usable* edge
/// with nothing subtracted here. That holds only while Handy's exclusive zone
/// stays 0 (`init_gtk_layer_shell`) and the automatic one is off; either would
/// count the margin twice and push every other desktop window by it.
#[cfg(target_os = "linux")]
fn configure_layer_shell_position(
    gtk_window: &gtk::ApplicationWindow,
    position: OverlayPosition,
    margin: u16,
    edge_slack: f64,
) {
    let placement = crate::overlay_geometry::layer_shell_placement(position, margin, edge_slack);
    let edge = layer_shell_edge(placement.anchor);
    let opposite_edge = layer_shell_edge(placement.anchor.opposite());

    gtk_window.set_anchor(edge, true);
    gtk_window.set_anchor(opposite_edge, false);
    gtk_window.set_layer_shell_margin(edge, placement.margin);
    gtk_window.set_layer_shell_margin(opposite_edge, placement.opposite_margin);
}

/// Configures a GTK layer surface's size, edge and offset.
///
/// Tauri's normal `set_size` path calls `gtk_window_resize`, but layer surfaces
/// derive their dimensions from GTK's size request. gtk-layer-shell documents
/// the `set_size_request` + `resize(1, 1)` sequence for forcing a new size, and
/// commits it itself, even while the surface is mapped, so a visible overlay
/// follows a size-scale change this way too.
#[cfg(target_os = "linux")]
fn configure_layer_shell_surface(
    gtk_window: &gtk::ApplicationWindow,
    position: OverlayPosition,
    margin: u16,
    width: f64,
    height: f64,
    edge_slack: f64,
) {
    use gtk::prelude::{GtkWindowExt, WidgetExt};

    configure_layer_shell_position(gtk_window, position, margin, edge_slack);

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
        let window = initial_overlay_window(app_handle);
        let (width, height) = window.window_size();
        configure_layer_shell_surface(
            &gtk_window,
            current_overlay_position(),
            window.edge_margin(),
            width,
            height,
            window.edge_slack(),
        );

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

/// The monitor the overlay is placed on: the one under the cursor, else the
/// primary, as both rectangles the placement needs.
///
/// `work_area()` is the usable rectangle: `NSScreen.visibleFrame` on macOS
/// (below the menu bar, above the Dock), `MONITORINFO.rcWork` on Windows
/// (outside the taskbar and any registered app bar), `gdk_monitor_get_workarea`
/// on Linux. It shares `position()`'s global coordinate space, so no monitor
/// offset is added. Handy has placed the macOS Bottom anchor from it since
/// tauri 2.11's `work_area.position.y` fix (#14655), the bug that led PR #969
/// to abandon it for full monitor bounds; this is the same call for every edge.
///
/// Where a platform reports no work area the usable edges are the raw ones, as
/// the fallback path does today. GDK on Wayland is that case, never overriding
/// `get_workarea` and handing back the monitor geometry. It only reaches the
/// screen without layer shell; under it the compositor subtracts and this is
/// never called.
fn overlay_monitor_bounds(app_handle: &AppHandle) -> Option<MonitorBounds> {
    let monitor = get_monitor_with_cursor(app_handle)?;
    let work_area = monitor.work_area();

    Some(MonitorBounds {
        position: *monitor.position(),
        size: *monitor.size(),
        work_top: work_area.position.y,
        work_bottom: work_area.position.y + work_area.size.height as i32,
        scale: monitor.scale_factor(),
    })
}

/// Returns overlay position in logical coordinates (points on macOS).
///
/// LogicalPosition, not PhysicalPosition, because Tauri/tao converts a
/// physical position using the scale factor of the monitor the window is
/// *currently* on, which is wrong when moving cross-monitor. Windows uses
/// `place_windows_overlay` instead (no single logical space across mixed DPI).
///
/// The arithmetic is `overlay_geometry::overlay_logical_origin`; this only
/// supplies the monitor.
fn calculate_overlay_position(
    app_handle: &AppHandle,
    width: f64,
    height: f64,
    position: OverlayPosition,
    margin: u16,
    edge_slack: f64,
) -> Option<(f64, f64)> {
    let monitor = overlay_monitor_bounds(app_handle)?;
    Some(crate::overlay_geometry::overlay_logical_origin(
        &monitor, width, height, position, margin, edge_slack,
    ))
}

/// Moves and sizes the overlay in one native SetWindowPos, bypassing tao's
/// current-DPI logical conversion that mislands cross-monitor moves.
#[cfg(target_os = "windows")]
fn place_windows_overlay(
    app_handle: &AppHandle,
    overlay_window: &tauri::webview::WebviewWindow,
    logical_width: f64,
    logical_height: f64,
    position: OverlayPosition,
    margin: u16,
    edge_slack: f64,
) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};

    let monitor = overlay_monitor_bounds(app_handle)
        .ok_or_else(|| "failed to determine the monitor containing the cursor".to_string())?;
    let scale = monitor.scale;
    let (x, y, width, height) = crate::overlay_geometry::windows_overlay_bounds(
        &monitor,
        logical_width,
        logical_height,
        position,
        margin,
        edge_slack,
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
        scale
    );
    Ok(())
}

/// Creates the recording overlay window and keeps it hidden by default
#[cfg(not(target_os = "macos"))]
pub fn create_recording_overlay(app_handle: &AppHandle) {
    // The new window is not configured for anything yet.
    forget_window_state();
    // Created at the compact size for the scale in effect. Every show resizes
    // the window anyway; starting at the right size saves the first show one.
    let window = initial_overlay_window(app_handle);
    let (width, height) = window.window_size();

    // On Linux (Wayland), monitor detection often fails, but we don't need exact coordinates
    // for Layer Shell as we use anchors. On other platforms, we require a monitor.
    #[cfg(not(target_os = "linux"))]
    {
        let position = calculate_overlay_position(
            app_handle,
            width,
            height,
            current_overlay_position(),
            window.edge_margin(),
            window.edge_slack(),
        );
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
            // Installs the native blur behind the webview, hidden until a Glass
            // show reveals it. A no-op on every platform this cfg covers, called
            // anyway so the Glass module has one install site per creation path.
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
    // The new panel is not configured for anything yet.
    forget_window_state();
    // Created at the compact size for the scale in effect. Every show resizes
    // the panel anyway; starting at the right size saves the first show one.
    let window = initial_overlay_window(app_handle);
    let (width, height) = window.window_size();

    if let Some((x, y)) = calculate_overlay_position(
        app_handle,
        width,
        height,
        current_overlay_position(),
        window.edge_margin(),
        window.edge_slack(),
    ) {
        // PanelBuilder creates a Tauri window then converts it to NSPanel.
        // The window remains registered, so get_webview_window() still works.
        match PanelBuilder::<_, RecordingOverlayPanel>::new(app_handle, "recording_overlay")
            .url(WebviewUrl::App("src/overlay/index.html".into()))
            .title("Recording")
            .position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .level(PanelLevel::Status)
            .size(tauri::Size::Logical(tauri::LogicalSize { width, height }))
            // Flat's starting point, and Flat's forever: the window is larger
            // than the card and transparent around it, so a shadow would trace
            // a rectangle nobody can see. Glass turns it back on at runtime,
            // where the window is the card (`overlay_glass::window_shadow`).
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
                // Installs the native blur behind the webview, created hidden
                // until a Glass show reveals it, so a session's first frame can
                // never be a blurred rectangle with no card on it (nspanel#94).
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
    // Refreshed on every show, so the edge the theme paths resolve
    // `edge_margin` against cannot drift from the setting for longer than one
    // recording.
    let anchor = settings.overlay_position;
    update_overlay_position_cache(anchor);

    // How much room the card needs and which Material to render it in both come
    // from the resolved overlay theme. Resolving re-reads the theme file, so it
    // happens here, on the calling thread, and only the result crosses to the
    // main thread. Every show re-reads, so a write the watcher missed is still
    // in effect on the first frame.
    let resolved = crate::overlay_theme::resolve_reloading(app_handle);

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
        .run_on_main_thread(move || show_overlay_state_on_main(&handle, &state, resolved, anchor));
}

fn show_overlay_state_on_main(
    app_handle: &AppHandle,
    state: &str,
    resolved: crate::overlay_theme::ResolvedOverlayTheme,
    anchor: OverlayPosition,
) {
    let material = resolved.effective_material;

    // The shape is recorded before anything else, including under Flat, where
    // nothing reads it until a later Glass switch. That way a reposition
    // mid-session sizes from the card on screen, never a stale default.
    let shape = OverlayCardShape::initial_for(state);
    set_current_card_shape(shape);
    // A show configures the window from the same inputs a reposition does, so it
    // keeps the same record, or the session's first theme edit repeats its work.
    let window = OverlayWindowState::new(shape, &resolved, anchor);
    let (width, height) = window.window_size();
    let margin = window.edge_margin();
    let edge_slack = window.edge_slack();
    let shadow = crate::overlay_glass::window_shadow(material, window.shadow_strength());
    record_window_state(window);

    // Hides the glass view at once if this show is Flat (in case a previous
    // Glass session left it visible); under Glass it changes nothing but the
    // blur's material, so the blur cannot appear before the card paints.
    crate::overlay_glass::apply_material(
        app_handle,
        material,
        shadow,
        crate::overlay_glass::GlassAppearance::from_theme(&resolved.theme),
    );
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Invalidate any delayed hide still in flight from a previous session
        // (see `hide_recording_overlay`).
        let generation = OVERLAY_SHOW_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        OVERLAY_SESSION_ACTIVE.store(true, Ordering::SeqCst);

        #[cfg(target_os = "linux")]
        let shown_with_layer_shell = if LAYER_SHELL_ACTIVE.load(Ordering::SeqCst) {
            match overlay_window.gtk_window() {
                Ok(gtk_window) => configure_layer_shell_surface(
                    &gtk_window,
                    anchor,
                    margin,
                    width,
                    height,
                    edge_slack,
                ),
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
            let set_pos_elapsed = if let Some((x, y)) =
                calculate_overlay_position(app_handle, width, height, anchor, margin, edge_slack)
            {
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
                if let Err(error) = place_windows_overlay(
                    app_handle,
                    &overlay_window,
                    width,
                    height,
                    anchor,
                    margin,
                    edge_slack,
                ) {
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
            if let Err(error) =
                place_windows_overlay(app_handle, &overlay_window, width, height, edge_slack)
            {
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

        {
            // Emit and record are one step, under the lock the readiness signal
            // also takes. A signal between them would find an empty slot and
            // leave this show unreplayed forever.
            let mut pending = pending_show_slot();
            if let Err(error) = overlay_window.emit("show-overlay", state) {
                log::warn!("Failed to hand the '{state}' overlay state to the webview: {error}");
            }
            // The emit reaches the overlay page only if it is already listening,
            // so remember a show it was too young to hear. Once the page has
            // ever been ready this stores nothing.
            let webview_ready = OVERLAY_WEBVIEW_READY.load(Ordering::SeqCst);
            *pending = pending_show_for(webview_ready, state, generation);
        }

        // The webview's first card-shape report for this session reveals the
        // glass view, not this call. The webview has only just been handed
        // `show-overlay` and is still fetching the resolved theme, so revealing
        // here would put a blurred rectangle on screen before the card painted
        // into it. This only arms the fallback for a webview that never reports
        // at all. A no-op under Flat and off macOS.
        if material == Material::Glass {
            schedule_glass_fallback_reveal(app_handle);
        }
    } else {
        log::warn!("Cannot show the '{state}' overlay: the overlay window does not exist");
    }
}

/// The event the overlay page emits once its listeners are registered.
const OVERLAY_WEBVIEW_READY_EVENT: &str = "overlay-webview-ready";

/// Whether the overlay page is listening yet.
///
/// Tauri hands an event only to webviews that already registered a listener,
/// and the overlay page registers its own after its bundle loads, well after
/// the window exists. A `show-overlay` emitted before that is dropped, which
/// made the first `--preview-overlay` after a launch map an empty window.
///
/// Latched and never cleared, because the page loads once, at startup, and
/// lives as long as the process.
static OVERLAY_WEBVIEW_READY: AtomicBool = AtomicBool::new(false);

/// A show that reached the overlay window before its page was listening.
#[derive(Debug, PartialEq, Eq)]
struct PendingShow {
    /// The state the show carried, verbatim: `"recording"`, `"streaming"`,
    /// `"transcribing"` or `"processing"`.
    state: String,
    /// The [`OVERLAY_SHOW_GENERATION`] this show belongs to.
    generation: u64,
}

/// The one show waiting on the overlay page, if any.
static PENDING_SHOW: Mutex<Option<PendingShow>> = Mutex::new(None);

/// What a show has to leave behind for the readiness signal. A show the page
/// received needs no replay and clears whatever an earlier one left, so at most
/// one show is ever pending.
fn pending_show_for(webview_ready: bool, state: &str, generation: u64) -> Option<PendingShow> {
    (!webview_ready).then(|| PendingShow {
        state: state.to_string(),
        generation,
    })
}

/// Which show the readiness signal replays: the remembered one, unless the
/// overlay has moved on since.
///
/// A hide clears the pending show outright; the generation catches the rest.
/// A show that raced the signal bumped the counter and reached the page on its
/// own, so replaying the older state would put the wrong card on screen.
fn show_to_replay(pending: Option<PendingShow>, generation: u64) -> Option<String> {
    pending
        .filter(|pending| pending.generation == generation)
        .map(|pending| pending.state)
}

/// The pending-show slot, recovered from a poisoned lock, since it holds a
/// plain record with no invariant a panic could have left broken.
fn pending_show_slot() -> std::sync::MutexGuard<'static, Option<PendingShow>> {
    PENDING_SHOW
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// Remember the show waiting on the overlay page, or forget it with `None`.
fn set_pending_show(pending: Option<PendingShow>) {
    *pending_show_slot() = pending;
}

/// Start listening for the overlay page's readiness signal. Registered before
/// the overlay window exists, so the signal cannot arrive with nothing to catch it.
pub fn listen_for_overlay_webview_ready(app_handle: &AppHandle) {
    let handle = app_handle.clone();
    app_handle.listen(OVERLAY_WEBVIEW_READY_EVENT, move |_| {
        note_overlay_webview_ready(&handle);
    });
}

/// The overlay page has registered its listeners. Latch that, and re-run the
/// show it was too young to hear, if there was one.
fn note_overlay_webview_ready(app_handle: &AppHandle) {
    // Latched and read out under one lock, so a show running concurrently
    // either records itself before this take or sees the page as ready.
    let pending = {
        let mut slot = pending_show_slot();
        OVERLAY_WEBVIEW_READY.store(true, Ordering::SeqCst);
        slot.take()
    };

    let generation = OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst);
    let Some(state) = show_to_replay(pending, generation) else {
        return;
    };

    log::debug!("Overlay page ready; re-running the '{state}' show it missed");
    // Off the event's own thread, because a show re-reads the theme file,
    // which must not happen on the main thread.
    let handle = app_handle.clone();
    std::thread::spawn(move || show_overlay_state(&handle, &state));
}

/// True from the moment a show maps the overlay window until the hide that
/// ends that session is requested. Read by the Glass fallback reveal, which
/// must not fade a blur in on a card already fading out. The show generation
/// cannot tell that apart, because a hide does not start a new session.
static OVERLAY_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// How long the show path waits for the webview's first card-shape report
/// before revealing the glass view itself. Twice the card's own fade, well
/// past the webview's first paint on any machine that renders the card.
const GLASS_FALLBACK_REVEAL_MS: u64 = CARD_FADE_MS as u64 * 2;

/// Reveal the glass view a short while after a Glass show, unless the webview
/// already had it revealed by reporting its card shape.
///
/// The reveal belongs to the first card-shape report, the only moment Rust
/// knows the card has painted. This is the safety net for a webview that never
/// reports (an older build's overlay page, or one whose script failed), which
/// would otherwise show a completely transparent window under Glass.
///
/// Guarded by [`OVERLAY_SHOW_GENERATION`], like the delayed hide, so an older
/// session's reveal cannot undo a newer one's, and by [`OVERLAY_SESSION_ACTIVE`],
/// so a session that ended inside the delay never has a blur faded in on its way
/// out. Revealing twice is harmless, because [`crate::overlay_glass::show_glass`]
/// only updates the radius once the view is fully visible.
///
/// The Material is resolved when the timer fires, never taken from the show
/// that armed it. A switch to Flat inside the delay hides the blur and gives
/// the window its slack back, so a reveal carrying the show's stale Glass would
/// put a translucent capsule around the Flat card, this exact bug. Resolving is
/// a settings read, so it happens on this thread rather than the main one.
///
/// The corner radius is re-derived here for the same reason, from the shape on
/// screen when the timer fires rather than the one the show seeded. A show can
/// only seed a card from its state, and every Live state seeds the pill; the
/// webview opens or collapses the panel afterwards, at two thirds and three
/// quarters of the pill's radius. Because a reveal writes the radius even when
/// the view is already visible, carrying the seed would round the blur for a
/// card that is no longer there, leaving the panel's own border tracing a
/// tighter arc than the glass under it.
fn schedule_glass_fallback_reveal(app_handle: &AppHandle) {
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
        let resolved = crate::overlay_theme::resolve(&app_handle);
        let material = resolved.effective_material;
        let radius =
            OverlayWindowState::new(current_card_shape(), &resolved, current_overlay_position())
                .corner_radius();
        crate::overlay_glass::show_glass(
            &app_handle,
            material,
            crate::overlay_glass::window_shadow(material, resolved.theme.shadow_strength(material)),
            radius,
        );
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

/// Updates the overlay window position and size from the current settings, for
/// callers that changed something other than the overlay theme and so have no
/// resolved scale in hand: the position and style commands.
pub fn update_overlay_position(app_handle: &AppHandle) {
    update_overlay_window(app_handle);
}

/// What the native window was last configured for, or `None` when that is
/// unknown (before the first configure, and after the window is re-created).
static LAST_WINDOW_STATE: Mutex<Option<OverlayWindowState>> = Mutex::new(None);

/// Remember what the window has just been configured for. Called by both paths
/// that size and place it, so the cache describes the window, not one caller.
fn record_window_state(state: OverlayWindowState) {
    if let Ok(mut last) = LAST_WINDOW_STATE.lock() {
        *last = Some(state);
    }
}

/// Forget what the window was configured for, because the window is new.
fn forget_window_state() {
    if let Ok(mut last) = LAST_WINDOW_STATE.lock() {
        *last = None;
    }
}

/// Reposition the overlay for a theme the caller has already resolved, unless
/// the window is already configured exactly that way. The skip is the point.
/// An accent or a padding edit changes nothing the native window is built from,
/// and the overlay theme is now delivered on every frame of a slider drag.
///
/// An `edge_margin` edit does move the window, so the margin and the anchored
/// edge are part of [`OverlayWindowState`]; without them the skip would swallow
/// every frame of that drag.
pub fn update_overlay_position_for_theme(
    app_handle: &AppHandle,
    resolved: &crate::overlay_theme::ResolvedOverlayTheme,
) {
    let next = OverlayWindowState::new(current_card_shape(), resolved, current_overlay_position());
    let unchanged = LAST_WINDOW_STATE
        .lock()
        .is_ok_and(|last| !native_update_needed(last.as_ref(), &next));
    if unchanged {
        return;
    }

    // Positioning queries monitors/cursor (GDK/Xlib on Linux) and moves the
    // window, so it must run on the main thread — see show_overlay_state.
    let handle = app_handle.clone();
    let resolved = resolved.clone();
    let _ = app_handle
        .run_on_main_thread(move || update_overlay_position_on_main(&handle, Some(resolved)));
}

fn update_overlay_window(app_handle: &AppHandle) {
    // Positioning queries monitors/cursor (GDK/Xlib on Linux) and moves the
    // window, so it must run on the main thread. See show_overlay_state.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || update_overlay_position_on_main(&handle, None));
}

fn update_overlay_position_on_main(
    app_handle: &AppHandle,
    resolved: Option<crate::overlay_theme::ResolvedOverlayTheme>,
) {
    if let Some(overlay_window) = app_handle.get_webview_window("recording_overlay") {
        // Resolved here only for callers with no theme to pass, the position and
        // style commands. Cache-only, no filesystem IO, safe on the main thread.
        let resolved = resolved.unwrap_or_else(|| crate::overlay_theme::resolve(app_handle));
        let material = resolved.effective_material;
        // Hides the glass view when this reposition lands on Flat (in case a
        // previous Glass session left it visible); under Glass it applies the
        // engine's tokens live, so a changed Glass style, or the surface the
        // liquid engine tints itself with, reaches an overlay already on screen.
        // Every platform recomputes the size from the card on screen and the
        // size scale rather than reading the window's size back from the OS, so
        // a scale change resizes a visible window too. Without that the card
        // would repaint larger inside the old window and be clipped.
        // The cached edge, not the store: this runs on every frame of an
        // `edge_margin` drag, where a full settings deserialize is a cost the
        // draft path cannot afford. It is also the edge `resolve` measured
        // `effective_edge_margin` against, so one read pairs the recorded
        // state's edge with its margin.
        let anchor = current_overlay_position();
        let window = OverlayWindowState::new(current_card_shape(), &resolved, anchor);
        let (width, height) = window.window_size();
        let margin = window.edge_margin();
        let edge_slack = window.edge_slack();
        let radius = window.corner_radius();
        let shadow = crate::overlay_glass::window_shadow(material, window.shadow_strength());

        crate::overlay_glass::apply_material(
            app_handle,
            material,
            shadow,
            crate::overlay_glass::GlassAppearance::from_theme(&resolved.theme),
        );

        // Recorded before the placement, so the one platform branch that returns
        // early (layer shell) is covered too. A failed placement is only logged
        // today; the next theme change carries a different state and tries again.
        record_window_state(window);

        #[cfg(target_os = "linux")]
        if LAYER_SHELL_ACTIVE.load(Ordering::SeqCst) {
            match overlay_window.gtk_window() {
                // Layer surfaces size themselves from GTK's size request, so the
                // full configure (size request + anchors) applies a new size
                // and forces the repaint gtk-layer-shell will not commit for a
                // margin change alone.
                Ok(gtk_window) => configure_layer_shell_surface(
                    &gtk_window,
                    anchor,
                    margin,
                    width,
                    height,
                    edge_slack,
                ),
                Err(error) => log::error!("Failed to access GTK overlay window: {error}"),
            }
            return;
        }

        #[cfg(target_os = "windows")]
        if let Err(error) = place_windows_overlay(
            app_handle,
            &overlay_window,
            width,
            height,
            anchor,
            margin,
            edge_slack,
        ) {
            log::error!("Failed to update recording overlay position: {error}");
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ =
                overlay_window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
            if let Some((x, y)) =
                calculate_overlay_position(app_handle, width, height, anchor, margin, edge_slack)
            {
                let _ = overlay_window
                    .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
            }
        }

        // The window now matches the current card shape and size scale, so bring
        // the glass view's corner radius in line, but only while the card is on
        // screen. Revealing the blur on a hidden window would leave it at full
        // alpha for the next show to flash before the card paints, and a
        // reposition is common while the overlay is down (every theme, style and
        // position change makes one). The next session's first card-shape report
        // recomputes the radius anyway. Reveal or hide is `material`'s call, not
        // this function's. Under Flat it takes the blur off screen a second time,
        // the point of routing every reveal through one door. `is_visible` is an
        // AppKit read, and this function already runs on the main thread.
        if overlay_window.is_visible().unwrap_or(false) {
            crate::overlay_glass::show_glass(app_handle, material, shadow, radius);
        }
    }
}

/// Record a card-shape report from the overlay webview and, under Glass, move
/// and reveal the native blur to match.
///
/// This is also where a Glass session's blur is first revealed. The report on
/// the first frame of a session repeats the shape the show path seeded, so it
/// takes the reveal branch below, the earliest moment Rust knows the card has
/// actually painted.
///
/// Coalescing goes by shape identity rather than by time. A report repeating
/// the shape already on screen only refreshes the radius and the reveal, and an
/// in-flight animation superseded by a new shape is left to AppKit's own
/// `animator()` to retarget rather than cancelled by hand. Under Flat, and off
/// macOS where the effective Material is never Glass, this only updates the
/// stored shape, which `update_overlay_position_on_main` reads back on the next
/// reposition, and makes sure the blur is off screen.
///
/// The theme is resolved inside the main-thread hop, not before. The Material
/// decides both the window size and whether the blur may be lit, and a switch
/// landing between the two would size the window for one Material and light the
/// blur for the other. Resolving is cache-only, so it is safe there.
pub fn set_card_shape(app_handle: &AppHandle, shape: OverlayCardShape, duration_ms: u32) {
    let previous = set_current_card_shape(shape);

    // Whether the window is mapped decides between animating the frame and
    // snapping it. `is_visible()` is an AppKit read on macOS, so it is taken in
    // the same main-thread hop that acts on it, not the handler's own thread.
    let handle = app_handle.clone();
    let _ = app_handle.run_on_main_thread(move || {
        let resolved = crate::overlay_theme::resolve(&handle);
        let material = resolved.effective_material;
        let window = OverlayWindowState::new(shape, &resolved, current_overlay_position());
        let size = window.window_size();
        let radius = window.corner_radius();
        let shadow = crate::overlay_glass::window_shadow(material, window.shadow_strength());

        let visible = handle
            .get_webview_window("recording_overlay")
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false);
        if material == Material::Glass && previous != shape && visible {
            crate::overlay_glass::morph_frame(&handle, material, shadow, size, radius, duration_ms);
        } else {
            // Reveals under Glass; under Flat this is what takes a blur left
            // over from a Glass session off screen.
            crate::overlay_glass::show_glass(&handle, material, shadow, radius);
        }
    });
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
        // A show still waiting on the overlay page belongs to the session that
        // is ending here, so it must never be replayed.
        set_pending_show(None);
        // Emit event to trigger fade-out animation
        let _ = overlay_window.emit("hide-overlay", ());
        // Under Glass the blur is a native layer that takes no part in the card's
        // exit, so it must be driven out on the same timing or it sits on screen
        // with nothing in it. A no-op under Flat and when never revealed.
        crate::overlay_glass::fade_out(app_handle, current_card_shape().glass_fade_out_ms());
        // Hide the window after a short delay to allow animation to complete,
        // unless a newer session has shown the overlay again by then.
        let window_clone = overlay_window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            if OVERLAY_SHOW_GENERATION.load(Ordering::SeqCst) != scheduled_at {
                log::debug!("Skipping stale overlay hide: a newer session is showing the overlay");
                return;
            }
            // Nothing is on screen any more, so the window has no shape to keep.
            // Cleared here rather than when the hide is scheduled, so a show
            // landing inside the 300 ms delay keeps the shape it just set. Under
            // zero slack a stale shape from a finished Live session must not be
            // what the next reposition sizes the window from.
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

    /// The race the first `--preview-overlay` after a launch used to lose. The
    /// show reaches the window before the page is listening, so it has to be
    /// remembered rather than emitted and forgotten.
    #[test]
    fn a_show_the_overlay_page_cannot_hear_yet_is_remembered() {
        assert_eq!(
            pending_show_for(false, "recording", 7),
            Some(PendingShow {
                state: "recording".to_string(),
                generation: 7,
            })
        );
        assert_eq!(pending_show_for(true, "recording", 7), None);
    }

    #[test]
    fn readiness_re_runs_the_show_that_was_missed() {
        let pending = pending_show_for(false, "streaming", 3);
        assert_eq!(show_to_replay(pending, 3), Some("streaming".to_string()));
    }

    /// Nothing pending, and a pending show the overlay has moved past, both
    /// leave the screen alone.
    #[test]
    fn readiness_never_re_runs_a_stale_show() {
        assert_eq!(show_to_replay(None, 3), None);
        let pending = pending_show_for(false, "streaming", 3);
        assert_eq!(show_to_replay(pending, 4), None);
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
}
