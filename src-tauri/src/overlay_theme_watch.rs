//! The theme file's watcher: a hand edit reaches the overlay without Reload.
//!
//! `overlay_theme.json` is the overlay theme, so editing it in a text editor,
//! or a theming tool rewriting it, has to look like editing the Appearance
//! tab. One background thread watches the file's *directory*, not the file,
//! which makes creation, deletion and the temp-file-plus-rename every careful
//! writer (Handy included) performs all visible.
//!
//! Three things keep it quiet and honest:
//!
//!  - **Debounced.** `notify`'s debouncer collapses the burst an editor's save
//!    produces into one batch, so one save is one re-read.
//!  - **Idempotent.** The re-read goes through
//!    [`crate::overlay_theme::deliver_if_changed`], so Handy seeing its own
//!    write come back repaints nothing.
//!  - **Optional.** Every failure to start is a `watching: false` in the
//!    resolved theme, which puts the Appearance tab's Reload button back.
//!    Launch and every overlay show still re-read the file, so a missed event
//!    self-heals at the next dictation either way.

use crate::overlay_theme;
use crate::overlay_theme_file;
use log::{debug, info, warn};
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::Duration;

/// How long the watcher waits for a directory to go quiet before re-reading.
///
/// Long enough to collapse the several events one editor save emits, short
/// enough that a theme switch feels immediate.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// How often the loop wakes with nothing to do, to re-resolve the theme file:
/// the folder it wants may have been created, and the file in effect may have
/// moved to a location outranking the one being watched.
const RETARGET_TICK: Duration = Duration::from_secs(5);

/// Whether a watcher is running, so the resolved theme can say so and the
/// Appearance tab can drop its Reload button.
static WATCHING: AtomicBool = AtomicBool::new(false);

/// Whether the theme file's watcher is delivering changes.
pub fn is_watching() -> bool {
    WATCHING.load(Ordering::Relaxed)
}

/// Start watching the theme file, on a thread of its own.
///
/// Called once at setup, after the first read. Returns at once; whether it
/// came up is reported by [`is_watching`], never a panic or a blocked launch.
pub fn start(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("overlay-theme-watch".to_string())
        .spawn(move || run(app))
        .map(|_| ())
        .unwrap_or_else(|error| {
            warn!("Could not start the overlay theme watcher: {error}; Reload stays the way to apply a hand edit");
        });
}

/// The watcher's whole life: open a session, then re-read on every batch that
/// touches the theme file until the watch dies.
fn run(app: tauri::AppHandle) {
    let Some(target) = overlay_theme_file::effective_target(&app) else {
        debug!("No theme file path to watch");
        return;
    };

    let mut session = match Session::open(&target) {
        Ok(session) => session,
        Err(problem) => {
            warn!("Could not watch {}: {problem}", target.display());
            return;
        }
    };

    info!("Watching {} for overlay theme changes", target.display());
    WATCHING.store(true, Ordering::Relaxed);

    while let Some(batch) = session.next_batch(RETARGET_TICK) {
        // Asked on every wake: neither the file in effect nor the folder it
        // sits in is settled for the app's life. A file appearing at a
        // higher-priority location moves the target, and the watched folder
        // can be deleted out from under the watch.
        let moved = session.follow(overlay_theme_file::effective_target(&app));
        if batch == Batch::Touched || moved {
            // Off the main thread already, which is what `resolve_reloading`
            // requires: it re-reads the file.
            let resolved = overlay_theme::resolve_reloading(&app);
            if overlay_theme::deliver_if_changed(&app, &resolved) {
                debug!("Applied a change to {}", session.target.display());
            }
        }
    }

    WATCHING.store(false, Ordering::Relaxed);
    warn!("The overlay theme watcher stopped; Reload is the way to apply a hand edit now");
    // `watching` hides the Appearance tab's Reload button, so a tab already
    // open has to hear that it is needed again. Unconditional: only that flag
    // changed, and `deliver_if_changed` is about the theme.
    let resolved = overlay_theme::resolve_reloading(&app);
    overlay_theme::deliver(&app, &resolved);
}

/// What one wake-up of the watch loop found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Batch {
    /// Something happened to the theme file. Re-read and deliver.
    Touched,
    /// Nothing that concerns the theme file. Go back to waiting.
    Quiet,
}

/// A live watch on the theme file's directory.
///
/// The directory, because a file that does not exist yet cannot be watched and
/// the atomic rename every careful writer performs replaces the inode a file
/// watch would hold. Non-recursive: one directory, and only the events naming
/// `overlay_theme.json` count.
struct Session {
    /// Held only to be dropped: dropping it stops the watch, which is how
    /// [`Session::follow`] moves it.
    _debouncer: Debouncer<notify::RecommendedWatcher>,
    events: Receiver<DebounceEventResult>,
    /// The theme file itself, for the log line and the name test.
    target: PathBuf,
    /// The directory being watched right now: the theme file's own parent, or
    /// the nearest ancestor that exists while that parent does not.
    watched: PathBuf,
    /// Set when an event names the watched directory itself, which is how a
    /// platform reports it being deleted or replaced. The watch is then
    /// holding something nobody writes to, so it is re-opened.
    stale: bool,
}

impl Session {
    /// Watch the directory holding `target`.
    ///
    /// Canonicalized, so a symlinked config directory is watched where the
    /// writes land, not at the link, which reports nothing. A parent that does
    /// not exist yet falls back to the nearest ancestor that does, and
    /// [`Session::follow`] moves onto the real parent as soon as it appears.
    fn open(target: &Path) -> Result<Session, String> {
        let watched = watch_location(target)
            .ok_or_else(|| format!("no existing folder above {}", target.display()))?;

        let (sender, events) = channel();
        let mut debouncer = new_debouncer(DEBOUNCE, move |result: DebounceEventResult| {
            // A closed receiver means the session was dropped; there is
            // nothing to report it to.
            let _ = sender.send(result);
        })
        .map_err(|error| format!("cannot create a file watcher: {error}"))?;

        debouncer
            .watcher()
            .watch(&watched, RecursiveMode::NonRecursive)
            .map_err(|error| format!("cannot watch {}: {error}", watched.display()))?;

        Ok(Session {
            _debouncer: debouncer,
            events,
            target: target.to_path_buf(),
            watched,
            stale: false,
        })
    }

    /// Wait for one debounced batch, up to `timeout`.
    ///
    /// A timeout is not a failure but the loop's chance to re-resolve the
    /// theme file and move the watch, which is why the caller runs
    /// [`Session::follow`] on every wake, quiet or not.
    ///
    /// `None` says the debouncer stopped sending, which ends the watch: its
    /// own thread must die, since this session owns the sender for life.
    fn next_batch(&mut self, timeout: Duration) -> Option<Batch> {
        match self.events.recv_timeout(timeout) {
            Ok(Ok(events)) => {
                self.stale |= events.iter().any(|event| event.path == self.watched);
                let touched = events.iter().any(|event| self.concerns_target(&event.path));
                Some(if touched {
                    Batch::Touched
                } else {
                    Batch::Quiet
                })
            }
            // The watcher itself failed on this batch. It is still alive, so
            // re-read rather than assume nothing happened.
            Ok(Err(error)) => {
                debug!("The overlay theme watcher reported {error}");
                Some(Batch::Touched)
            }
            Err(RecvTimeoutError::Timeout) => Some(Batch::Quiet),
            Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Whether an event path is the theme file.
    ///
    /// By file name, not full path. The watch is on one directory, so every
    /// event is in it, and the path a platform reports can differ from the one
    /// Handy asked for (macOS resolves `/tmp` to `/private/tmp`). An editor's
    /// `overlay_theme.json~` or `.swp` has a different name and is ignored.
    fn concerns_target(&self, path: &Path) -> bool {
        path.file_name() == self.target.file_name()
    }

    /// Point the watch at the theme file in effect now, and re-open one that
    /// has gone blind. Returns whether anything moved.
    ///
    /// Three things make a watch wrong, and none announces itself: a watch
    /// standing in at an ancestor when `~/.config/handy/` is finally created,
    /// a file appearing at a higher-priority location so the theme file is a
    /// different file, and the watched directory being deleted or replaced. A
    /// move counts as a touch: the file may have arrived with the directory.
    ///
    /// `target` is the freshly resolved path, or `None` when there is nowhere
    /// to write one; the watch then stays where it is rather than going deaf.
    fn follow(&mut self, target: Option<PathBuf>) -> bool {
        let target = target.unwrap_or_else(|| self.target.clone());
        let location = watch_location(&target);
        let settled = !self.stale
            && target == self.target
            && location.as_deref() == Some(self.watched.as_path())
            && self.watched.is_dir();
        if settled {
            return false;
        }

        // Re-opened rather than re-watched in place, because the watch may
        // have to leave and re-enter the same path, where `unwatch` after
        // `watch` would undo the new one. Dropping the old session stops the
        // old watch; a failure keeps it, and the next wake tries again.
        match Session::open(&target) {
            Ok(session) => {
                debug!(
                    "Moved the overlay theme watch from {} to {}",
                    self.watched.display(),
                    session.watched.display()
                );
                *self = session;
                true
            }
            Err(problem) => {
                debug!("Cannot move the overlay theme watch: {problem}");
                false
            }
        }
    }
}

/// The directory a watch on `target` belongs on right now: the file's own
/// folder, or the nearest ancestor that exists while that folder does not, so
/// a watch can start before `~/.config/handy/` does.
///
/// Canonical, because notify's non-recursive filter compares an event's parent
/// against the path it was handed, and on macOS a temp or home path is usually
/// a symlink away from what FSEvents reports.
fn watch_location(target: &Path) -> Option<PathBuf> {
    let parent =
        overlay_theme_file::containing_directory(target).unwrap_or_else(|| PathBuf::from("."));
    overlay_theme_file::nearest_existing(&parent, |ancestor| ancestor.is_dir())
        .map(|directory| std::fs::canonicalize(&directory).unwrap_or(directory))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay_theme_file::THEME_FILE_NAME;

    /// Long enough that a slow machine is not called a failure, short enough
    /// that a broken watch does not hang the suite.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// How long "and then nothing else happened" is given to be wrong.
    const SILENCE: Duration = Duration::from_secs(1);

    /// One turn of the watch loop, as `run` performs it: a batch, then the
    /// re-resolve every wake does. Quiet wake-ups are ignored until the
    /// patience runs out, and a watch that moved counts as a touch.
    fn turn(session: &mut Session) -> Batch {
        let batch = session
            .next_batch(Duration::from_millis(250))
            .expect("the watch is alive");
        let moved = session.follow(None);
        if moved {
            Batch::Touched
        } else {
            batch
        }
    }

    /// Wait for a batch, ignoring the quiet wake-ups a retarget tick makes.
    fn wait_for_touch(session: &mut Session) -> Batch {
        let deadline = std::time::Instant::now() + PATIENCE;
        loop {
            match turn(session) {
                Batch::Quiet if std::time::Instant::now() < deadline => continue,
                batch => return batch,
            }
        }
    }

    /// Whether the watch stays quiet for [`SILENCE`].
    fn stays_quiet(session: &mut Session) -> bool {
        let deadline = std::time::Instant::now() + SILENCE;
        while std::time::Instant::now() < deadline {
            if turn(session) == Batch::Touched {
                return false;
            }
        }
        true
    }

    /// The watcher's reason to exist: somebody else writes the theme file and
    /// Handy hears about it. Creation, rewriting and deletion all count,
    /// because all three change what the overlay looks like.
    #[test]
    fn an_external_write_to_the_theme_file_is_seen() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let target = directory.path().join(THEME_FILE_NAME);
        let mut session = Session::open(&target).expect("a watch on an existing directory");

        std::fs::write(&target, "{\n  \"version\": 1\n}\n").expect("the file is created");
        assert_eq!(wait_for_touch(&mut session), Batch::Touched, "created");

        std::fs::write(&target, "{\n  \"radius\": 12\n}\n").expect("the file is rewritten");
        assert_eq!(wait_for_touch(&mut session), Batch::Touched, "rewritten");

        std::fs::remove_file(&target).expect("the file is deleted");
        assert_eq!(wait_for_touch(&mut session), Batch::Touched, "deleted");
    }

    /// A directory holds more than the theme file. An editor's backup, a swap
    /// file and Handy's own temp file must not each cost a re-read.
    #[test]
    fn a_neighbouring_file_is_not_the_theme_file() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let target = directory.path().join(THEME_FILE_NAME);
        let session = Session::open(&target).expect("a watch on an existing directory");

        assert!(session.concerns_target(&directory.path().join(THEME_FILE_NAME)));
        // The path a platform reports need not be the one Handy asked for.
        assert!(session.concerns_target(Path::new("/private/somewhere/overlay_theme.json")));

        assert!(!session.concerns_target(&directory.path().join("overlay_theme.json~")));
        assert!(!session.concerns_target(&directory.path().join(".overlay_theme.json.swp")));
        assert!(!session.concerns_target(&directory.path().join(".overlay_theme.json.42.7.tmp")));
        assert!(!session.concerns_target(directory.path()));
    }

    /// `~/.config/handy/` usually does not exist yet, and a watch that refused
    /// to start there would leave the tab on Reload forever. The stand-in is
    /// an ancestor, and the watch moves onto the real one when it appears.
    #[test]
    fn a_missing_directory_is_watched_from_the_nearest_ancestor() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let nested = directory.path().join("handy");
        let target = nested.join(THEME_FILE_NAME);
        let canonical =
            |path: &Path| std::fs::canonicalize(path).expect("a temp path canonicalizes");

        let mut session = Session::open(&target).expect("a watch above the missing directory");
        assert_eq!(session.watched, canonical(directory.path()));

        std::fs::create_dir(&nested).expect("the directory is created");
        assert_eq!(wait_for_touch(&mut session), Batch::Touched);
        assert_eq!(
            session.watched,
            canonical(&nested),
            "the watch moved onto it"
        );

        std::fs::write(&target, "{\n  \"version\": 1\n}\n").expect("the file is created");
        assert_eq!(wait_for_touch(&mut session), Batch::Touched);
    }

    /// The watch is on a directory, so it sees Handy's own commits as well as
    /// everyone else's. One write has to arrive as one batch: the hidden temp
    /// file it goes through is not the theme file, so only the rename counts,
    /// and nothing follows it. (`overlay_theme::deliver_if_changed` is what
    /// makes that one batch cost no repaint, tested with the resolver.)
    #[test]
    fn handys_own_write_arrives_once_and_is_then_quiet() {
        // The write asks whether an absent path is Handy's, which reads
        // `HANDY_OVERLAY_THEME_FILE`.
        let _lock = crate::overlay_theme_file::env_var_test_lock();
        let directory = tempfile::tempdir().expect("a temp dir");
        let target = directory.path().join(THEME_FILE_NAME);
        let mut session = Session::open(&target).expect("a watch on an existing directory");

        let theme = crate::overlay_theme::OverlayTheme {
            radius: Some(12),
            ..Default::default()
        };
        crate::overlay_theme_write::save_for_test(&target, &theme).expect("the write lands");

        assert_eq!(wait_for_touch(&mut session), Batch::Touched);
        assert!(
            stays_quiet(&mut session),
            "the temp file the write went through is not the theme file"
        );
    }

    /// A theme file created at a higher-priority location is a different file
    /// in effect, and the watch has to leave the old one for it.
    #[test]
    fn the_watch_follows_the_theme_file_to_another_folder() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let first = directory.path().join("app-data");
        let second = directory.path().join("config");
        std::fs::create_dir_all(&first).expect("the temp dir is writable");
        std::fs::create_dir_all(&second).expect("the temp dir is writable");

        let mut session = Session::open(&first.join(THEME_FILE_NAME)).expect("a watch");
        let moved = session.follow(Some(second.join(THEME_FILE_NAME)));
        assert!(moved, "the target moved, so the watch moved with it");
        assert_eq!(
            session.watched,
            std::fs::canonicalize(&second).expect("a temp path canonicalizes")
        );

        std::fs::write(second.join(THEME_FILE_NAME), "{\n  \"version\": 1\n}\n")
            .expect("the file is created");
        assert_eq!(wait_for_touch(&mut session), Batch::Touched);
    }
}
