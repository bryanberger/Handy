//! Tauri commands for the overlay theme.
//!
//! These deliberately live here rather than in `shortcut/mod.rs`, where the
//! other `change_*_setting` commands are: that file is upstream's busiest
//! command file and this fork keeps new code in new files so it rebases
//! cleanly. Anyone tidying this up should read that note first.
//!
//! Whether a command is `async` is load-bearing, not stylistic: Tauri runs a
//! non-`async` command inline on the IPC (main) thread and spawns an `async fn`
//! on the runtime. So a command that touches the filesystem is `async`, and one
//! that only reads a cache stays synchronous — which is what lets the overlay
//! pull its theme on the show path without paying for IO.

use crate::overlay_theme::{self, OverlayTheme, ResolvedOverlayTheme};
use crate::settings::{get_settings, write_settings};
use tauri::{AppHandle, Emitter};

/// Persist the whole overlay theme.
///
/// The frontend always sends the complete nine-token object: setting one token,
/// clearing one token (reset to inherit) and resetting the whole theme are all
/// this one call with a different object. That keeps the settings store's
/// optimistic write and its rollback — both keyed on a single `AppSettings`
/// field — working unchanged.
///
/// Values are clamped before they are stored, so nothing out of range ever
/// reaches the store, the native geometry or the frontend.
#[tauri::command]
#[specta::specta]
pub fn change_overlay_theme_setting(app: AppHandle, theme: OverlayTheme) -> Result<(), String> {
    let mut settings = get_settings(&app);
    settings.overlay_theme = theme.normalized();
    write_settings(&app, settings);

    // Clamping happens behind the frontend's back, so what was stored can differ
    // from what the settings store optimistically wrote. `settings-changed`
    // makes it re-read `AppSettings`, which is the only thing that pulls a
    // control back to the value that was actually kept. The cost is a full
    // settings re-fetch per commit, so the tab's controls must stay debounced —
    // an undebounced slider would fetch on every frame and could snap mid-drag.
    let _ = app.emit(
        "settings-changed",
        serde_json::json!({ "setting": "overlay_theme" }),
    );

    let resolved = overlay_theme::resolve(&app);
    overlay_theme::deliver(&app, &resolved);

    Ok(())
}

/// The current resolved overlay theme, from the theme-file cache.
///
/// A pure pull: it does no filesystem IO, emits nothing and touches no native
/// window, which is why the overlay can call it inside the settings read it
/// already awaits when it is about to become visible.
#[tauri::command]
#[specta::specta]
pub fn get_resolved_overlay_theme(app: AppHandle) -> Result<ResolvedOverlayTheme, String> {
    Ok(overlay_theme::resolve(&app))
}

/// Resolve the overlay theme, deliver it, and return it.
///
/// **There is no theme file yet**, so today this only re-resolves: it is the
/// seam the Appearance tab already calls on mount and from its Reload button.
/// It is `async` from the start because the theme-file slice adds a filesystem
/// read here, and `async` is what keeps that read off the main thread — Tauri
/// runs a sync command inline on the IPC thread and spawns an `async fn` on the
/// runtime, so changing this later would change where the read lands.
#[tauri::command]
#[specta::specta]
pub async fn reload_overlay_theme_file(app: AppHandle) -> Result<ResolvedOverlayTheme, String> {
    let resolved = overlay_theme::resolve_reloading(&app);
    overlay_theme::deliver(&app, &resolved);

    Ok(resolved)
}
