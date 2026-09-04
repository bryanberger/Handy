//! Tauri commands for the overlay theme.
//!
//! These live here rather than in `shortcut/mod.rs` with the other
//! `change_*_setting` commands, because that file is the busiest in the tree
//! and new code in new files does not collide on a rebase.
//!
//! Whether a command is `async` is load-bearing. Tauri runs a non-`async`
//! command inline on the IPC (main) thread and spawns an `async fn` on the
//! runtime, so filesystem work is `async` and a cache read stays synchronous,
//! letting the overlay pull its theme on the show path without paying for IO.

use crate::overlay_theme::{self, OverlayTheme, ResolvedOverlayTheme};
use crate::overlay_theme_file::{self, RevealTarget};
use crate::overlay_theme_write;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// Whether the overlay is currently painted with a value nobody has stored.
///
/// Set by [`preview_overlay_theme_draft`], cleared by the commit below. A
/// draft paints without touching the file, so a commit that writes what the
/// file already says has nothing to do and would strand the draft on screen.
/// The clearest case is a reset mid-drag, where the debounce is cancelled, the
/// token was already inherit and the commit is a no-op.
static OVERLAY_DRAFTED: AtomicBool = AtomicBool::new(false);

/// Remember that the overlay is showing an uncommitted value.
fn mark_overlay_drafted() {
    OVERLAY_DRAFTED.store(true, Ordering::SeqCst);
}

/// Read and clear the mark. A commit repaints the overlay from the file either
/// way, so no draft is outstanding once this has been asked.
fn take_overlay_drafted() -> bool {
    OVERLAY_DRAFTED.swap(false, Ordering::SeqCst)
}

/// What a commit owes the overlay and the theme file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitEffect {
    /// The file already holds this theme and the overlay is already painting
    /// it: no write, no broadcast, no repaint.
    Nothing,
    /// The file already holds this theme, but the overlay shows a draft on top
    /// of it. Nothing to persist, still a frame to send, because the screen has
    /// to end on the stored value.
    RepaintOnly,
    /// The ordinary commit: write the file, tell everyone, repaint.
    PersistAndRepaint,
}

/// The commit rule, as a pure function of the two facts it turns on.
///
/// A commit always leaves the overlay showing the committed theme. "Nothing
/// changed" is only permission to skip the work when nothing changed on screen
/// either.
fn commit_effect(theme_changed: bool, overlay_drafted: bool) -> CommitEffect {
    match (theme_changed, overlay_drafted) {
        (true, _) => CommitEffect::PersistAndRepaint,
        (false, true) => CommitEffect::RepaintOnly,
        (false, false) => CommitEffect::Nothing,
    }
}

/// Write the whole overlay theme to the theme file.
///
/// The theme file is the overlay theme, so this is where a committed change
/// from the Appearance tab lands. The frontend always sends the complete
/// twenty-two-token object: setting one token, clearing one (reset to inherit)
/// and resetting the whole theme are all this one call with a different
/// object.
///
/// Values are clamped before they are written, so nothing out of range reaches
/// the file, the native geometry or the frontend. The file is then read back
/// and resolved, and that resolved theme is both the answer and what goes out
/// to the two windows. Reading back is not ceremony: it makes the answer the
/// document on disk rather than the intent, and it is what lets the watcher
/// recognise Handy's own write and stay quiet.
///
/// A managed theme file (a symlink, or one Handy cannot write) is refused
/// here as well as locked in the tab, so the guard does not depend on the UI.
///
/// `async` is load-bearing: Tauri runs a sync command inline on the IPC thread
/// and spawns an `async fn` on the runtime, and the write then goes to a
/// blocking thread, so neither the main thread nor an async worker waits on
/// the filesystem.
#[tauri::command]
#[specta::specta]
pub async fn change_overlay_theme_setting(
    app: AppHandle,
    theme: OverlayTheme,
) -> Result<ResolvedOverlayTheme, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let normalized = theme.normalized();
        // Read, not the cache. "Nothing changed" has to be measured against
        // the document on disk: a hand edit or a theming tool's write between
        // two commits would otherwise look like no change at all, and the
        // user's edit would be dropped instead of written.
        let current = overlay_theme_file::read(&app);

        // A commit that writes what is already in the file has nothing to
        // persist and nothing to announce. It happens often: a debounced drag
        // settles on a value an earlier commit in the same drag wrote, and a
        // reset re-sends the theme it just reset to. What it may still owe is
        // a frame; see `OVERLAY_DRAFTED`.
        let effect = commit_effect(
            current.tokens.normalized() != normalized,
            take_overlay_drafted(),
        );
        if effect == CommitEffect::Nothing {
            return Ok(overlay_theme::resolve(&app));
        }

        if effect == CommitEffect::PersistAndRepaint {
            overlay_theme_write::save(&app, &normalized)?;
        }

        // Resolved from the document just written, not from the intent, so the
        // tab and the overlay agree with the file byte for byte. Delivered on
        // the `RepaintOnly` path too, which exists to take an abandoned draft
        // off the screen.
        let resolved = overlay_theme::resolve_reloading(&app);
        overlay_theme::deliver(&app, &resolved);
        Ok(resolved)
    })
    .await
    .map_err(|error| format!("Failed to write the overlay theme file: {error}"))?
}

/// Paint a theme the user is still dragging, without persisting anything.
///
/// The Appearance tab commits on a debounce, right for the file and far too
/// slow for the eye. It also sends the draft here, coalesced to one call per
/// animation frame, and this puts it on the overlay with nothing written, no
/// broadcast to the settings window, and no native window work unless a token
/// the window is built from moved.
///
/// A no-op unless a preview is running and nothing is recording, as decided by
/// `overlay_preview::accepts_theme_drafts`. Anywhere else the overlay belongs
/// to a recording, or to a preview told to stop, and is not the tab's to
/// paint.
///
/// Every draft that gets through leaves a mark, which
/// `change_overlay_theme_setting` clears (plain text, not an intra-doc link:
/// this doc comment is copied verbatim into `src/bindings.ts`). That
/// guarantees the screen ends on the file's own theme even when the commit
/// that follows has nothing to write, and it is why a draft is never recorded
/// as a delivery.
#[tauri::command]
#[specta::specta]
pub fn preview_overlay_theme_draft(app: AppHandle, theme: OverlayTheme) -> Result<(), String> {
    if !crate::overlay_preview::accepts_theme_drafts(&app) {
        return Ok(());
    }

    let resolved = overlay_theme::resolve_authored(&app, theme.normalized());
    overlay_theme::deliver_draft(&app, &resolved);
    mark_overlay_drafted();

    Ok(())
}

/// The current resolved overlay theme, from the theme-file cache.
///
/// A pure pull. It reads the cache the show path has just refreshed, emits
/// nothing and touches no native window, so the overlay can call it inside the
/// settings read it already awaits before becoming visible. That keeps a show
/// to exactly one file read; the backend re-reads and the webview only pulls.
#[tauri::command]
#[specta::specta]
pub fn get_resolved_overlay_theme(app: AppHandle) -> Result<ResolvedOverlayTheme, String> {
    Ok(overlay_theme::resolve(&app))
}

/// Re-read the theme file, resolve, deliver, and return the result.
///
/// What the Appearance tab calls on mount, and what its Reload button calls on
/// the machines where the watcher could not start. With the watcher running a
/// hand edit arrives on its own, so the button is not shown; this stays the
/// backstop, and the mount read stays because a tab opened after a change made
/// while it was closed must not show a stale theme.
///
/// `async` is load-bearing twice over. Tauri runs a sync command inline on the
/// IPC thread and spawns an `async fn` on the runtime, and the read then goes
/// to a blocking thread, so neither the main thread nor an async worker waits
/// on the filesystem.
#[tauri::command]
#[specta::specta]
pub async fn reload_overlay_theme_file(app: AppHandle) -> Result<ResolvedOverlayTheme, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = overlay_theme::resolve_reloading(&app);
        // Delivery is what makes this a reload rather than a query. The
        // overlay repaints and the native window resizes for a file-supplied
        // scale.
        overlay_theme::deliver(&app, &resolved);
        resolved
    })
    .await
    .map_err(|error| format!("Failed to read the overlay theme file: {error}"))
}

/// Open the folder the theme file belongs in, creating it when it is Handy's
/// own and missing.
///
/// What the Appearance tab's Open button calls when no theme file exists. The
/// path it shows then is usually `~/.config/handy/overlay_theme.json`, which
/// most users have had no reason to create, so revealing it has to make it
/// first; `revealItemInDir` needs an item, and here there is none.
///
/// Only a directory is created, never `overlay_theme.json`, and only under
/// `~/.config/handy/`. A path named by `HANDY_OVERLAY_THEME_FILE` opens at its
/// nearest existing folder instead, Handy having been told to read it, not to
/// build a tree at it. `overlay_theme_file::reveal_target` holds that choice.
///
/// `async` and then `spawn_blocking`, like `reload_overlay_theme_file`. The
/// `mkdir`, its probe and the hand-off to the file manager are all filesystem
/// work, kept off the IPC thread and the async workers.
#[tauri::command]
#[specta::specta]
pub async fn reveal_overlay_theme_location(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = match overlay_theme_file::reveal_target(&app)? {
            RevealTarget::Create(directory) => {
                overlay_theme_file::ensure_location(&directory)?;
                directory
            }
            RevealTarget::Open(directory) => directory,
        };

        app.opener()
            .open_path(directory.to_string_lossy().into_owned(), None::<String>)
            .map_err(|error| format!("Failed to open {}: {error}", directory.display()))
    })
    .await
    .map_err(|error| format!("Failed to open the theme file's folder: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ordinary case, and the one the early return exists for. A commit
    /// that changes the file always repaints; one that changes nothing, over
    /// an overlay nobody drafted onto, does nothing.
    #[test]
    fn a_commit_that_changes_nothing_over_an_undrafted_overlay_does_nothing() {
        assert_eq!(commit_effect(false, false), CommitEffect::Nothing);
        assert_eq!(commit_effect(true, false), CommitEffect::PersistAndRepaint);
        assert_eq!(commit_effect(true, true), CommitEffect::PersistAndRepaint);
    }

    /// The fix. A reset mid-drag cancels the debounce and commits `null` over
    /// a token that was already inherit, so there is nothing to write and the
    /// overlay still shows the abandoned draft. It has to be repainted from
    /// the file anyway.
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
