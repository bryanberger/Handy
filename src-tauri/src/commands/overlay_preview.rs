//! Tauri commands for preview mode.
//!
//! Adapters only. Preview mode itself lives in [`crate::overlay_preview`],
//! which owns the guard, the driver thread, the cycles and every decision they
//! turn on. It sits there because the cancel funnel, the window-close handler,
//! the exit handler and the `--preview-overlay` flag all reach it without
//! going through a command.
//!
//! Whether a command is `async` is load-bearing. Tauri runs a non-`async`
//! command inline on the IPC (main) thread and spawns an `async fn` on the
//! runtime. So the start, which waits briefly for a previous driver to let go,
//! is `async` and hands the wait to a blocking thread; the two that only write
//! an atomic stay synchronous.

use crate::overlay_preview::{self, PreviewState};
use tauri::AppHandle;

/// Show the real overlay and keep it there, cycling or pinned, until something
/// stops it.
///
/// `sample_text` is the Live panel's transcript, passed in already translated
/// so i18n stays entirely on the frontend. `None` falls back to a built-in
/// English sentence. Returns as soon as the overlay is up. The tab keeps
/// editing tokens while it runs, and every change repaints the overlay live.
#[tauri::command]
#[specta::specta]
pub async fn start_overlay_preview(
    app: AppHandle,
    state: PreviewState,
    sample_text: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || overlay_preview::start(&app, state, sample_text))
        .await
        .map_err(|error| format!("Overlay preview could not start: {error}"))?
}

/// Set which state the preview shows, without restarting the driver.
///
/// Safe to call while nothing is running. The pin sticks, and the next start
/// uses it.
#[tauri::command]
#[specta::specta]
pub fn set_overlay_preview_state(state: PreviewState) {
    overlay_preview::pin_state(state);
}

/// Stop the running preview and hide the overlay. A no-op when none is running.
#[tauri::command]
#[specta::specta]
pub fn stop_overlay_preview() {
    overlay_preview::stop_preview();
}
