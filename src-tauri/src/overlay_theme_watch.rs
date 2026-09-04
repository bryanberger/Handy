//! The theme file's watcher: a hand edit reaches the overlay without Reload.
//!
//! `overlay_theme.json` is the overlay theme, so editing it in a text editor,
//! or a theming tool rewriting it, has to look like editing the Appearance
//! tab. One background thread watches the file's *directory*, not the file,
//! which is what makes creation, deletion and the temp-file-plus-rename that
//! every careful writer (Handy included) performs all visible.
//!
//! Three things keep it quiet and honest:
//!
//!  - **Debounced.** `notify`'s debouncer collapses the burst an editor's save
//!    produces into one batch, so one save is one re-read.
//!  - **Idempotent.** The re-read is delivered through
//!    [`crate::overlay_theme::deliver_if_changed`], so Handy seeing its own
//!    write come back repaints nothing.
//!  - **Optional.** Every failure to start is a `watching: false` in the
//!    resolved theme, which is what puts the Appearance tab's Reload button
//!    back. Launch and every overlay show still re-read the file, so a missed
//!    event self-heals at the next dictation either way.

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

/// How often the loop wakes with nothing to do, to notice that the directory
/// it wants has finally been created.
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
/// Called once at setup, after the first read. Returns immediately; whether
/// the watch actually came up is reported through [`is_watching`], never a
/// panic and never a blocked launch.
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

    loop {
        match session.next_batch(RETARGET_TICK) {
            Batch::Touched => {
                // Off the main thread already, which is what `resolve_reloading`
                // requires: it re-reads the file.
                let resolved = overlay_theme::resolve_reloading(&app);
                if overlay_theme::deliver_if_changed(&app, &resolved) {
                    debug!("Applied a change to {}", session.target.display());
                }
            }
            Batch::Quiet => {}
            Batch::Closed => break,
        }
    }

    WATCHING.store(false, Ordering::Relaxed);
    warn!("The overlay theme watcher stopped; Reload is the way to apply a hand edit now");
}

/// What one wake-up of the watch loop found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Batch {
    /// Something happened to the theme file. Re-read and deliver.
    Touched,
    /// Nothing that concerns the theme file. Go back to waiting.
    Quiet,
    /// The watcher is gone and will send nothing more.
    Closed,
}

/// A live watch on the theme file's directory.
///
/// The directory, because a file that does not exist yet cannot be watched and
/// because the atomic rename every careful writer performs replaces the inode
/// a file watch would be holding. Non-recursive: one directory, and only the
/// events naming `overlay_theme.json` count.
pub struct Session {
    /// Kept alive: dropping the debouncer stops the watch.
    debouncer: Debouncer<notify::RecommendedWatcher>,
    events: Receiver<DebounceEventResult>,
    /// The theme file itself, for the log line and the name test.
    target: PathBuf,
    /// The directory being watched right now.
    watched: PathBuf,
    /// The directory that should be watched: the theme file's own parent. It
    /// differs from `watched` only while that parent does not exist yet.
    wanted: PathBuf,
}

impl Session {
    /// Watch the directory holding `target`.
    ///
    /// The parent is canonicalized, so a symlinked config directory is watched
    /// where the writes actually land rather than at the link, which reports
    /// nothing. A parent that does not exist yet falls back to the nearest
    /// ancestor that does, and [`Session::next_batch`] moves the watch onto
    /// the real parent as soon as it appears.
    pub fn open(target: &Path) -> Result<Session, String> {
        let wanted = watch_directory(target);
        // Canonical either way: notify's non-recursive filter compares an
        // event's parent against the path it was handed, and on macOS a temp
        // or home path is usually a symlink away from the one FSEvents reports.
        let watched = nearest_existing(&wanted)
            .map(|directory| std::fs::canonicalize(&directory).unwrap_or(directory))
            .ok_or_else(|| format!("no existing folder above {}", wanted.display()))?;

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
            debouncer,
            events,
            target: target.to_path_buf(),
            watched,
            wanted,
        })
    }

    /// Wait for one debounced batch, up to `timeout`.
    ///
    /// A timeout is not idleness to report: it is the loop's chance to notice
    /// that the directory it wanted has been created, and to move the watch
    /// onto it. A move counts as a touch, because the file may have arrived
    /// with the directory.
    pub fn next_batch(&mut self, timeout: Duration) -> Batch {
        match self.events.recv_timeout(timeout) {
            Ok(Ok(events)) => {
                let touched = events.iter().any(|event| self.concerns_target(&event.path));
                // A batch in a stand-in directory may be the wanted one being
                // created, so retarget before answering.
                let moved = self.retarget();
                if touched || moved {
                    Batch::Touched
                } else {
                    Batch::Quiet
                }
            }
            // The watcher itself failed on this batch. It is still alive, so
            // re-read rather than assume nothing happened.
            Ok(Err(error)) => {
                debug!("The overlay theme watcher reported {error}");
                Batch::Touched
            }
            Err(RecvTimeoutError::Timeout) => {
                if self.retarget() {
                    Batch::Touched
                } else {
                    Batch::Quiet
                }
            }
            Err(RecvTimeoutError::Disconnected) => Batch::Closed,
        }
    }

    /// Whether an event path is the theme file.
    ///
    /// By file name, not by full path. The watch is on one directory, so every
    /// event is in it, and the path a platform reports can differ from the one
    /// Handy asked for (macOS resolves `/tmp` to `/private/tmp`). An editor's
    /// own `overlay_theme.json~` or `.swp` has a different name and is
    /// correctly ignored.
    fn concerns_target(&self, path: &Path) -> bool {
        path.file_name() == self.target.file_name()
    }

    /// Move the watch onto the theme file's own directory once it exists.
    /// Returns whether the watch moved.
    fn retarget(&mut self) -> bool {
        if self.watched == self.wanted || !self.wanted.is_dir() {
            return false;
        }

        let wanted = watch_directory(&self.target);
        if self
            .debouncer
            .watcher()
            .watch(&wanted, RecursiveMode::NonRecursive)
            .is_err()
        {
            return false;
        }

        let previous = std::mem::replace(&mut self.watched, wanted.clone());
        let _ = self.debouncer.watcher().unwatch(&previous);
        debug!(
            "Moved the overlay theme watch from {} to {}",
            previous.display(),
            wanted.display()
        );
        self.wanted = wanted;
        true
    }
}

/// The directory a theme file's changes arrive in.
///
/// Canonicalized when it exists, so a symlinked `~/.config` is watched at its
/// real location. `canonicalize` fails on a directory that is not there yet,
/// and then the plain parent is the right answer to keep waiting for.
fn watch_directory(target: &Path) -> PathBuf {
    let parent = target
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    std::fs::canonicalize(&parent).unwrap_or(parent)
}

/// The closest directory at or above `directory` that exists, so a watch can
/// start before `~/.config/handy/` does.
fn nearest_existing(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .find(|ancestor| ancestor.is_dir())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay_theme_file::THEME_FILE_NAME;

    /// Long enough that a slow machine is not called a failure, short enough
    /// that a broken watch does not hang the suite.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// Wait for a batch, ignoring the quiet wake-ups a retarget tick makes.
    fn wait_for_touch(session: &mut Session) -> Batch {
        let deadline = std::time::Instant::now() + PATIENCE;
        loop {
            match session.next_batch(Duration::from_millis(250)) {
                Batch::Quiet if std::time::Instant::now() < deadline => continue,
                batch => return batch,
            }
        }
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
        assert!(!session.concerns_target(&directory.path().join(".overlay_theme.json.42.tmp")));
        assert!(!session.concerns_target(directory.path()));
    }

    /// `~/.config/handy/` usually does not exist yet, and a watch that refused
    /// to start there would leave the tab on Reload forever. The stand-in is
    /// an ancestor, and the watch moves onto the real directory once it is
    /// created.
    #[test]
    fn a_missing_directory_is_watched_from_the_nearest_ancestor() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let nested = directory.path().join("handy");
        let target = nested.join(THEME_FILE_NAME);

        let mut session = Session::open(&target).expect("a watch above the missing directory");
        assert_ne!(session.watched, session.wanted);
        assert_eq!(
            session.watched,
            std::fs::canonicalize(directory.path()).expect("a temp dir canonicalizes")
        );

        std::fs::create_dir(&nested).expect("the directory is created");
        assert_eq!(wait_for_touch(&mut session), Batch::Touched);
        assert_eq!(session.watched, session.wanted, "the watch moved onto it");

        std::fs::write(&target, "{\n  \"version\": 1\n}\n").expect("the file is created");
        assert_eq!(wait_for_touch(&mut session), Batch::Touched);
    }
}
