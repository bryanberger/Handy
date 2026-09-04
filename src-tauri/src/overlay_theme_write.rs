//! Writing `overlay_theme.json`, the file that is the overlay theme.
//!
//! Every committed change from the Appearance tab lands here, so the file and
//! the settings window agree by construction, not by a precedence rule. Three
//! promises make that safe on a file a user also hand-edits:
//!
//!  1. **Atomic.** A temp file beside the target, then a rename. No reader
//!     sees half a document; a crash mid-write leaves the previous one intact.
//!  2. **Read back through the same parser.** [`crate::overlay_theme_file`]
//!     parses the rendered text, and the rename happens only if the tokens
//!     come back unchanged. Handy never installs what it cannot load.
//!  3. **Owned only.** [`crate::overlay_theme_file::ownership_at`] decides. A
//!     symlink or a read-only file is somebody else's document; Handy reads
//!     it and leaves it alone.
//!
//! The document keeps the README's shape: `version` first, the set tokens in
//! the contract's order, then any key Handy did not recognise, two-space
//! indented with a trailing newline. Inherit is an absent key, not `null`, so
//! a per-token reset shortens the file rather than filling it with nulls.

use crate::overlay_theme::OverlayTheme;
use crate::overlay_theme_file::{
    self, CURRENT_OVERLAY_THEME_FILE_VERSION, THEME_FILE_ENV_VAR, VERSION_KEY,
};
use crate::settings::{get_settings, write_settings};
use log::{debug, info, warn};
use serde_json::{Map, Value};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::AppHandle;

/// One writer at a time.
///
/// Two commits overlap easily: a debounced drag settles while a reset is in
/// flight, and each is a read-modify-write of the same document. Serialised,
/// the later reads what the earlier wrote, not the document both started from.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Gives each temp file its own name, so an unlucky pair of writes cannot meet
/// in one file. The lock above keeps them apart in time; this keeps them apart
/// on disk, and a temp file a crash abandoned cannot be reused half-written.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Persist the overlay theme, and report where it went.
///
/// Filesystem work, so off the main thread. The caller re-reads the file
/// afterwards rather than trusting this: the file is the source of truth, and
/// the re-read is also what makes the watcher's own event a no-op.
pub fn save(app: &AppHandle, theme: &OverlayTheme) -> Result<PathBuf, String> {
    let path = overlay_theme_file::effective_target(app).ok_or_else(|| {
        format!(
            "There is nowhere to write {}",
            overlay_theme_file::THEME_FILE_NAME
        )
    })?;

    save_to(&path, theme)?;
    debug!("Wrote the overlay theme to {}", path.display());
    Ok(path)
}

/// [`save`] over a path already chosen, so the ownership guard is independent
/// of how the path was found, and a temp directory is enough to test it.
fn save_to(path: &Path, theme: &OverlayTheme) -> Result<(), String> {
    let _writing = writing();

    let ownership = overlay_theme_file::ownership_at(path);
    if !ownership.writable {
        return Err(format!(
            "{} is not Handy's to write ({:?}); the Appearance tab is read-only while it is",
            path.display(),
            ownership.reason
        ));
    }

    // Best effort: an unreadable or malformed document contributes no keys to
    // preserve. Overwriting a broken file with the values on screen is the
    // user's explicit act, which is what a committed change is.
    let existing = std::fs::read_to_string(path).ok();
    let normalized = theme.normalized();
    let text = document_text(&normalized, existing.as_deref())?;
    install(path, &text, &normalized)
}

/// The write path itself, for the watcher's tests.
///
/// They need a real commit, temp file, `sync_all` and rename, landing in a
/// watched folder, and a unit test has no `AppHandle` for [`save`].
#[cfg(test)]
pub fn save_for_test(path: &Path, theme: &OverlayTheme) -> Result<(), String> {
    save_to(path, theme)
}

/// Hold [`WRITE_LOCK`] for the rest of the caller's scope.
///
/// A poisoned lock is taken anyway: the guarded work is a read and a rename,
/// so a panicking writer leaves the document as it was or replaced, never
/// half a file for the next writer to inherit.
fn writing() -> std::sync::MutexGuard<'static, ()> {
    WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The document to write: `version`, the set tokens in the contract's order,
/// then every key Handy does not know.
///
/// Pure over the previous document's text, so the preservation rules are
/// testable without a filesystem. `existing` is whatever is on disk now, and
/// anything that is not a JSON object contributes nothing and is replaced.
fn document_text(theme: &OverlayTheme, existing: Option<&str>) -> Result<String, String> {
    let previous = existing.and_then(object_of);

    let mut rows: Vec<(String, Value)> = vec![(VERSION_KEY.to_string(), version_row(&previous))];

    // Serialized through `OverlayTheme` itself, so the spelling written is the
    // one the reader's parsers expect: canonical `#rrggbb`, `"glass"`,
    // `"hud_window"`. Nothing here formats a token by hand.
    let serialized = serde_json::to_value(theme)
        .map_err(|error| format!("Cannot serialize the overlay theme: {error}"))?;
    let serialized = serialized
        .as_object()
        .ok_or_else(|| "The overlay theme did not serialize to an object".to_string())?;

    for key in overlay_theme_file::token_keys() {
        // Inherit is an absent key. A `null` reads the same way, but grows
        // the file by a line every time a token is reset.
        match serialized.get(key) {
            Some(value) if !value.is_null() => rows.push((key.to_string(), value.clone())),
            _ => {}
        }
    }

    if let Some(previous) = &previous {
        for (key, value) in previous {
            if key == VERSION_KEY || overlay_theme_file::token_keys().any(|token| token == key) {
                continue;
            }
            // `_comment` is the documented way to annotate a document, and a
            // key from a newer schema is somebody's intent too. Handy ignores
            // them when reading and must not delete them when writing.
            rows.push((key.clone(), value.clone()));
        }
    }

    Ok(render(&rows))
}

/// The `version` to write: whatever the document declared, when that is a
/// usable version, else this build's.
///
/// A document a newer Handy wrote keeps its number. Dropping it to 1 would
/// tell the next reader that keys it does not recognise are typos.
fn version_row(previous: &Option<Map<String, Value>>) -> Value {
    previous
        .as_ref()
        .and_then(|object| object.get(VERSION_KEY))
        .filter(|value| value.as_u64().is_some_and(|version| version >= 1))
        .cloned()
        .unwrap_or_else(|| Value::from(CURRENT_OVERLAY_THEME_FILE_VERSION))
}

/// A document's top-level object, or `None` for anything else.
fn object_of(text: &str) -> Option<Map<String, Value>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

/// Rows as JSON text: two-space indent, one key per line, trailing newline.
///
/// Rendered rather than handed to `serde_json::to_string_pretty`, because
/// `serde_json::Map` is a `BTreeMap` here and sorts keys alphabetically. The
/// README's table and the tab's "Copy theme as JSON" both use the contract's
/// order, so the file follows it too.
fn render(rows: &[(String, Value)]) -> String {
    let mut document = String::from("{\n");
    for (index, (key, value)) in rows.iter().enumerate() {
        document.push_str("  ");
        // Through `Value` so a key needing an escape gets one.
        document.push_str(&Value::String(key.clone()).to_string());
        document.push_str(": ");
        document.push_str(&value.to_string());
        if index + 1 < rows.len() {
            document.push(',');
        }
        document.push('\n');
    }
    document.push_str("}\n");
    document
}

/// Write `text` beside `path`, check it reads back as `expected`, then rename
/// it into place.
///
/// The temp file is in the target's own directory, so the rename stays within
/// one filesystem and is atomic. It is hidden and named per process and per
/// write, so neither two Handys nor two commits collide, and a stray one is
/// never mistaken for the theme (the reader only opens `overlay_theme.json`).
fn install(path: &Path, text: &str, expected: &OverlayTheme) -> Result<(), String> {
    let directory = overlay_theme_file::containing_directory(path)
        .ok_or_else(|| format!("{} names no folder to write into", path.display()))?;

    // The one directory Handy creates, and only under a path it owns: the
    // ownership check above already refused anything else.
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Cannot create {}: {error}", directory.display()))?;

    let temp = temp_path(path);
    if let Err(problem) = write_temp(&temp, text).and_then(|()| verify(&temp, expected)) {
        let _ = std::fs::remove_file(&temp);
        return Err(problem);
    }

    std::fs::rename(&temp, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        format!("Cannot replace {}: {error}", path.display())
    })
}

/// Write the temp file and flush it all the way to the disk.
///
/// `sync_all` before the rename makes it atomic in a crash as well as to a
/// reader: without it the directory entry can reach the disk before the bytes
/// it points at, and the theme file comes back empty after a power cut.
fn write_temp(temp: &Path, text: &str) -> Result<(), String> {
    let mut file = std::fs::File::create(temp)
        .map_err(|error| format!("Cannot write {}: {error}", temp.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("Cannot write {}: {error}", temp.display()))?;
    file.sync_all()
        .map_err(|error| format!("Cannot flush {}: {error}", temp.display()))
}

/// Read the temp file back through the reader's own parser and check the
/// tokens survived the round trip.
///
/// Not a formality: the difference between "Handy wrote a file" and "Handy
/// wrote a file Handy can read". Before the rename, so a document that fails
/// it never becomes the theme.
fn verify(temp: &Path, expected: &OverlayTheme) -> Result<(), String> {
    let written = std::fs::read_to_string(temp)
        .map_err(|error| format!("Cannot read back {}: {error}", temp.display()))?;

    let parsed = overlay_theme_file::tokens_of(&written)
        .map_err(|problem| format!("Handy rendered a theme file it cannot read: {problem}"))?;

    if &parsed != expected {
        return Err("Handy rendered a theme file that reads back as a different theme".to_string());
    }
    Ok(())
}

/// Where the temp file goes: hidden, beside the target, named per process and
/// per write, so nothing else in that folder is mistaken for it.
fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| overlay_theme_file::THEME_FILE_NAME.to_string());
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    directory.join(format!(".{name}.{}.{sequence}.tmp", std::process::id()))
}

/// What the one-time migration should do with the store's `overlay_theme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Migration {
    /// It has already run. Nothing looks at the stored theme again.
    AlreadyDone,
    /// `HANDY_OVERLAY_THEME_FILE` names the file. Writing Handy's own
    /// location would create a document nothing reads, so this waits for the
    /// variable to go rather than marking itself done.
    Deferred,
    /// Something is already at one of Handy's locations. It wins, and the
    /// store is retired unread, so deleting that file later means the built-in
    /// look, not a theme from before the file existed.
    FileWins,
    /// The store holds nothing but inherits: no theme to move, so the store
    /// is retired.
    NothingToMove,
    /// Write the stored theme to `~/.config/handy/overlay_theme.json`.
    Write,
}

/// The migration's rule, pure over the four facts it turns on.
fn migration_step(
    already_migrated: bool,
    env_override: bool,
    file_present: bool,
    store_has_tokens: bool,
) -> Migration {
    match (
        already_migrated,
        env_override,
        file_present,
        store_has_tokens,
    ) {
        (true, _, _, _) => Migration::AlreadyDone,
        (_, true, _, _) => Migration::Deferred,
        (_, _, true, _) => Migration::FileWins,
        (_, _, _, false) => Migration::NothingToMove,
        _ => Migration::Write,
    }
}

/// Move the store's overlay theme into the theme file, once.
///
/// Runs at setup, after the settings load and the first theme-file read and
/// before any window shows the overlay, so the first frame is already drawn
/// from the file. Returns the path it wrote, when it wrote one.
///
/// Every outcome but [`Migration::Deferred`] retires the stored theme, so this
/// is a one-time pass however it ends, and deleting the theme file later
/// cannot resurrect a theme from before the file was the theme.
pub fn migrate_once(app: &AppHandle) -> Option<PathBuf> {
    let mut settings = get_settings(app);
    // "Is a file there", not "did a file parse". A document Handy could not
    // read is still somebody's, and replacing it would drop the keys and the
    // typo the user is halfway through fixing.
    let step = migration_step(
        settings.overlay_theme_migrated,
        overlay_theme_file::env_override_in_effect(),
        overlay_theme_file::any_candidate_exists(app),
        settings.overlay_theme != OverlayTheme::default(),
    );

    let written = match step {
        Migration::AlreadyDone => return None,
        Migration::Deferred => {
            info!(
                "{THEME_FILE_ENV_VAR} names the theme file, so the stored overlay theme stays \
                 where it is until the variable is unset"
            );
            return None;
        }
        Migration::FileWins => {
            info!(
                "A theme file is already in place, so it is the overlay theme; the stored \
                 overlay theme is retired unread"
            );
            None
        }
        Migration::NothingToMove => {
            debug!("No stored overlay theme to move into the theme file");
            None
        }
        Migration::Write => match overlay_theme_file::config_theme_file(app) {
            Some(path) => {
                let theme = settings.overlay_theme.normalized();
                match create_from_store(&path, &theme) {
                    Ok(()) => {
                        info!(
                            "Migrated the stored overlay theme to {}; it is the overlay theme \
                             from now on",
                            path.display()
                        );
                        Some(path)
                    }
                    Err(problem) => {
                        // Left unmarked on purpose: the next launch tries
                        // again rather than losing the theme silently.
                        warn!("Could not migrate the stored overlay theme: {problem}");
                        return None;
                    }
                }
            }
            None => {
                warn!(
                    "No ~/.config/handy/ to migrate the stored overlay theme into; it stays in \
                     the settings store"
                );
                return None;
            }
        },
    };

    settings.overlay_theme_migrated = true;
    write_settings(app, settings);
    written
}

/// Write the stored theme to `path`, but only onto nothing at all.
///
/// The migration is the one write with no user behind it, so it must never
/// replace a document: not a file that will not parse, not a symlink a theming
/// tool owns, not a read-only file. Both halves matter: the first refuses
/// anything that is there, the second a path not Handy's even when empty.
fn create_from_store(path: &Path, theme: &OverlayTheme) -> Result<(), String> {
    let _writing = writing();

    if overlay_theme_file::anything_at(path) {
        return Err(format!(
            "{} already exists, so it is the overlay theme and the stored one is not migrated",
            path.display()
        ));
    }

    let ownership = overlay_theme_file::ownership_at(path);
    if !ownership.writable {
        return Err(format!(
            "{} is not Handy's to create ({:?})",
            path.display(),
            ownership.reason
        ));
    }

    let text = document_text(theme, None)?;
    install(path, &text, theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay_theme::{HexColor, Material, WaveformStyle};

    fn hex(raw: &str) -> Option<HexColor> {
        Some(HexColor::parse(raw).expect("test colours are valid"))
    }

    fn a_theme() -> OverlayTheme {
        OverlayTheme {
            accent: hex("#7aa2f7"),
            material: Some(Material::Glass),
            radius: Some(12),
            show_cancel: Some(false),
            waveform_style: Some(WaveformStyle::Ribbon),
            ..Default::default()
        }
    }

    /// The README's shape, as a hand editor meets it: `version` first, the set
    /// tokens in the contract's order, two spaces, one trailing newline.
    /// Written out in full rather than re-derived: this text is the contract.
    #[test]
    fn a_document_is_version_first_then_the_set_tokens_in_contract_order() {
        assert_eq!(
            document_text(&a_theme(), None).expect("a theme renders"),
            concat!(
                "{\n",
                "  \"version\": 1,\n",
                "  \"accent\": \"#7aa2f7\",\n",
                "  \"material\": \"glass\",\n",
                "  \"show_cancel\": false,\n",
                "  \"radius\": 12,\n",
                "  \"waveform_style\": \"ribbon\"\n",
                "}\n",
            )
        );
    }

    /// Inherit is an absent key, never an explicit null, so resetting every
    /// token leaves the version row alone, not twenty-two nulls.
    #[test]
    fn an_all_inherit_theme_writes_only_the_version() {
        assert_eq!(
            document_text(&OverlayTheme::default(), None).expect("inherit renders"),
            "{\n  \"version\": 1\n}\n"
        );
    }

    /// Anything Handy does not recognise belongs to whoever wrote it: the
    /// documented `_comment`, and keys from a schema this build predates.
    #[test]
    fn unknown_keys_and_a_newer_version_survive_a_write() {
        let existing = concat!(
            "{\n",
            "  \"version\": 4,\n",
            "  \"_comment\": \"generated by omarchy-theme-set\",\n",
            "  \"accent\": \"#ff0000\",\n",
            "  \"future_token\": { \"light\": \"#fff\" }\n",
            "}\n"
        );

        let written = document_text(&a_theme(), Some(existing)).expect("a theme renders");

        assert_eq!(
            written,
            concat!(
                "{\n",
                "  \"version\": 4,\n",
                "  \"accent\": \"#7aa2f7\",\n",
                "  \"material\": \"glass\",\n",
                "  \"show_cancel\": false,\n",
                "  \"radius\": 12,\n",
                "  \"waveform_style\": \"ribbon\",\n",
                "  \"_comment\": \"generated by omarchy-theme-set\",\n",
                "  \"future_token\": {\"light\":\"#fff\"}\n",
                "}\n",
            ),
            "unknown keys keep their values and follow the tokens"
        );

        // A document that is not an object contributes nothing to preserve,
        // and the version falls back to this build's.
        assert!(document_text(&a_theme(), Some("nonsense"))
            .expect("a broken file is replaced, not refused")
            .starts_with("{\n  \"version\": 1,\n"));
    }

    /// Every document Handy writes has to be one Handy reads: the tokens come
    /// back out as they went in, for every kind of value in the contract.
    #[test]
    fn a_written_document_reads_back_as_the_theme_it_was_given() {
        let theme = OverlayTheme {
            accent: hex("#7aa2f7"),
            surface: hex("#1a1b26"),
            surface_opacity: Some(0.92),
            glass_tint: Some(0.45),
            text: hex("#c0caf5"),
            border: hex("#ffffff"),
            border_opacity: Some(0.3),
            material: Some(Material::Glass),
            glass_material: Some(crate::overlay_theme::GlassMaterial::Popover),
            glass_style: Some(crate::overlay_theme::GlassStyle::Clear),
            shadow_strength: Some(0.35),
            shadow_offset_y: Some(6),
            show_waveform: Some(true),
            show_cancel: Some(false),
            size_scale: Some(1.1),
            radius: Some(12),
            border_width: Some(1),
            padding: Some(14),
            element_gap: Some(8),
            waveform_style: Some(WaveformStyle::Ribbon),
            waveform_gap: Some(2),
            waveform_width: Some(4),
        }
        .normalized();

        let text = document_text(&theme, None).expect("a full theme renders");
        assert_eq!(
            overlay_theme_file::tokens_of(&text).expect("Handy reads what Handy writes"),
            theme
        );
        assert_eq!(
            text.matches('\n').count(),
            25,
            "an opening brace, twenty-three rows and a closing brace"
        );
    }

    /// The write is atomic and verified: the file appears whole, and the temp
    /// file it went through is gone.
    #[test]
    fn installing_a_document_leaves_the_file_whole_and_no_leftovers() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let path = directory.path().join("overlay_theme.json");
        let theme = a_theme();
        let text = document_text(&theme, None).expect("a theme renders");

        install(&path, &text, &theme).expect("the write lands");
        assert_eq!(
            std::fs::read_to_string(&path).expect("the file is there"),
            text
        );

        let leftovers: Vec<_> = std::fs::read_dir(directory.path())
            .expect("the directory reads")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "overlay_theme.json")
            .collect();
        assert_eq!(leftovers, Vec::<String>::new());

        // A second write replaces the first outright.
        let replacement = document_text(&OverlayTheme::default(), Some(&text))
            .expect("an all-inherit theme renders");
        install(&path, &replacement, &OverlayTheme::default()).expect("the second write lands");
        assert_eq!(
            std::fs::read_to_string(&path).expect("the file is still there"),
            replacement
        );
    }

    /// The read-back check is the guard, so it must actually fail a document
    /// whose tokens do not come back, and take the temp file with it.
    #[test]
    fn a_document_that_does_not_read_back_never_becomes_the_theme() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let path = directory.path().join("overlay_theme.json");
        std::fs::write(&path, "{\n  \"version\": 1\n}\n").expect("a starting document");

        // A theme the rendered text cannot read back as.
        let mismatch = install(&path, "{\n  \"version\": 1\n}\n", &a_theme());
        assert!(mismatch.is_err(), "{mismatch:?}");

        // Neither the target nor a temp file was left in a broken state.
        assert_eq!(
            std::fs::read_to_string(&path).expect("the previous document survives"),
            "{\n  \"version\": 1\n}\n"
        );
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("the directory reads")
                .count(),
            1
        );
    }

    /// The write guard: "Handy owns this file" versus "Handy reads somebody
    /// else's". Refused, and the document is left byte for byte as it was.
    #[test]
    fn a_commit_refuses_a_theme_file_handy_does_not_own() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let original = "{\n  \"version\": 1\n}\n";

        let read_only = directory.path().join(overlay_theme_file::THEME_FILE_NAME);
        std::fs::write(&read_only, original).expect("the temp dir is writable");
        let mut permissions = std::fs::metadata(&read_only)
            .expect("the file is there")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&read_only, permissions).expect("permissions are settable");

        assert!(save_to(&read_only, &a_theme()).is_err());
        assert_eq!(
            std::fs::read_to_string(&read_only).expect("the file is still there"),
            original
        );

        #[cfg(unix)]
        {
            let real = directory.path().join("tokyo.json");
            std::fs::write(&real, original).expect("the temp dir is writable");
            let link = directory.path().join("linked.json");
            std::os::unix::fs::symlink(&real, &link).expect("the temp dir takes a symlink");

            assert!(save_to(&link, &a_theme()).is_err());
            assert_eq!(
                std::fs::read_to_string(&real).expect("the tool's document is untouched"),
                original
            );
            assert!(link.is_symlink(), "and the link is still a link");
        }
    }

    /// A document that will not parse keeps applying its last good values, and
    /// the tab says so. Changing anything then is the user's explicit act, and
    /// has to leave a file that reads.
    #[test]
    fn a_commit_repairs_a_document_that_would_not_parse() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let path = directory.path().join(overlay_theme_file::THEME_FILE_NAME);
        std::fs::write(&path, "{ \"accent\": ").expect("the temp dir is writable");

        save_to(&path, &a_theme()).expect("a commit over a broken file lands");
        assert_eq!(
            overlay_theme_file::tokens_of(
                &std::fs::read_to_string(&path).expect("the file is there")
            )
            .expect("and it reads"),
            a_theme().normalized()
        );
    }

    /// The migration is the one write with nobody behind it, so it may only
    /// create. A file that parses, a file that does not and a link a theming
    /// tool owns are all somebody's, and all three survive it.
    #[test]
    fn the_migration_never_touches_an_existing_file_parseable_or_not() {
        // `create_from_store` asks whether an absent path is Handy's, which
        // reads `HANDY_OVERLAY_THEME_FILE`.
        let _lock = overlay_theme_file::env_var_test_lock();
        let directory = tempfile::tempdir().expect("a temp dir");
        let theme = a_theme();

        for (name, contents) in [
            ("broken.json", "{ not json at all"),
            ("good.json", "{\n  \"version\": 1\n}\n"),
        ] {
            let path = directory.path().join(name);
            std::fs::write(&path, contents).expect("the temp dir is writable");
            assert!(
                create_from_store(&path, &theme).is_err(),
                "{name} is already the overlay theme"
            );
            assert_eq!(
                std::fs::read_to_string(&path).expect("the file is still there"),
                contents
            );
        }

        #[cfg(unix)]
        {
            let dangling = directory.path().join("dangling.json");
            std::os::unix::fs::symlink(directory.path().join("nowhere.json"), &dangling)
                .expect("the temp dir takes a symlink");
            assert!(
                create_from_store(&dangling, &theme).is_err(),
                "a link that leads nowhere is still a link somebody made"
            );
            assert!(dangling.is_symlink());
        }

        // Nothing at all is the one case this exists for, folder and all.
        let fresh = directory.path().join("handy").join("overlay_theme.json");
        create_from_store(&fresh, &theme).expect("the migration creates its own file");
        assert_eq!(
            overlay_theme_file::tokens_of(&std::fs::read_to_string(&fresh).expect("it is there"))
                .expect("and it reads"),
            theme.normalized()
        );
    }

    /// The migration's whole rule: once, never over a file already there,
    /// never for a store that only inherits.
    #[test]
    fn the_migration_runs_once_and_only_with_something_to_move() {
        assert_eq!(
            migration_step(false, false, false, true),
            Migration::Write,
            "a stored theme and no file anywhere is the case this exists for"
        );

        // Once it has run, nothing looks at the store again, whatever became
        // of the file since.
        for file_present in [false, true] {
            for store_has_tokens in [false, true] {
                assert_eq!(
                    migration_step(true, false, file_present, store_has_tokens),
                    Migration::AlreadyDone
                );
            }
        }

        // A tool's file wins, and an empty store has nothing to move; both
        // retire the stored theme rather than leaving it pending forever.
        assert_eq!(
            migration_step(false, false, true, true),
            Migration::FileWins
        );
        assert_eq!(
            migration_step(false, false, false, false),
            Migration::NothingToMove
        );

        // The env var outranks all of it: writing ~/.config/handy/ while it
        // points elsewhere would create a document nothing reads.
        for file_present in [false, true] {
            assert_eq!(
                migration_step(false, true, file_present, true),
                Migration::Deferred
            );
        }
    }
}
