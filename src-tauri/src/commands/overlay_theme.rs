//! Tauri commands for the overlay theme.
//!
//! These deliberately live here rather than in `shortcut/mod.rs`, where the
//! other `change_*_setting` commands are. That file is the busiest command
//! file in the tree, and new code kept in new files does not collide when
//! this branch is rebased on upstream. Folding them in would trade that away.
//!
//! Whether a command is `async` is load-bearing. Tauri runs a non-`async`
//! command inline on the IPC (main) thread and spawns an `async fn` on the
//! runtime. So a command that touches the filesystem is `async`, and one that
//! only reads a cache stays synchronous, which is what lets the overlay pull
//! its theme on the show path without paying for IO.

use crate::overlay_theme::{self, OverlayTheme, ResolvedOverlayTheme};
use crate::settings::{get_settings, write_settings};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// Whether the overlay is currently painted with a value nobody has stored.
///
/// Set by [`preview_overlay_theme_draft`], cleared by the commit below. It
/// exists because the two paths can disagree about what is on screen. A draft
/// paints the overlay without touching the store, so what the store holds is
/// no longer what the user is looking at. A commit that happens to store what
/// was already stored would then have nothing to do, and would leave the
/// abandoned draft on screen for good. The clearest case is a reset in the
/// middle of a drag, where the debounce is cancelled, the token was already
/// inherit and the commit is a no-op. That used to strand the overlay at
/// whatever the finger last touched.
static OVERLAY_DRAFTED: AtomicBool = AtomicBool::new(false);

/// Remember that the overlay is showing an uncommitted value.
fn mark_overlay_drafted() {
    OVERLAY_DRAFTED.store(true, Ordering::SeqCst);
}

/// Read and clear the mark. A commit repaints the overlay from the store
/// either way, so no draft is outstanding once this has been asked.
fn take_overlay_drafted() -> bool {
    OVERLAY_DRAFTED.swap(false, Ordering::SeqCst)
}

/// What a commit owes the overlay and the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitEffect {
    /// The store already holds this theme and the overlay is already painting
    /// it: no write, no broadcast, no repaint.
    Nothing,
    /// The store already holds this theme, but the overlay is showing a draft
    /// on top of it. Nothing to persist, but still a frame to send, because
    /// the screen has to end on the value that is actually stored.
    RepaintOnly,
    /// The ordinary commit: persist, tell everyone, repaint.
    PersistAndRepaint,
}

/// The commit rule, as a pure function of the two facts it turns on.
///
/// The rule in one line: a commit always leaves the overlay showing the
/// committed theme. "Nothing changed" is only permission to skip the work when
/// nothing changed on screen either.
fn commit_effect(theme_changed: bool, overlay_drafted: bool) -> CommitEffect {
    match (theme_changed, overlay_drafted) {
        (true, _) => CommitEffect::PersistAndRepaint,
        (false, true) => CommitEffect::RepaintOnly,
        (false, false) => CommitEffect::Nothing,
    }
}

/// Persist the whole overlay theme.
///
/// The frontend always sends the complete sixteen-token object. Setting one
/// token, clearing one token (reset to inherit) and resetting the whole theme
/// are all this one call with a different object. That keeps the settings
/// store's optimistic write and its rollback working unchanged, since both are
/// keyed on a single `AppSettings` field.
///
/// Values are clamped before they are stored, so nothing out of range ever
/// reaches the store, the native geometry or the frontend. This returns the
/// clamped theme rather than leaving the caller to re-read it, which lets the
/// settings store correct its own optimistic write without a round trip back
/// through `get_app_settings`.
#[tauri::command]
#[specta::specta]
pub fn change_overlay_theme_setting(
    app: AppHandle,
    theme: OverlayTheme,
) -> Result<OverlayTheme, String> {
    let normalized = theme.normalized();
    let mut settings = get_settings(&app);
    // A commit that stores what is already stored has nothing to persist, and
    // nothing to tell anyone about. It happens often. A debounced drag settles
    // on a value an earlier commit in the same drag already wrote, and a reset
    // re-sends the theme it just reset to. What it may still owe is a
    // frame; see `OVERLAY_DRAFTED`.
    let effect = commit_effect(settings.overlay_theme != normalized, take_overlay_drafted());
    if effect == CommitEffect::Nothing {
        return Ok(normalized);
    }

    if effect == CommitEffect::PersistAndRepaint {
        settings.overlay_theme = normalized.clone();
        write_settings(&app, settings);

        // Everything else that reads `AppSettings`, the tray or another tab,
        // still has to hear that the store moved. The settings store
        // deliberately does not re-read on this one, because the normalized
        // theme returned above is the same value that re-read would fetch.
        let _ = app.emit(
            "settings-changed",
            serde_json::json!({ "setting": "overlay_theme" }),
        );
    }

    // Resolved from the theme just written rather than read back out of the
    // store, because this command is the one place that already knows it.
    // Delivered on the `RepaintOnly` path too, and that path exists precisely
    // to take an abandoned draft off the screen.
    let resolved = overlay_theme::resolve_with(&app, normalized.clone());
    overlay_theme::deliver(&app, &resolved);

    Ok(normalized)
}

/// Paint a theme the user is still dragging, without persisting anything.
///
/// The Appearance tab commits on a debounce, which is right for the store and
/// far too slow for the eye, so nothing reached the overlay until the drag
/// stopped. The tab also sends the draft here, coalesced to one call per
/// animation frame, and this puts it on the overlay with no settings read, no
/// settings write, no `settings-changed`, and no native window work unless a
/// token the window is actually built from moved.
///
/// A no-op unless a preview is running and nothing is recording, as decided by
/// `overlay_preview::accepts_theme_drafts`. Outside preview mode the overlay
/// belongs to whatever is recording, and a draft has no business repainting
/// it. The same goes for a preview that has been told to stop, and for one a
/// real recording has taken, both of which own an overlay that is no longer
/// the tab's to paint.
///
/// Every draft that does get through leaves a mark, which
/// `change_overlay_theme_setting` clears. That is what guarantees the screen
/// ends on a stored value even when the commit that follows has nothing to
/// store.
#[tauri::command]
#[specta::specta]
pub fn preview_overlay_theme_draft(app: AppHandle, theme: OverlayTheme) -> Result<(), String> {
    if !crate::overlay_preview::accepts_theme_drafts(&app) {
        return Ok(());
    }

    let resolved = overlay_theme::resolve_with(&app, theme.normalized());
    overlay_theme::deliver_draft(&app, &resolved);
    mark_overlay_drafted();

    Ok(())
}

/// The current resolved overlay theme, from the theme-file cache.
///
/// A pure pull. It reads the cache the show path has just refreshed, emits
/// nothing and touches no native window, which is why the overlay can call it
/// inside the settings read it already awaits when it is about to become
/// visible. That is also what keeps a show to exactly one file read. The
/// backend re-reads, and the webview only pulls.
#[tauri::command]
#[specta::specta]
pub fn get_resolved_overlay_theme(app: AppHandle) -> Result<ResolvedOverlayTheme, String> {
    Ok(overlay_theme::resolve(&app))
}

/// Re-read the theme file, resolve, deliver, and return the result.
///
/// What the Appearance tab calls on mount and from its Reload button, and the
/// only way a user gets a hand-edited theme file onto the screen without
/// recording, because there is no file watcher.
///
/// `async` is load-bearing twice over. Tauri runs a sync command inline on the
/// IPC thread and spawns an `async fn` on the runtime, and the read itself
/// then goes to a blocking thread, so neither the main thread nor an async
/// worker ever waits on the filesystem.
#[tauri::command]
#[specta::specta]
pub async fn reload_overlay_theme_file(app: AppHandle) -> Result<ResolvedOverlayTheme, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = overlay_theme::resolve_reloading(&app);
        // Delivery is what makes this a reload rather than a query. The
        // overlay repaints, and the native window is resized for a
        // file-supplied scale.
        overlay_theme::deliver(&app, &resolved);
        resolved
    })
    .await
    .map_err(|error| format!("Failed to read the overlay theme file: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ordinary case, and the one the early return exists for: a commit
    /// that changes the store always repaints; one that changes nothing, over
    /// an overlay nobody drafted onto, does nothing at all.
    #[test]
    fn a_commit_that_changes_nothing_over_an_undrafted_overlay_does_nothing() {
        assert_eq!(commit_effect(false, false), CommitEffect::Nothing);
        assert_eq!(commit_effect(true, false), CommitEffect::PersistAndRepaint);
        assert_eq!(commit_effect(true, true), CommitEffect::PersistAndRepaint);
    }

    /// The fix: a reset in the middle of a drag cancels the debounce and
    /// commits `null` over a token that was already inherit, so there is
    /// nothing to persist, and the overlay is still showing the abandoned
    /// draft. It has to be repainted from the store anyway.
    #[test]
    fn a_commit_over_a_drafted_overlay_repaints_even_with_nothing_to_store() {
        assert_eq!(commit_effect(false, true), CommitEffect::RepaintOnly);
    }

    /// The mark is a one-shot. The commit that reads it is the commit that
    /// repaints, so a second commit right behind it has nothing left to undo.
    #[test]
    fn taking_the_draft_mark_clears_it() {
        // Left exactly as found, because this static is process-wide.
        let previous = OVERLAY_DRAFTED.load(Ordering::SeqCst);

        OVERLAY_DRAFTED.store(false, Ordering::SeqCst);
        assert!(!take_overlay_drafted(), "no draft, nothing to correct");

        mark_overlay_drafted();
        assert!(take_overlay_drafted(), "the draft is outstanding");
        assert!(!take_overlay_drafted(), "and only outstanding once");

        OVERLAY_DRAFTED.store(previous, Ordering::SeqCst);
    }
}
