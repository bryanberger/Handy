//! The theme file `overlay_theme.json`, a read-only input from outside Handy.
//!
//! An external theming tool drives the overlay without the settings window.
//! Handy only reads it, never writing, moving or rewriting. The one folder it
//! creates is `~/.config/handy/`, behind the Appearance tab's Open button,
//! never a path [`THEME_FILE_ENV_VAR`] named. Only typed tokens come out:
//! canonical `#rrggbb` colours, `flat | glass`, the eight macOS
//! `glass_material` and two `glass_style` values, and numbers rounded and
//! clamped to the token contract's bounds. No CSS, stylesheet, script, font,
//! path, URL or command is read, so a hostile file at worst costs one
//! session's styling.
//!
//! Two tiers of failure, mirroring `salvage_settings` one level up. A
//! document-level problem (unreadable, not UTF-8, malformed JSON, not an
//! object) keeps the last good document and marks it
//! [`ThemeFileState::stale`]; a key-level problem costs that one key, which
//! then inherits. Deleting the file says "stop overriding", so it clears the
//! cache instead.
//!
//! One `open` serves both the metadata check and a bounded sub-KiB read, so a
//! candidate cannot be swapped between them. Runs at launch, on every overlay
//! show (off the main thread) and when the Appearance tab asks. No file
//! watcher.
//!
//! Forward compatibility, promised to the tools that write this file. Colour
//! values are `"#RRGGBB"` strings today; a future version may also accept
//! `{ "light": "#RRGGBB", "dark": "#RRGGBB" }` under the same key names.
//! Writers emitting a single string stay valid, and readers should tolerate
//! either shape.

use crate::overlay_theme::{
    GlassMaterial, GlassStyle, HexColor, Material, OverlayTheme, ThemeFileDiagnostic,
    ThemeFileDiagnosticCode, ThemeFileState, BORDER_OPACITY_MAX, BORDER_OPACITY_MIN,
    BORDER_WIDTH_MAX, ELEMENT_GAP_MAX, GLASS_TINT_MAX, GLASS_TINT_MIN, PADDING_MAX, RADIUS_MAX,
    SHADOW_OFFSET_Y_MAX, SHADOW_STRENGTH_MAX, SHADOW_STRENGTH_MIN, SIZE_SCALE_MAX, SIZE_SCALE_MIN,
    SURFACE_OPACITY_MAX, SURFACE_OPACITY_MIN, WAVEFORM_GAP_MAX, WAVEFORM_WIDTH_MAX,
    WAVEFORM_WIDTH_MIN,
};
use log::{debug, warn};
use serde_json::Value;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tauri::AppHandle;

/// The theme file's name, identical in every location it may live.
pub const THEME_FILE_NAME: &str = "overlay_theme.json";

/// The schema version this build documents and accepts without comment. A
/// missing `version` means this; a newer one is parsed best-effort.
pub const CURRENT_OVERLAY_THEME_FILE_VERSION: u32 = 1;

/// The environment variable that names a theme file outright.
///
/// A path, not a flag, so `std::env::var_os` rather than
/// `utils::env_flag_enabled`. `std::env::var` collapses "unset" and "not valid
/// Unicode" into one `Err` behind a careless `.ok()`. Set to anything, even an
/// unusable path, it is the only candidate tried, so an explicit instruction
/// cannot resolve elsewhere. See [`EnvOverride`].
pub const THEME_FILE_ENV_VAR: &str = "HANDY_OVERLAY_THEME_FILE";

/// Where theming tools write, under the config home. `~/.config/handy/` is the
/// bare product name, not the bundle identifier, as Discussion #1802 asked.
const CONFIG_SUBDIR: &str = "handy";

/// The environment variable that moves the config home off `~/.config`.
///
/// Read on every platform, not Linux only. A dotfile manager setting XDG's
/// variable on macOS or Windows is the setup `~/.config/handy/` serves, at one
/// lookup. Per XDG, empty is unset and a relative value invalid.
const CONFIG_HOME_ENV_VAR: &str = "XDG_CONFIG_HOME";

/// Anything larger than this is not a twenty-one-key document; refused unread.
const MAX_THEME_FILE_BYTES: u64 = 64 * 1024;

/// Diagnostics carried in [`ThemeFileState`]. All reach the log; this bounds
/// only the payload, since a hostile document's unknown keys are unbounded.
const MAX_DIAGNOSTICS: usize = 5;

/// A token's key, and how a value for it becomes an [`OverlayTheme`] field.
struct TokenSpec {
    key: &'static str,
    /// Parse `value` into `tokens`, diagnosing what it rejects or clamps, and
    /// report whether the file set the key. [`TokenSpec::parse`] passes it.
    parser: fn(&str, &Value, &mut OverlayTheme, &mut Vec<ThemeFileDiagnostic>) -> bool,
}

impl TokenSpec {
    /// Read this row's value out of the document.
    ///
    /// Diagnostics use the row's own key, so it is not a parameter.
    fn parse(
        &self,
        value: &Value,
        tokens: &mut OverlayTheme,
        diagnostics: &mut Vec<ThemeFileDiagnostic>,
    ) -> bool {
        (self.parser)(self.key, value, tokens, diagnostics)
    }
}

const fn token(
    key: &'static str,
    parser: fn(&str, &Value, &mut OverlayTheme, &mut Vec<ThemeFileDiagnostic>) -> bool,
) -> TokenSpec {
    TokenSpec { key, parser }
}

/// The twenty-one tokens in the token contract's order, which the Appearance tab,
/// [`ThemeFileState::owned_keys`] and the per-key diagnostics all follow, so
/// the payload does not depend on `serde_json`'s key order.
///
/// One table, not a key list beside a match on it. Key, parser and bounds are
/// one fact; splitting them made a mismatch a runtime debug assertion.
const TOKENS: [TokenSpec; 21] = [
    token("accent", |key, value, tokens, diagnostics| {
        tokens.accent = parse_color(key, value, diagnostics);
        tokens.accent.is_some()
    }),
    token("surface", |key, value, tokens, diagnostics| {
        tokens.surface = parse_color(key, value, diagnostics);
        tokens.surface.is_some()
    }),
    token("surface_opacity", |key, value, tokens, diagnostics| {
        tokens.surface_opacity = parse_ratio(
            key,
            value,
            SURFACE_OPACITY_MIN,
            SURFACE_OPACITY_MAX,
            diagnostics,
        );
        tokens.surface_opacity.is_some()
    }),
    token("glass_tint", |key, value, tokens, diagnostics| {
        tokens.glass_tint = parse_ratio(key, value, GLASS_TINT_MIN, GLASS_TINT_MAX, diagnostics);
        tokens.glass_tint.is_some()
    }),
    token("text", |key, value, tokens, diagnostics| {
        tokens.text = parse_color(key, value, diagnostics);
        tokens.text.is_some()
    }),
    token("border", |key, value, tokens, diagnostics| {
        tokens.border = parse_color(key, value, diagnostics);
        tokens.border.is_some()
    }),
    token("border_opacity", |key, value, tokens, diagnostics| {
        tokens.border_opacity = parse_ratio(
            key,
            value,
            BORDER_OPACITY_MIN,
            BORDER_OPACITY_MAX,
            diagnostics,
        );
        tokens.border_opacity.is_some()
    }),
    token("material", |key, value, tokens, diagnostics| {
        tokens.material = parse_material(key, value, diagnostics);
        tokens.material.is_some()
    }),
    token("glass_material", |key, value, tokens, diagnostics| {
        tokens.glass_material = parse_glass_material(key, value, diagnostics);
        tokens.glass_material.is_some()
    }),
    token("glass_style", |key, value, tokens, diagnostics| {
        tokens.glass_style = parse_glass_style(key, value, diagnostics);
        tokens.glass_style.is_some()
    }),
    token("shadow_strength", |key, value, tokens, diagnostics| {
        tokens.shadow_strength = parse_ratio(
            key,
            value,
            SHADOW_STRENGTH_MIN,
            SHADOW_STRENGTH_MAX,
            diagnostics,
        );
        tokens.shadow_strength.is_some()
    }),
    token("shadow_offset_y", |key, value, tokens, diagnostics| {
        tokens.shadow_offset_y = parse_px(key, value, 0, SHADOW_OFFSET_Y_MAX, diagnostics);
        tokens.shadow_offset_y.is_some()
    }),
    token("show_waveform", |key, value, tokens, diagnostics| {
        tokens.show_waveform = parse_switch(key, value, diagnostics);
        tokens.show_waveform.is_some()
    }),
    token("show_cancel", |key, value, tokens, diagnostics| {
        tokens.show_cancel = parse_switch(key, value, diagnostics);
        tokens.show_cancel.is_some()
    }),
    token("size_scale", |key, value, tokens, diagnostics| {
        tokens.size_scale = parse_ratio(key, value, SIZE_SCALE_MIN, SIZE_SCALE_MAX, diagnostics);
        tokens.size_scale.is_some()
    }),
    token("radius", |key, value, tokens, diagnostics| {
        tokens.radius = parse_px(key, value, 0, RADIUS_MAX, diagnostics);
        tokens.radius.is_some()
    }),
    token("border_width", |key, value, tokens, diagnostics| {
        tokens.border_width = parse_px(key, value, 0, BORDER_WIDTH_MAX, diagnostics);
        tokens.border_width.is_some()
    }),
    token("padding", |key, value, tokens, diagnostics| {
        tokens.padding = parse_px(key, value, 0, PADDING_MAX, diagnostics);
        tokens.padding.is_some()
    }),
    token("element_gap", |key, value, tokens, diagnostics| {
        tokens.element_gap = parse_px(key, value, 0, ELEMENT_GAP_MAX, diagnostics);
        tokens.element_gap.is_some()
    }),
    token("waveform_gap", |key, value, tokens, diagnostics| {
        tokens.waveform_gap = parse_px(key, value, 0, WAVEFORM_GAP_MAX, diagnostics);
        tokens.waveform_gap.is_some()
    }),
    token("waveform_width", |key, value, tokens, diagnostics| {
        tokens.waveform_width = parse_px(
            key,
            value,
            WAVEFORM_WIDTH_MIN,
            WAVEFORM_WIDTH_MAX,
            diagnostics,
        );
        tokens.waveform_width.is_some()
    }),
];

/// The one top-level key that is not a token.
const VERSION_KEY: &str = "version";

/// The last read, shared by every consumer.
///
/// Warmed at launch before the overlay window exists, refreshed on every
/// overlay show and by the Appearance tab's Reload button.
static CACHE: RwLock<Option<ThemeFileState>> = RwLock::new(None);

/// Where Handy looks for the theme file, highest priority first.
///
/// The first readable regular file wins, alone; locations are never merged.
/// Empty when [`THEME_FILE_ENV_VAR`] cannot become a path, as [`read`]
/// reports.
pub fn candidate_paths(app: &AppHandle) -> Vec<PathBuf> {
    match env_override() {
        EnvOverride::Invalid => Vec::new(),
        EnvOverride::Unset => candidates_from(Locations {
            env: None,
            portable_data: portable_data_dir().as_deref(),
            config_home: config_home(app).as_deref(),
            app_data: platform_app_data_dir(app).as_deref(),
        }),
        // `candidates_from` already returns the singleton list for `env`, so
        // routing through it keeps "the env var is exclusive" in one place.
        EnvOverride::Path(path) => candidates_from(Locations {
            env: Some(&path),
            ..Locations::default()
        }),
    }
}

/// What the Appearance tab's Open button does when no theme file exists yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevealTarget {
    /// Create this directory, parents and all, if it is missing, then open it.
    /// Only ever Handy's own documented location, `~/.config/handy/`.
    Create(PathBuf),
    /// Open this directory as it is. It exists, so nothing is created.
    Open(PathBuf),
}

/// Where the Open button should land, from the real environment.
///
/// The impure half. Two lookups, then [`location_to_reveal`], the rule itself.
pub fn reveal_target(app: &AppHandle) -> Result<RevealTarget, String> {
    let named = match env_override() {
        EnvOverride::Invalid => {
            return Err(format!(
                "{THEME_FILE_ENV_VAR} is set to a value that is not valid UTF-8, so it names no folder to open"
            ))
        }
        EnvOverride::Path(path) => Some(path),
        EnvOverride::Unset => None,
    };

    location_to_reveal(named.as_deref(), config_theme_file(app).as_deref(), |dir| {
        dir.is_dir()
    })
}

/// The Open button's rule, pure over its two paths and a directory predicate,
/// so every branch is testable without an `AppHandle` or a temp tree.
///
/// Handy creates a directory only under its own documented `~/.config/handy/`.
/// A path from [`THEME_FILE_ENV_VAR`] belongs to whoever set it (a Nix store
/// path, an unmounted volume, a typo), and Handy will not build a tree where
/// it was told only to read. An env-named path opens the nearest existing
/// folder; with no root either, the error names the variable rather than
/// opening elsewhere.
fn location_to_reveal(
    named: Option<&Path>,
    config_file: Option<&Path>,
    is_directory: impl Fn(&Path) -> bool,
) -> Result<RevealTarget, String> {
    let Some(named) = named else {
        return config_file
            .and_then(containing_directory)
            .map(RevealTarget::Create)
            .ok_or_else(|| {
                "No home directory, so there is no ~/.config/handy/ to create or open".to_string()
            });
    };

    containing_directory(named)
        .and_then(|directory| nearest_existing(&directory, is_directory))
        .map(RevealTarget::Open)
        .ok_or_else(|| {
            format!(
                "{THEME_FILE_ENV_VAR} names {}, and no folder along that path exists. Handy does not create folders under {THEME_FILE_ENV_VAR}: make it yourself, or unset the variable to use ~/.config/handy/",
                named.display()
            )
        })
}

/// The closest directory at or above `directory` that already exists.
///
/// Reads only, creating nothing. `None` when nothing on the path is a
/// directory, including a relative one whose ancestors end at `""`.
fn nearest_existing(directory: &Path, is_directory: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    directory
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .find(|ancestor| is_directory(ancestor))
        .map(Path::to_path_buf)
}

/// Create `dir` and any missing parent, so a printed path becomes a folder.
///
/// The one filesystem write in this module, and it writes a directory, never
/// `overlay_theme.json`. Reached only for [`RevealTarget::Create`], so always
/// Handy's own `~/.config/handy/`. An existing directory is success.
pub fn ensure_location(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("Cannot create {}: {error}", dir.display()))
}

/// The directory holding `path`, if it names one. A bare file name's parent is
/// `""`, which nobody can open, so `None`, not the working directory.
fn containing_directory(path: &Path) -> Option<PathBuf> {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

/// Read the theme file, update the cache, and return what it contributes.
///
/// Never panics, never writes, never falls back from the file
/// [`THEME_FILE_ENV_VAR`] named. Filesystem IO, so off the main thread bar the
/// launch warm-up.
pub fn read(app: &AppHandle) -> ThemeFileState {
    let state = match env_override() {
        // Set but not a usable path, so nothing to search and, per
        // `THEME_FILE_ENV_VAR`'s contract, nothing else tried. A
        // document-level diagnostic naming the variable, since no candidate
        // list was built.
        EnvOverride::Invalid => log_truncate_and_attach_diagnostics(
            ThemeFileState::absent_at(THEME_FILE_ENV_VAR.to_string()),
            vec![diagnostic(
                ThemeFileDiagnosticCode::Unreadable,
                Some(THEME_FILE_ENV_VAR.to_string()),
                "is set to a value that is not valid UTF-8, so it cannot be used as a path; no other theme file location is tried".to_string(),
            )],
        ),
        EnvOverride::Path(path) => {
            let previous = cached_state();
            // The env var's target is always the reported path, present or
            // not, and being exclusive it is the file to create when missing.
            read_candidates(
                std::slice::from_ref(&path),
                true,
                Some(&path),
                previous.as_ref(),
            )
        }
        EnvOverride::Unset => {
            let candidates = candidate_paths(app);
            let previous = cached_state();
            read_candidates(
                &candidates,
                false,
                absent_path(app).as_deref(),
                previous.as_ref(),
            )
        }
    };

    store_cache(&state);
    state
}

/// The last read, performing one read if the cache has never been populated.
///
/// Only cold before the launch-time [`read`], so in a running app this is a
/// lock and a clone, letting the resolver run on the main thread.
pub fn cached(app: &AppHandle) -> ThemeFileState {
    match cached_state() {
        Some(state) => state,
        None => read(app),
    }
}

/// The directories a candidate list is built from, lookups already done. Every
/// field is a directory, except `env`, which names a file outright.
///
/// A struct, not four positional `Option<&Path>` arguments, because swapping
/// two same-typed parameters would be a silent priority change that compiles.
#[derive(Default)]
struct Locations<'a> {
    /// [`THEME_FILE_ENV_VAR`]'s target, a file. Exclusive.
    env: Option<&'a Path>,
    /// `<exe dir>/Data`, present only for a portable install.
    portable_data: Option<&'a Path>,
    /// `$XDG_CONFIG_HOME`, or `~/.config`. `handy/` is joined here.
    config_home: Option<&'a Path>,
    /// The OS app data directory, `<data dir>/com.pais.handy`.
    app_data: Option<&'a Path>,
}

/// The candidate list, in priority order.
///
/// Pure, so order and exclusivity are testable without an `AppHandle`.
/// `handy/` and the file name are joined here; callers only find directories.
fn candidates_from(locations: Locations) -> Vec<PathBuf> {
    // The env var is exclusive. An explicit path is the whole list; a missing
    // target warns rather than falling back to a file the user did not name.
    if let Some(path) = locations.env {
        return vec![path.to_path_buf()];
    }

    let mut candidates = Vec::with_capacity(3);
    // A portable install's promise is that everything it needs sits beside the
    // executable, so its own `Data/` outranks anything in the user's home.
    if let Some(dir) = locations.portable_data {
        candidates.push(dir.join(THEME_FILE_NAME));
    }
    // Documented on every platform, and what the tab prints with no file.
    if let Some(dir) = locations.config_home {
        candidates.push(dir.join(CONFIG_SUBDIR).join(THEME_FILE_NAME));
    }
    // Kept last so a file written where older builds told users to put it
    // still drives the overlay. Nothing is migrated or moved.
    if let Some(dir) = locations.app_data {
        candidates.push(dir.join(THEME_FILE_NAME));
    }
    candidates
}

/// What [`THEME_FILE_ENV_VAR`] contributes to the candidate search.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EnvOverride {
    /// Not set, or empty. The other candidates are tried normally.
    Unset,
    /// Set, to a path.
    Path(PathBuf),
    /// Set to a value that is not valid Unicode. `std::env::var_os` still
    /// returns it, since environment variables are just bytes everywhere Handy
    /// ships, but this module builds `Path`s only from `str`, so there is
    /// nothing to try, join or display. Exclusive like `Path`, so the search
    /// stops with a diagnostic rather than falling back to app data or XDG.
    Invalid,
}

/// The file [`THEME_FILE_ENV_VAR`] names, from the real process environment.
fn env_override() -> EnvOverride {
    env_candidate_os(std::env::var_os(THEME_FILE_ENV_VAR).as_deref())
}

/// [`env_override`]'s logic, pure over `std::env::var_os`'s result, so the
/// non-Unicode branch is testable. It cannot be expressed as `Option<&str>`.
fn env_candidate_os(value: Option<&OsStr>) -> EnvOverride {
    match value {
        None => EnvOverride::Unset,
        Some(value) if value.is_empty() => EnvOverride::Unset,
        Some(value) => match value.to_str() {
            // Delegates to `env_candidate` rather than building a `PathBuf`
            // here, so "a usable path" has one definition. `text` is
            // non-empty, so this always lands in `Path`; the fallback is
            // defensive.
            Some(text) => env_candidate(Some(text)).map_or(EnvOverride::Unset, EnvOverride::Path),
            None => EnvOverride::Invalid,
        },
    }
}

/// The env var's value as a candidate. Unset and empty are the same thing.
/// Pure, so this is testable without touching the process environment.
fn env_candidate(value: Option<&str>) -> Option<PathBuf> {
    match value {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => None,
    }
}

/// `<exe dir>/Data`, and only for a portable install.
///
/// Not [`crate::portable::app_data_dir`], which stands in for the OS directory
/// when the marker is present. Here they are separate candidates, so a
/// portable install reads its own `Data/` first and app data last.
fn portable_data_dir() -> Option<PathBuf> {
    crate::portable::data_dir().cloned()
}

/// The OS app data directory, `<data dir>/com.pais.handy`, portable or not.
fn platform_app_data_dir(app: &AppHandle) -> Option<PathBuf> {
    use tauri::Manager;

    match app.path().app_data_dir() {
        Ok(dir) => Some(dir),
        Err(error) => {
            warn!("Cannot locate the app data directory for the theme file: {error}");
            None
        }
    }
}

/// The user's config home: `$XDG_CONFIG_HOME`, else `~/.config`.
///
/// Not Tauri's `config_dir()`, which is `~/.config` only on Linux and returns
/// `~/Library/Application Support` and `%APPDATA%` elsewhere. One documented
/// path has to work everywhere, so it is built from the home directory, giving
/// `%USERPROFILE%\.config\handy\` on Windows.
fn config_home(app: &AppHandle) -> Option<PathBuf> {
    use tauri::Manager;

    let home = match app.path().home_dir() {
        Ok(dir) => Some(dir),
        Err(error) => {
            debug!("No home directory for the theme file: {error}");
            None
        }
    };
    config_home_from(
        std::env::var_os(CONFIG_HOME_ENV_VAR).as_deref(),
        home.as_deref(),
    )
}

/// [`config_home`]'s rule, pure over an injected home and environment value.
///
/// Per XDG, empty means unset and a relative value is invalid, ignored rather
/// than joined against Handy's working directory, wherever that is.
fn config_home_from(xdg: Option<&OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    let configured = xdg
        .map(Path::new)
        .filter(|dir| !dir.as_os_str().is_empty() && dir.is_absolute());

    match configured {
        Some(dir) => Some(dir.to_path_buf()),
        None => home.map(|dir| dir.join(".config")),
    }
}

/// `~/.config/handy/overlay_theme.json`: the candidate the Appearance tab
/// prints when no file exists anywhere, and the directory Open creates.
fn config_theme_file(app: &AppHandle) -> Option<PathBuf> {
    config_home(app).map(|dir| dir.join(CONFIG_SUBDIR).join(THEME_FILE_NAME))
}

/// The path to report when no candidate holds a file, from the real lookups.
///
/// A missing home directory is only a `debug!` in [`config_home`], since
/// `$XDG_CONFIG_HOME` can answer without one. Both empty earns a warning,
/// because the documented location is unavailable and the tab must name app
/// data.
fn absent_path(app: &AppHandle) -> Option<PathBuf> {
    let config_file = config_theme_file(app);
    if config_file.is_none() {
        warn!(
            "No home directory and no absolute {CONFIG_HOME_ENV_VAR}, so ~/.config/{CONFIG_SUBDIR}/ \
             is unavailable; the app data directory is where the theme file is reported instead"
        );
    }

    absent_path_from(
        config_file.as_deref(),
        platform_app_data_dir(app).as_deref(),
    )
}

/// [`absent_path`]'s rule, pure over the two locations it chooses between.
///
/// Ordinarily `~/.config/handy/overlay_theme.json`, so the tab says where to
/// create a file; app data is only the fallback for files already there.
/// Without a home, app data stands in, since an empty path says nothing.
fn absent_path_from(config_file: Option<&Path>, app_data: Option<&Path>) -> Option<PathBuf> {
    config_file
        .map(Path::to_path_buf)
        .or_else(|| app_data.map(|dir| dir.join(THEME_FILE_NAME)))
}

/// What one candidate path yielded.
enum CandidateRead {
    /// Nothing usable here; try the next candidate. Carries a warning when the
    /// path exists but is not a theme file (a directory, a device, or too big).
    Absent(Option<String>),
    /// The file's text, ready to parse.
    Text(String),
    /// The file is there but could not be read. The search stops, because a
    /// file that exists is the file in effect, even when reading it failed.
    Unreadable(String),
}

/// Read one candidate, refusing anything that is not a small regular file.
///
/// One `open`, with metadata and bytes both from that handle, rather than
/// `fs::metadata` then a separate `fs::read`. Two calls could check one file
/// and read another after a non-atomic replace or a retargeted symlink,
/// dodging the type check or the size cap; on POSIX the open descriptor keeps
/// referring to the original file even if the path is unlinked. [`Read::take`]
/// at one byte over [`MAX_THEME_FILE_BYTES`] caps the read independently of
/// the metadata check, so an understated size cannot land more than a byte
/// over the limit in memory.
///
/// Opening follows symlinks, so the symlink-to-a-real-file a Nix or
/// Home-Manager setup needs still works, while the check rejecting a directory
/// or a device rejects a symlink to one too. [`dangling_symlink_or_absent`]
/// handles a dangling one.
fn load_candidate(path: &Path) -> CandidateRead {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return dangling_symlink_or_absent(path);
        }
        Err(error) => {
            return CandidateRead::Unreadable(format!("cannot be read ({error})"));
        }
    };

    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => return CandidateRead::Unreadable(format!("cannot be read ({error})")),
    };

    if !metadata.is_file() {
        return CandidateRead::Absent(Some("is not a regular file; ignored".to_string()));
    }
    if metadata.len() > MAX_THEME_FILE_BYTES {
        return CandidateRead::Absent(Some(format!(
            "is {} bytes, over the {MAX_THEME_FILE_BYTES} byte limit; ignored",
            metadata.len()
        )));
    }

    let mut bytes = Vec::new();
    if let Err(error) = file.take(MAX_THEME_FILE_BYTES + 1).read_to_end(&mut bytes) {
        return CandidateRead::Unreadable(format!("cannot be read ({error})"));
    }
    // Belt-and-braces against a file that grew since the metadata check.
    // `Read::take` stops one byte over the limit whatever `metadata.len()`
    // said, so a swapped-in larger file never reaches `String::from_utf8`.
    if bytes.len() as u64 > MAX_THEME_FILE_BYTES {
        return CandidateRead::Absent(Some(format!(
            "grew past the {MAX_THEME_FILE_BYTES} byte limit while being read; ignored"
        )));
    }

    match String::from_utf8(bytes) {
        Ok(text) => CandidateRead::Text(text),
        Err(_) => CandidateRead::Unreadable("is not valid UTF-8".to_string()),
    }
}

/// Tell a plain absence apart from a symlink whose target does not exist.
///
/// Called only after `File::open` failed with `NotFound`, which a dangling
/// symlink also produces since opening follows the link. `symlink_metadata`
/// does not follow links, so it succeeds with a missing target, telling the
/// two apart. Absent like a directory, but worth one `warn!`, because a
/// symlink pointing at nothing is a misconfiguration, not the ordinary "no
/// file here yet".
fn dangling_symlink_or_absent(path: &Path) -> CandidateRead {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            CandidateRead::Absent(Some("is a symlink with no target; ignored".to_string()))
        }
        _ => CandidateRead::Absent(None),
    }
}

/// Walk the candidates and build the state, keeping the last good document
/// when a file that exists cannot be turned into one.
///
/// Takes the previous state instead of reading the cache, so the retention
/// rule is testable with temp files and no `AppHandle`.
fn read_candidates(
    candidates: &[PathBuf],
    env_exclusive: bool,
    fallback_path: Option<&Path>,
    previous: Option<&ThemeFileState>,
) -> ThemeFileState {
    // Warnings from skipped candidates. They survive into whatever state the
    // search ends in, because a directory sitting where a theme file belongs
    // is worth saying out loud even when a lower-priority file is found.
    let mut skipped = Vec::new();

    for path in candidates {
        match load_candidate(path) {
            CandidateRead::Absent(None) => continue,
            CandidateRead::Absent(Some(message)) => {
                skipped.push(diagnostic(
                    ThemeFileDiagnosticCode::Unreadable,
                    None,
                    format!("{} {message}", path.display()),
                ));
                continue;
            }
            CandidateRead::Unreadable(message) => {
                let mut diagnostics = skipped;
                diagnostics.push(diagnostic(
                    ThemeFileDiagnosticCode::Unreadable,
                    None,
                    message,
                ));
                return keep_last_good(path, previous, diagnostics);
            }
            CandidateRead::Text(text) => {
                return match parse_document(&text) {
                    Ok(document) => {
                        let mut diagnostics = skipped;
                        diagnostics.extend(document.diagnostics);
                        log_truncate_and_attach_diagnostics(
                            ThemeFileState {
                                path: path.display().to_string(),
                                present: true,
                                version: document.version,
                                tokens: document.tokens,
                                owned_keys: document.owned_keys,
                                diagnostics: Vec::new(),
                                diagnostics_total: 0, // set below, from the real count
                                stale: false,
                            },
                            diagnostics,
                        )
                    }
                    Err(problem) => {
                        let mut diagnostics = skipped;
                        diagnostics.push(problem);
                        keep_last_good(path, previous, diagnostics)
                    }
                };
            }
        }
    }

    // Nothing found. Deleting the file is the documented way to stop
    // overriding, so this clears the tokens instead of keeping the last good
    // one.
    let path = fallback_path.map(|path| path.display().to_string());
    if env_exclusive {
        skipped.push(diagnostic(
            ThemeFileDiagnosticCode::Unreadable,
            Some(THEME_FILE_ENV_VAR.to_string()),
            format!(
                "{THEME_FILE_ENV_VAR} names {}, which does not exist; no other location is tried",
                path.clone().unwrap_or_default()
            ),
        ));
    } else {
        debug!(
            "No theme file at {}; the overlay theme comes from the settings",
            path.clone().unwrap_or_else(|| "any candidate".to_string())
        );
    }

    log_truncate_and_attach_diagnostics(
        ThemeFileState::absent_at(path.unwrap_or_default()),
        skipped,
    )
}

/// A document that failed to parse or read keeps the previous one, so a
/// non-atomic write cannot snap the overlay back to the settings theme for a
/// dictation. On a first read there is nothing to keep, so the tokens stay
/// empty.
fn keep_last_good(
    path: &Path,
    previous: Option<&ThemeFileState>,
    diagnostics: Vec<ThemeFileDiagnostic>,
) -> ThemeFileState {
    let path = path.display().to_string();

    match previous.filter(|state| state.present) {
        Some(good) => log_truncate_and_attach_diagnostics(
            ThemeFileState {
                path,
                present: true,
                version: good.version,
                tokens: good.tokens.clone(),
                owned_keys: good.owned_keys.clone(),
                diagnostics: Vec::new(),
                diagnostics_total: 0, // set below, from the real count
                stale: true,
            },
            diagnostics,
        ),
        None => log_truncate_and_attach_diagnostics(ThemeFileState::absent_at(path), diagnostics),
    }
}

/// Log every diagnostic, truncate to [`MAX_DIAGNOSTICS`], and attach the
/// capped list and the pre-cap count to the state.
///
/// The log gets every diagnostic, [`ThemeFileState::diagnostics`] at most
/// [`MAX_DIAGNOSTICS`], and [`ThemeFileState::diagnostics_total`] the pre-cap
/// count, which lets the tab say "…and N more" rather than just "more".
fn log_truncate_and_attach_diagnostics(
    mut state: ThemeFileState,
    diagnostics: Vec<ThemeFileDiagnostic>,
) -> ThemeFileState {
    for problem in &diagnostics {
        warn!("Theme file {}: {}", state.path, problem.message);
    }

    state.diagnostics_total = diagnostics.len() as u32;
    state.diagnostics = diagnostics;
    state.diagnostics.truncate(MAX_DIAGNOSTICS);
    state
}

/// A parsed document, before it becomes a [`ThemeFileState`].
struct ParsedDocument {
    version: Option<u32>,
    tokens: OverlayTheme,
    owned_keys: Vec<String>,
    diagnostics: Vec<ThemeFileDiagnostic>,
}

/// Parse a theme document. `Err` is the one document-level diagnostic that
/// makes the whole file contribute nothing new.
///
/// Pure and over `&str`, so every leniency rule and every per-key failure is
/// testable without touching the filesystem.
fn parse_document(text: &str) -> Result<ParsedDocument, ThemeFileDiagnostic> {
    // PowerShell's `>` and some editors emit a BOM `serde_json` rejects.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);

    let value: Value = serde_json::from_str(text).map_err(|error| {
        diagnostic(
            ThemeFileDiagnosticCode::MalformedDocument,
            None,
            format!("is not valid JSON: {error}"),
        )
    })?;

    let Some(object) = value.as_object() else {
        return Err(diagnostic(
            ThemeFileDiagnosticCode::MalformedDocument,
            None,
            "must be a JSON object with the token keys at the top level".to_string(),
        ));
    };

    let mut diagnostics = Vec::new();
    let version = parse_version(object.get(VERSION_KEY), &mut diagnostics);

    // One line naming every offender, because a typo and a key from a newer
    // schema look the same here and a silently ignored typo is this feature's
    // likeliest failure. `key` carries the list for the tab's message.
    let unknown: Vec<&str> = object
        .keys()
        .map(String::as_str)
        .filter(|key| *key != VERSION_KEY && !TOKENS.iter().any(|token| token.key == *key))
        .collect();
    if !unknown.is_empty() {
        let names = unknown.join(", ");
        diagnostics.push(diagnostic(
            ThemeFileDiagnosticCode::UnknownKey,
            Some(names.clone()),
            format!("ignored unknown keys: {names}"),
        ));
    }

    let mut tokens = OverlayTheme::default();
    let mut owned_keys = Vec::new();

    // Fixed contract order, not the document's. `serde_json` sorts keys unless
    // `preserve_order` is on, so document order cannot be honoured anyway.
    for token in &TOKENS {
        let Some(value) = object.get(token.key) else {
            continue;
        };
        // Explicit null spells inherit, not a value to complain about.
        if value.is_null() {
            continue;
        }

        if token.parse(value, &mut tokens, &mut diagnostics) {
            owned_keys.push(token.key.to_string());
        }
    }

    Ok(ParsedDocument {
        version,
        tokens,
        owned_keys,
        diagnostics,
    })
}

/// Parse `version`. Absent means 1, a newer one is parsed best-effort, and
/// anything that is not a positive integer is ignored.
///
/// A newer document is not rejected because the safety is in the rest of the
/// contract, not the number. Unknown keys and bad values already cost only
/// themselves, so a v1 build applies what it understands.
fn parse_version(value: Option<&Value>, diagnostics: &mut Vec<ThemeFileDiagnostic>) -> Option<u32> {
    let value = value?;
    if value.is_null() {
        return None;
    }

    match value.as_u64() {
        Some(version) if (1..=u32::MAX as u64).contains(&version) => {
            let version = version as u32;
            if version > CURRENT_OVERLAY_THEME_FILE_VERSION {
                diagnostics.push(diagnostic(
                    ThemeFileDiagnosticCode::UnsupportedVersion,
                    Some(VERSION_KEY.to_string()),
                    format!(
                        "'version' is {version}, newer than the {CURRENT_OVERLAY_THEME_FILE_VERSION} this build understands; values it does not know were ignored"
                    ),
                ));
            }
            Some(version)
        }
        _ => {
            diagnostics.push(diagnostic(
                ThemeFileDiagnosticCode::UnsupportedVersion,
                Some(VERSION_KEY.to_string()),
                format!("'version' is {value}, not a positive integer; reading the document as version {CURRENT_OVERLAY_THEME_FILE_VERSION}"),
            ));
            None
        }
    }
}

/// A colour token, a JSON string parsed by the one lenient colour parser.
fn parse_color(
    key: &str,
    value: &Value,
    diagnostics: &mut Vec<ThemeFileDiagnostic>,
) -> Option<HexColor> {
    let raw = expect_string(key, value, diagnostics)?;

    match HexColor::parse(raw) {
        Some(colour) => Some(colour),
        None => {
            diagnostics.push(diagnostic(
                ThemeFileDiagnosticCode::InvalidColor,
                Some(key.to_string()),
                color_problem(key, raw),
            ));
            None
        }
    }
}

/// Why a colour was refused. The alpha forms get their own sentence, because
/// dropping the alpha silently would misapply intent that belongs in the two
/// alpha tokens.
fn color_problem(key: &str, raw: &str) -> String {
    let trimmed = raw.trim();
    let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
    let carries_alpha =
        matches!(digits.len(), 4 | 8) && digits.chars().all(|digit| digit.is_ascii_hexdigit());

    if carries_alpha {
        format!("'{key}' is {raw:?}, which carries alpha; colours are '#rrggbb' and transparency is 'surface_opacity' or 'glass_tint'")
    } else {
        format!("'{key}' is {raw:?}, not a '#rrggbb' colour")
    }
}

/// `material` is the closed enum, matched case-insensitively because this
/// document is written by hand and by third-party tools.
fn parse_material(
    key: &str,
    value: &Value,
    diagnostics: &mut Vec<ThemeFileDiagnostic>,
) -> Option<Material> {
    let raw = expect_string(key, value, diagnostics)?;

    match raw.trim().to_ascii_lowercase().as_str() {
        "flat" => Some(Material::Flat),
        "glass" => Some(Material::Glass),
        _ => {
            diagnostics.push(diagnostic(
                ThemeFileDiagnosticCode::WrongType,
                Some(key.to_string()),
                format!("'{key}' is {raw:?}; expected \"flat\" or \"glass\""),
            ));
            None
        }
    }
}

/// `glass_material` is the closed enum of macOS materials, matched
/// case-insensitively and ignoring `-`/` `, so `"HUD Window"`,
/// `"hud-window"` and `"hud_window"` all land on one value. Hand-written and
/// tool-written alike.
fn parse_glass_material(
    key: &str,
    value: &Value,
    diagnostics: &mut Vec<ThemeFileDiagnostic>,
) -> Option<GlassMaterial> {
    let raw = expect_string(key, value, diagnostics)?;
    let normalized: String = raw
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();

    GlassMaterial::ALL
        .into_iter()
        .find(|material| material.as_key().replace('_', "") == normalized)
        .or_else(|| {
            let expected = GlassMaterial::ALL
                .map(|material| format!("\"{}\"", material.as_key()))
                .join(", ");
            diagnostics.push(diagnostic(
                ThemeFileDiagnosticCode::WrongType,
                Some(key.to_string()),
                format!("'{key}' is {raw:?}; expected one of {expected}"),
            ));
            None
        })
}

/// `glass_style` takes one of the two Liquid Glass styles, matched
/// case-insensitively and ignoring `-`/` ` like [`parse_glass_material`],
/// so hand-written and tool-written documents spell them freely.
fn parse_glass_style(
    key: &str,
    value: &Value,
    diagnostics: &mut Vec<ThemeFileDiagnostic>,
) -> Option<GlassStyle> {
    let raw = expect_string(key, value, diagnostics)?;
    let normalized: String = raw
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect();

    GlassStyle::ALL
        .into_iter()
        .find(|style| style.as_key() == normalized)
        .or_else(|| {
            let expected = GlassStyle::ALL
                .map(|style| format!("\"{}\"", style.as_key()))
                .join(", ");
            diagnostics.push(diagnostic(
                ThemeFileDiagnosticCode::WrongType,
                Some(key.to_string()),
                format!("'{key}' is {raw:?}; expected one of {expected}"),
            ));
            None
        })
}

/// A 0 to 1 style token (`surface_opacity`, `glass_tint`, `border_opacity`,
/// `size_scale`), read as a JSON number and clamped to the contract's bounds,
/// with a diagnostic when it moved.
fn parse_ratio(
    key: &str,
    value: &Value,
    min: f64,
    max: f64,
    diagnostics: &mut Vec<ThemeFileDiagnostic>,
) -> Option<f64> {
    let number = expect_number(key, value, diagnostics)?;
    let clamped = number.clamp(min, max);

    if clamped != number {
        diagnostics.push(out_of_bounds(
            key,
            &number.to_string(),
            &clamped.to_string(),
        ));
    }
    Some(clamped)
}

/// A px token (`radius`, `border_width`, `padding`, `waveform_gap`,
/// `waveform_width`), read as a JSON number, rounded half away from zero, then
/// clamped to `min..=max`. A float is accepted, a numeric string is not.
///
/// `min` is 0 for all but `waveform_width`, whose bars vanish under 2 px.
fn parse_px(
    key: &str,
    value: &Value,
    min: u16,
    max: u16,
    diagnostics: &mut Vec<ThemeFileDiagnostic>,
) -> Option<u16> {
    let number = expect_number(key, value, diagnostics)?;
    let rounded = number.round();
    let clamped = rounded.clamp(f64::from(min), f64::from(max));

    if clamped != rounded {
        diagnostics.push(out_of_bounds(
            key,
            &number.to_string(),
            &clamped.to_string(),
        ));
    }
    Some(clamped as u16)
}

/// A switch token (`show_waveform`, `show_cancel`), read as a JSON boolean.
///
/// The contract's first boolean, and deliberately the strictest reader in the
/// file: `true` and `false` only. There is nothing to be lenient about, since
/// no spelling question arises, and `"true"`, `1` and `"yes"` are all a
/// theming tool emitting the wrong JSON type, which is exactly what
/// `WrongType` is for.
fn parse_switch(
    key: &str,
    value: &Value,
    diagnostics: &mut Vec<ThemeFileDiagnostic>,
) -> Option<bool> {
    match value.as_bool() {
        Some(switch) => Some(switch),
        None => {
            diagnostics.push(wrong_type(key, "true or false", value));
            None
        }
    }
}

/// A JSON string, or a `WrongType` diagnostic. Numbers, booleans, objects and
/// arrays are type errors. Leniency covers how a colour or an enum is spelled,
/// never which JSON type carries it.
fn expect_string<'a>(
    key: &str,
    value: &'a Value,
    diagnostics: &mut Vec<ThemeFileDiagnostic>,
) -> Option<&'a str> {
    match value.as_str() {
        Some(raw) => Some(raw),
        None => {
            diagnostics.push(wrong_type(key, "a string", value));
            None
        }
    }
}

/// A finite JSON number, or a `WrongType` diagnostic. `"12"` and `"12px"` are
/// type errors, because numbers are JSON numbers.
fn expect_number(
    key: &str,
    value: &Value,
    diagnostics: &mut Vec<ThemeFileDiagnostic>,
) -> Option<f64> {
    match value.as_f64() {
        Some(number) if number.is_finite() => Some(number),
        _ => {
            diagnostics.push(wrong_type(key, "a number", value));
            None
        }
    }
}

/// One key ignored because its value is not the JSON type the contract wants.
fn wrong_type(key: &str, expected: &str, value: &Value) -> ThemeFileDiagnostic {
    diagnostic(
        ThemeFileDiagnosticCode::WrongType,
        Some(key.to_string()),
        format!("'{key}' is {value}; expected {expected}. Ignoring it"),
    )
}

/// One number moved to the nearest bound.
fn out_of_bounds(key: &str, given: &str, clamped: &str) -> ThemeFileDiagnostic {
    diagnostic(
        ThemeFileDiagnosticCode::OutOfBounds,
        Some(key.to_string()),
        format!("'{key}' is {given}, outside the allowed range; using {clamped}"),
    )
}

/// One diagnostic. `code` is the stable identity the tab translates, `message`
/// the English detail that goes to the log.
fn diagnostic(
    code: ThemeFileDiagnosticCode,
    key: Option<String>,
    message: String,
) -> ThemeFileDiagnostic {
    ThemeFileDiagnostic { code, key, message }
}

/// The cached state, or `None` when nothing has been read yet.
fn cached_state() -> Option<ThemeFileState> {
    match CACHE.read() {
        Ok(cache) => cache.clone(),
        // Nothing here panics while holding the lock, so this is unreachable in
        // practice; recovering beats propagating a panic into the overlay.
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Publish a state as the current one.
fn store_cache(state: &ThemeFileState) {
    match CACHE.write() {
        Ok(mut cache) => *cache = Some(state.clone()),
        Err(poisoned) => *poisoned.into_inner() = Some(state.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes the tests touching the real `HANDY_OVERLAY_THEME_FILE`, so
    /// two cannot interleave under the multi-threaded runner and leave one
    /// another's value behind. Almost every test here uses the pure
    /// `env_candidate` / `env_candidate_os` seams and needs no lock. Only
    /// `env_override` needs it.
    static ENV_VAR_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Sets [`THEME_FILE_ENV_VAR`] for the life of the guard and restores it
    /// on drop, panic included, since `Drop` runs while an assertion failure
    /// unwinds. A failing test cannot leak a mutated variable into the next
    /// one. Holds [`ENV_VAR_TEST_LOCK`] for the same reason.
    struct EnvVarGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(value: impl AsRef<OsStr>) -> Self {
            let lock = ENV_VAR_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous = std::env::var_os(THEME_FILE_ENV_VAR);
            std::env::set_var(THEME_FILE_ENV_VAR, value);
            EnvVarGuard {
                _lock: lock,
                previous,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(THEME_FILE_ENV_VAR, value),
                None => std::env::remove_var(THEME_FILE_ENV_VAR),
            }
        }
    }

    /// The contract's inherit-everything example, byte-identical and frozen. A
    /// schema change that breaks it needs tolerance, not an edited fixture.
    const EXAMPLE_INHERIT: &str = r##"{ "version": 1 }"##;

    /// The contract's explicit spelling of the same thing.
    const EXAMPLE_EXPLICIT_NULLS: &str = r##"{
  "version": 1,
  "accent": null,
  "surface": null,
  "surface_opacity": null,
  "glass_tint": null,
  "text": null,
  "border": null,
  "border_opacity": null,
  "material": null,
  "glass_material": null,
  "glass_style": null,
  "shadow_strength": null,
  "shadow_offset_y": null,
  "show_waveform": null,
  "show_cancel": null,
  "size_scale": null,
  "radius": null,
  "border_width": null,
  "padding": null,
  "element_gap": null,
  "waveform_gap": null,
  "waveform_width": null
}"##;

    /// The contract's custom theme, byte-identical to its worked example.
    const EXAMPLE_CUSTOM: &str = r##"{
  "version": 1,
  "accent": "#7aa2f7",
  "surface": "#1a1b26",
  "surface_opacity": 0.92,
  "glass_tint": 0.45,
  "text": "#c0caf5",
  "border": "#ffffff",
  "border_opacity": 0.3,
  "material": "glass",
  "glass_material": "popover",
  "glass_style": "clear",
  "shadow_strength": 0.35,
  "shadow_offset_y": 6,
  "show_waveform": true,
  "show_cancel": false,
  "size_scale": 1.1,
  "radius": 12,
  "border_width": 1,
  "padding": 14,
  "element_gap": 8,
  "waveform_gap": 2,
  "waveform_width": 4
}"##;

    /// The contract's theming-tool document: every leniency, a comment key,
    /// and a key from a schema this build does not have.
    const EXAMPLE_THEMING_TOOL: &str = r##"{
  "version": 1,
  "_comment": "generated by omarchy-theme-set; do not edit",
  "accent": "#8AADF4",
  "surface": "24273a",
  "text": "#cad",
  "surface_opacity": 1,
  "material": "Flat",
  "app_theme": "dark"
}"##;

    fn hex(raw: &str) -> Option<HexColor> {
        Some(HexColor::parse(raw).expect("test colours are valid"))
    }

    fn parse(text: &str) -> ParsedDocument {
        parse_document(text).expect("the document parses")
    }

    fn codes(diagnostics: &[ThemeFileDiagnostic]) -> Vec<ThemeFileDiagnosticCode> {
        diagnostics.iter().map(|problem| problem.code).collect()
    }

    fn messages(diagnostics: &[ThemeFileDiagnostic]) -> String {
        diagnostics
            .iter()
            .map(|problem| problem.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("the temp dir is writable");
        path
    }

    /// `~/.config/handy/` is documented, so it outranks app data, which stays
    /// on the list only so a file written where older builds pointed still
    /// works. A portable `Data/` outranks both, everything beside the
    /// executable.
    #[test]
    fn candidate_paths_are_in_priority_order() {
        let portable = PathBuf::from("/opt/handy/Data");
        let config = PathBuf::from("/home/user/.config");
        let app_data = PathBuf::from("/data/com.pais.handy");

        assert_eq!(
            candidates_from(Locations {
                env: None,
                portable_data: Some(&portable),
                config_home: Some(&config),
                app_data: Some(&app_data),
            }),
            vec![
                PathBuf::from("/opt/handy/Data/overlay_theme.json"),
                PathBuf::from("/home/user/.config/handy/overlay_theme.json"),
                PathBuf::from("/data/com.pais.handy/overlay_theme.json"),
            ]
        );

        // Ordinary install, no marker, so `~/.config/handy/` first of two.
        assert_eq!(
            candidates_from(Locations {
                config_home: Some(&config),
                app_data: Some(&app_data),
                ..Locations::default()
            }),
            vec![
                PathBuf::from("/home/user/.config/handy/overlay_theme.json"),
                PathBuf::from("/data/com.pais.handy/overlay_theme.json"),
            ]
        );
        // Nothing to look at is an empty list, not a panic.
        assert!(candidates_from(Locations::default()).is_empty());
    }

    /// The same order on every platform, differing only in what the two home
    /// lookups return. `~/.config/handy/` is no longer Linux-only, and never
    /// Tauri's `config_dir()`, which is app data again on macOS and Windows.
    #[test]
    fn the_config_location_is_the_same_shape_on_every_platform() {
        let cases = [
            (
                "macOS",
                "/Users/user",
                "/Users/user/Library/Application Support/com.pais.handy",
            ),
            (
                "Windows",
                r"C:\Users\user",
                r"C:\Users\user\AppData\Roaming\com.pais.handy",
            ),
            (
                "Linux",
                "/home/user",
                "/home/user/.local/share/com.pais.handy",
            ),
        ];

        for (platform, home, app_data) in cases {
            let config = config_home_from(None, Some(Path::new(home)))
                .unwrap_or_else(|| panic!("{platform} has a home directory"));
            let candidates = candidates_from(Locations {
                config_home: Some(&config),
                app_data: Some(Path::new(app_data)),
                ..Locations::default()
            });

            // Joined rather than written out, because the separator is the
            // host's. A literal `C:\Users\user\.config\...` passes on Windows
            // and fails on macOS, and the components are what this test is
            // about.
            let expected_config_file = Path::new(home)
                .join(".config")
                .join("handy")
                .join("overlay_theme.json");

            assert_eq!(
                candidates,
                vec![
                    expected_config_file,
                    Path::new(app_data).join(THEME_FILE_NAME)
                ],
                "{platform}"
            );
        }
    }

    /// `$XDG_CONFIG_HOME` moves the config location on every platform. Per
    /// XDG, empty is unset and a relative value invalid, not resolved against
    /// wherever Handy was started.
    #[test]
    fn the_config_home_follows_xdg_config_home_when_it_is_usable() {
        let home = Path::new("/Users/user");

        assert_eq!(
            config_home_from(Some(OsStr::new("/Users/user/dotfiles/config")), Some(home)),
            Some(PathBuf::from("/Users/user/dotfiles/config"))
        );
        assert_eq!(
            config_home_from(None, Some(home)),
            Some(PathBuf::from("/Users/user/.config"))
        );
        assert_eq!(
            config_home_from(Some(OsStr::new("")), Some(home)),
            Some(PathBuf::from("/Users/user/.config")),
            "empty is unset"
        );
        assert_eq!(
            config_home_from(Some(OsStr::new("relative/config")), Some(home)),
            Some(PathBuf::from("/Users/user/.config")),
            "a relative XDG_CONFIG_HOME is invalid and ignored"
        );
        // A set variable still answers with no home directory to fall back to.
        assert_eq!(
            config_home_from(Some(OsStr::new("/etc/xdg")), None),
            Some(PathBuf::from("/etc/xdg"))
        );
        assert_eq!(config_home_from(None, None), None);
    }

    /// First found wins; app data only when `~/.config/handy/` has none.
    #[test]
    fn the_config_file_beats_the_app_data_file_and_the_app_data_file_still_loads() {
        let root = tempfile::tempdir().expect("a temp dir");
        let config_home = root.path().join(".config");
        let handy_config = config_home.join(CONFIG_SUBDIR);
        let app_data = root.path().join("com.pais.handy");
        std::fs::create_dir_all(&handy_config).expect("the temp dir is writable");
        std::fs::create_dir_all(&app_data).expect("the temp dir is writable");

        let config_file = write(
            &handy_config,
            THEME_FILE_NAME,
            r##"{"version":1,"accent":"#7aa2f7"}"##,
        );
        let app_data_file = write(
            &app_data,
            THEME_FILE_NAME,
            r##"{"version":1,"accent":"#f7768e"}"##,
        );

        let candidates = candidates_from(Locations {
            config_home: Some(&config_home),
            app_data: Some(&app_data),
            ..Locations::default()
        });
        assert_eq!(candidates, vec![config_file.clone(), app_data_file.clone()]);

        let state = read_candidates(&candidates, false, Some(&config_file), None);
        assert!(state.present);
        assert_eq!(state.path, config_file.display().to_string());
        assert_eq!(state.tokens.accent, hex("#7aa2f7"));

        // Delete the winner and the fallback loads, unchanged and unmoved.
        std::fs::remove_file(&config_file).expect("the temp dir is writable");
        let state = read_candidates(&candidates, false, Some(&config_file), Some(&state));
        assert!(state.present);
        assert_eq!(state.path, app_data_file.display().to_string());
        assert_eq!(state.tokens.accent, hex("#f7768e"));
    }

    /// With no file anywhere, the reported path is what the tab tells the user
    /// to create, `~/.config/handy/overlay_theme.json`, not app data.
    #[test]
    fn the_absent_path_is_the_config_location() {
        let root = tempfile::tempdir().expect("a temp dir");
        let config_home = root.path().join(".config");
        let app_data = root.path().join("com.pais.handy");
        let config_file = config_home.join(CONFIG_SUBDIR).join(THEME_FILE_NAME);

        let state = read_candidates(
            &candidates_from(Locations {
                config_home: Some(&config_home),
                app_data: Some(&app_data),
                ..Locations::default()
            }),
            false,
            Some(&config_file),
            None,
        );

        assert!(!state.present);
        assert_eq!(state.tokens, OverlayTheme::default());
        assert_eq!(state.path, config_file.display().to_string());
        // Nothing was found, but nothing was wrong either.
        assert!(state.diagnostics.is_empty());
        assert_eq!(state.diagnostics_total, 0);
    }

    /// With no home directory, the tab names app data, not an empty path.
    #[test]
    fn without_a_config_location_the_app_data_candidate_is_reported() {
        let app_data = Path::new("/data/com.pais.handy");
        let app_data_file = app_data.join(THEME_FILE_NAME);

        // The pure seam with no home: one candidate, the fallback one.
        let candidates = candidates_from(Locations {
            config_home: None,
            app_data: Some(app_data),
            ..Locations::default()
        });
        assert_eq!(candidates, vec![app_data_file.clone()]);

        // And that candidate is what an absent theme file is reported at.
        assert_eq!(
            absent_path_from(None, Some(app_data)),
            Some(app_data_file.clone())
        );
        // With a home, the documented location wins, as everywhere else here.
        let config_file = Path::new("/home/user/.config/handy/overlay_theme.json");
        assert_eq!(
            absent_path_from(Some(config_file), Some(app_data)),
            Some(config_file.to_path_buf())
        );
        // Neither location resolvable is an empty path, not a panic.
        assert_eq!(absent_path_from(None, None), None);

        // End to end: nothing found anywhere still names a path.
        let state = read_candidates(
            &candidates,
            false,
            absent_path_from(None, Some(app_data)).as_deref(),
            None,
        );
        assert!(!state.present);
        assert_eq!(state.path, app_data_file.display().to_string());
    }

    /// Where the Open button lands, and where it may not create. Pure, over
    /// injected env and config paths and an injected directory check.
    #[test]
    fn revealing_creates_only_under_the_config_location() {
        let existing = ["/Volumes", "/Volumes/backup"];
        let is_directory = |dir: &Path| existing.iter().any(|known| Path::new(known) == dir);
        let config_file = Path::new("/Users/user/.config/handy/overlay_theme.json");

        // No override means the documented folder, created if missing. The one
        // branch that may create anything, and it never touches the
        // filesystem, since the point is a folder that is not there yet.
        assert_eq!(
            location_to_reveal(None, Some(config_file), is_directory),
            Ok(RevealTarget::Create(PathBuf::from(
                "/Users/user/.config/handy"
            )))
        );

        // An env-named folder that exists opens as it is.
        assert_eq!(
            location_to_reveal(
                Some(Path::new("/Volumes/backup/overlay_theme.json")),
                Some(config_file),
                is_directory
            ),
            Ok(RevealTarget::Open(PathBuf::from("/Volumes/backup")))
        );
        // One that does not opens the nearest folder that does, so a user
        // whose drive holds no `themes/` yet lands next to it, and Handy has
        // written nothing under a path it was only told to read.
        assert_eq!(
            location_to_reveal(
                Some(Path::new("/Volumes/backup/themes/overlay_theme.json")),
                Some(config_file),
                is_directory
            ),
            Ok(RevealTarget::Open(PathBuf::from("/Volumes/backup")))
        );

        // Nothing along the path exists, so an error names the variable rather
        // than opening the config folder the user did not ask about.
        let error = location_to_reveal(
            Some(Path::new("/nowhere/at/all/overlay_theme.json")),
            Some(config_file),
            is_directory,
        )
        .expect_err("an unmounted volume has no folder to open");
        assert!(error.contains(THEME_FILE_ENV_VAR), "{error}");
        assert!(
            error.contains("/nowhere/at/all/overlay_theme.json"),
            "{error}"
        );

        // A bare file name has no folder, and never the working directory.
        let error = location_to_reveal(
            Some(Path::new(THEME_FILE_NAME)),
            Some(config_file),
            is_directory,
        )
        .expect_err("a bare file name names no folder");
        assert!(error.contains(THEME_FILE_ENV_VAR), "{error}");

        // No home directory and no override: nothing to open, said plainly.
        assert!(location_to_reveal(None, None, is_directory).is_err());
    }

    /// Open on an absent theme file creates the folder it belongs in, so a
    /// path the tab has only printed becomes somewhere to drop a file. An
    /// existing directory is success, and the theme file itself is never
    /// written.
    #[test]
    fn revealing_an_absent_location_creates_the_directory() {
        let root = tempfile::tempdir().expect("a temp dir");
        let handy_config = root.path().join(".config").join(CONFIG_SUBDIR);

        // The directory the Open button is handed, derived as the command
        // derives it, the folder of the reported path.
        assert_eq!(
            containing_directory(&handy_config.join(THEME_FILE_NAME)),
            Some(handy_config.clone())
        );

        assert!(!handy_config.exists());
        ensure_location(&handy_config).expect("the temp dir is writable");
        assert!(handy_config.is_dir(), "the missing parent is created too");
        assert!(
            !handy_config.join(THEME_FILE_NAME).exists(),
            "Handy never writes the theme file itself"
        );

        // Idempotent: the ordinary case is a directory that is already there.
        ensure_location(&handy_config).expect("an existing directory is success");
        assert!(handy_config.is_dir());

        // A bare file name has no directory to open, never the working one.
        assert_eq!(containing_directory(Path::new(THEME_FILE_NAME)), None);
        // A file that cannot become a directory is an error, not a panic.
        let occupied = write(root.path(), "occupied", "not a directory");
        assert!(ensure_location(&occupied).is_err());
    }

    /// An explicit instruction must not quietly resolve to a different file.
    #[test]
    fn env_var_is_exclusive_and_does_not_fall_back() {
        let portable = PathBuf::from("/opt/handy/Data");
        let app_data = PathBuf::from("/data/com.pais.handy");
        let config = PathBuf::from("/home/user/.config");
        let named = PathBuf::from("/nix/store/theme/overlay_theme.json");

        assert_eq!(
            candidates_from(Locations {
                env: Some(&named),
                portable_data: Some(&portable),
                config_home: Some(&config),
                app_data: Some(&app_data),
            }),
            vec![named.clone()]
        );

        // The env var names a path verbatim, not a directory to join onto.
        assert_eq!(
            candidates_from(Locations {
                env: Some(Path::new("/tmp/my-theme.json")),
                app_data: Some(&app_data),
                ..Locations::default()
            }),
            vec![PathBuf::from("/tmp/my-theme.json")]
        );

        // Set-but-empty is unset.
        assert_eq!(env_candidate(Some("")), None);
        assert_eq!(env_candidate(None), None);
        assert_eq!(
            env_candidate(Some("/tmp/t.json")),
            Some(PathBuf::from("/tmp/t.json"))
        );

        // The whole read against a file only the env var knows about, not
        // named `overlay_theme.json`, in neither the app data nor the XDG
        // directory. The Nix/Home-Manager case, and the one path not checkable
        // on screen, since the harness cannot start the dev app with a
        // variable.
        let dir = tempfile::tempdir().expect("a temp dir");
        let elsewhere = write(dir.path(), "my-theme.json", EXAMPLE_CUSTOM);
        let state = read_candidates(
            std::slice::from_ref(&elsewhere),
            true,
            Some(&elsewhere),
            None,
        );

        assert!(state.present);
        assert_eq!(state.tokens.accent, hex("#7aa2f7"));
        assert_eq!(state.path, elsewhere.display().to_string());

        // …and the variable feeds that candidate, checked through real
        // `std::env::var_os` and guarded so it cannot race or leak into
        // another test.
        {
            let _guard = EnvVarGuard::set(elsewhere.as_os_str());
            assert_eq!(env_override(), EnvOverride::Path(elsewhere.clone()));
        }
        assert_eq!(env_override(), EnvOverride::Unset);

        // When the named file is not there, the state says so, names the env
        // var, and contributes nothing, with no app data fallback.
        let missing = dir.path().join("absent.json");
        let state = read_candidates(std::slice::from_ref(&missing), true, Some(&missing), None);

        assert!(!state.present);
        assert_eq!(state.tokens, OverlayTheme::default());
        assert_eq!(state.path, missing.display().to_string());
        assert_eq!(
            codes(&state.diagnostics),
            vec![ThemeFileDiagnosticCode::Unreadable]
        );
        assert!(messages(&state.diagnostics).contains(THEME_FILE_ENV_VAR));
    }

    /// [`env_candidate_os`] tells "unset" apart from "cannot become a path",
    /// which `std::env::var`'s `Result` cannot. A `VarError::NotUnicode`
    /// collapsing to `None` behind an `.ok()` is the bug this fixes, falling
    /// back to app data / XDG instead of stopping. Pure, so no lock.
    #[test]
    fn env_candidate_os_tells_unset_a_path_and_invalid_unicode_apart() {
        assert_eq!(env_candidate_os(None), EnvOverride::Unset);
        assert_eq!(env_candidate_os(Some(OsStr::new(""))), EnvOverride::Unset);
        assert_eq!(
            env_candidate_os(Some(OsStr::new("/tmp/t.json"))),
            EnvOverride::Path(PathBuf::from("/tmp/t.json"))
        );

        // Bytes that are not valid UTF-8 cannot become a `&str` or a `Path`.
        // `env_candidate_os` reports `Invalid`, not unset.
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;

            let invalid = OsStr::from_bytes(&[0x66, 0xff, 0x66]);
            assert!(invalid.to_str().is_none(), "fixture must be non-Unicode");
            assert_eq!(env_candidate_os(Some(invalid)), EnvOverride::Invalid);
        }
    }

    /// [`env_override`] is a thin wrapper over `std::env::var_os`; this pins
    /// that it reaches the real environment, including a non-Unicode value,
    /// the branch [`EnvVarGuard`] exists for since `std::env::set_var` is the
    /// only way one gets into the process. Guarded against races.
    #[test]
    #[cfg(unix)]
    fn env_override_reports_invalid_for_a_real_non_unicode_value() {
        use std::os::unix::ffi::OsStrExt;

        let invalid = OsStr::from_bytes(&[0x66, 0xff, 0x66]);
        let _guard = EnvVarGuard::set(invalid);

        assert_eq!(env_override(), EnvOverride::Invalid);
    }

    /// The inherit-everything document spells "today's overlay". It must
    /// contribute nothing, or the defaults stop reproducing today's look.
    #[test]
    fn the_inherit_example_contributes_nothing() {
        for document in [EXAMPLE_INHERIT, EXAMPLE_EXPLICIT_NULLS, "{}"] {
            let parsed = parse(document);

            assert_eq!(parsed.tokens, OverlayTheme::default(), "{document}");
            assert!(parsed.owned_keys.is_empty(), "{document}");
            assert!(parsed.diagnostics.is_empty(), "{document}");
        }

        // `{}` has no version at all, which means 1.
        assert_eq!(parse(EXAMPLE_INHERIT).version, Some(1));
        assert_eq!(parse("{}").version, None);
    }

    /// The fully custom example from the token contract, token by token.
    #[test]
    fn the_custom_example_round_trips() {
        let parsed = parse(EXAMPLE_CUSTOM);

        assert_eq!(parsed.version, Some(1));
        assert_eq!(
            parsed.tokens,
            OverlayTheme {
                accent: hex("#7aa2f7"),
                surface: hex("#1a1b26"),
                surface_opacity: Some(0.92),
                glass_tint: Some(0.45),
                text: hex("#c0caf5"),
                border: hex("#ffffff"),
                border_opacity: Some(0.3),
                material: Some(Material::Glass),
                glass_material: Some(GlassMaterial::Popover),
                glass_style: Some(GlassStyle::Clear),
                shadow_strength: Some(0.35),
                shadow_offset_y: Some(6),
                show_waveform: Some(true),
                show_cancel: Some(false),
                size_scale: Some(1.1),
                radius: Some(12),
                border_width: Some(1),
                padding: Some(14),
                element_gap: Some(8),
                waveform_gap: Some(2),
                waveform_width: Some(4),
            }
        );
        // Every key is owned, so the tab locks all twenty-one.
        assert_eq!(
            parsed.owned_keys,
            TOKENS.iter().map(|token| token.key).collect::<Vec<_>>()
        );
        assert!(parsed.diagnostics.is_empty());
    }

    /// What a theming tool emits: 3-digit shorthand, a missing `#`, uppercase,
    /// an integer for a float, a comment key, and a key from a newer schema.
    #[test]
    fn the_theming_tool_example_exercises_every_leniency() {
        let parsed = parse(EXAMPLE_THEMING_TOOL);

        assert_eq!(parsed.tokens.accent, hex("#8aadf4"));
        assert_eq!(parsed.tokens.surface, hex("#24273a"));
        assert_eq!(parsed.tokens.text, hex("#ccaadd"));
        assert_eq!(parsed.tokens.surface_opacity, Some(1.0));
        assert_eq!(parsed.tokens.material, Some(Material::Flat));

        // The sixteen tokens it does not mention still inherit.
        assert_eq!(parsed.tokens.glass_tint, None);
        assert_eq!(parsed.tokens.border, None);
        assert_eq!(parsed.tokens.border_opacity, None);
        assert_eq!(parsed.tokens.glass_material, None);
        assert_eq!(parsed.tokens.glass_style, None);
        assert_eq!(parsed.tokens.shadow_strength, None);
        assert_eq!(parsed.tokens.shadow_offset_y, None);
        assert_eq!(parsed.tokens.show_waveform, None);
        assert_eq!(parsed.tokens.show_cancel, None);
        assert_eq!(parsed.tokens.size_scale, None);
        assert_eq!(parsed.tokens.radius, None);
        assert_eq!(parsed.tokens.border_width, None);
        assert_eq!(parsed.tokens.padding, None);
        assert_eq!(parsed.tokens.element_gap, None);
        assert_eq!(parsed.tokens.waveform_gap, None);
        assert_eq!(parsed.tokens.waveform_width, None);

        assert_eq!(
            parsed.owned_keys,
            vec!["accent", "surface", "surface_opacity", "text", "material"]
        );

        // Both non-token keys in one line, so a typo is not read as a comment.
        assert_eq!(
            codes(&parsed.diagnostics),
            vec![ThemeFileDiagnosticCode::UnknownKey]
        );
        let message = messages(&parsed.diagnostics);
        assert!(message.contains("_comment"), "{message}");
        assert!(message.contains("app_theme"), "{message}");
    }

    /// A half-written file must not snap the overlay back to the settings
    /// theme for one dictation.
    #[test]
    fn broken_json_keeps_the_last_good_document() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = write(dir.path(), THEME_FILE_NAME, EXAMPLE_CUSTOM);
        let candidates = [path.clone()];

        let good = read_candidates(&candidates, false, Some(&path), None);
        assert!(good.present);
        assert_eq!(good.tokens.accent, hex("#7aa2f7"));
        assert!(!good.stale);

        // The truncation an external tool's non-atomic write leaves behind.
        write(dir.path(), THEME_FILE_NAME, r##"{"version":1,"acc"##);
        let broken = read_candidates(&candidates, false, Some(&path), Some(&good));

        assert!(broken.present);
        assert!(broken.stale);
        assert_eq!(broken.tokens, good.tokens);
        assert_eq!(broken.owned_keys, good.owned_keys);
        assert_eq!(
            codes(&broken.diagnostics),
            vec![ThemeFileDiagnosticCode::MalformedDocument]
        );

        // A document that is valid JSON but not an object fails the same way.
        write(dir.path(), THEME_FILE_NAME, "[1, 2, 3]");
        let array = read_candidates(&candidates, false, Some(&path), Some(&good));
        assert!(array.stale);
        assert_eq!(array.tokens, good.tokens);

        // With no previous document to keep, the file contributes nothing.
        let cold = read_candidates(&candidates, false, Some(&path), None);
        assert!(!cold.present);
        assert!(!cold.stale);
        assert_eq!(cold.tokens, OverlayTheme::default());
    }

    /// Deleting the file is how a tool says "stop overriding", so it is the one
    /// read failure that does not keep the last good document.
    #[test]
    fn deleted_file_clears_the_cache() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = write(dir.path(), THEME_FILE_NAME, EXAMPLE_CUSTOM);
        let candidates = [path.clone()];

        let good = read_candidates(&candidates, false, Some(&path), None);
        assert!(good.present);
        store_cache(&good);
        assert_eq!(
            cached_state().map(|state| state.tokens),
            Some(good.tokens.clone())
        );

        std::fs::remove_file(&path).expect("the temp file is removable");
        let cleared = read_candidates(&candidates, false, Some(&path), Some(&good));

        assert!(!cleared.present);
        assert!(!cleared.stale);
        assert_eq!(cleared.tokens, OverlayTheme::default());
        assert!(cleared.owned_keys.is_empty());
        assert!(cleared.diagnostics.is_empty());
        // The path still points where a file would go, for the tab to name.
        assert_eq!(cleared.path, path.display().to_string());

        store_cache(&cleared);
        assert_eq!(
            cached_state().map(|state| state.tokens),
            Some(OverlayTheme::default())
        );
    }

    /// Out-of-bounds numbers clamp rather than drop, so the user's intent
    /// (bigger, rounder) survives and the overlay cannot cover the screen.
    #[test]
    fn out_of_bounds_numbers_clamp_with_a_diagnostic() {
        let parsed = parse(
            r##"{
              "size_scale": 9,
              "radius": 99,
              "padding": -3
            }"##,
        );

        assert_eq!(parsed.tokens.size_scale, Some(1.50));
        assert_eq!(parsed.tokens.radius, Some(32));
        assert_eq!(parsed.tokens.padding, Some(0));
        // Clamped rather than ignored, so the tab locks all three.
        assert_eq!(parsed.owned_keys, vec!["size_scale", "radius", "padding"]);

        assert_eq!(
            codes(&parsed.diagnostics),
            vec![
                ThemeFileDiagnosticCode::OutOfBounds,
                ThemeFileDiagnosticCode::OutOfBounds,
                ThemeFileDiagnosticCode::OutOfBounds,
            ]
        );
        let message = messages(&parsed.diagnostics);
        assert!(message.contains("'size_scale' is 9"), "{message}");
        assert!(message.contains("1.5"), "{message}");

        // In bounds is silent; a float px rounds half away from zero first.
        let quiet = parse(r##"{ "size_scale": 1.13, "radius": 12.5, "surface_opacity": 0.3 }"##);
        assert_eq!(quiet.tokens.size_scale, Some(1.13));
        assert_eq!(quiet.tokens.radius, Some(13));
        assert_eq!(quiet.tokens.surface_opacity, Some(0.3));
        assert!(quiet.diagnostics.is_empty());
    }

    /// Glass tint bounds from the token table, not the module's constants. Its
    /// floor is zero, `surface_opacity`'s 0.30, the only place they differ and
    /// why a document can ask for untinted glass but not an invisible Flat
    /// card.
    #[test]
    fn the_glass_tint_parses_clamps_and_reaches_zero() {
        let parsed = parse(r##"{ "glass_tint": 0.15, "surface_opacity": 1 }"##);
        assert_eq!(parsed.tokens.glass_tint, Some(0.15));
        assert_eq!(parsed.tokens.surface_opacity, Some(1.0));
        assert!(parsed.diagnostics.is_empty());
        // In contract order the Glass tint sits beside the Flat opacity it
        // replaces, not with the Material tokens.
        assert_eq!(parsed.owned_keys, vec!["surface_opacity", "glass_tint"]);

        // Zero is in bounds and silent.
        let untinted = parse(r##"{ "glass_tint": 0 }"##);
        assert_eq!(untinted.tokens.glass_tint, Some(0.0));
        assert!(untinted.diagnostics.is_empty());

        // Out of bounds clamps and is reported; the key is still owned.
        let clamped = parse(r##"{ "glass_tint": 4 }"##);
        assert_eq!(clamped.tokens.glass_tint, Some(1.0));
        assert_eq!(clamped.owned_keys, vec!["glass_tint"]);
        assert_eq!(
            codes(&clamped.diagnostics),
            vec![ThemeFileDiagnosticCode::OutOfBounds]
        );

        // A negative value clamps to untinted rather than inheriting.
        let negative = parse(r##"{ "glass_tint": -1 }"##);
        assert_eq!(negative.tokens.glass_tint, Some(0.0));

        // The wrong JSON type costs this key alone.
        let wrong = parse(r##"{ "glass_tint": "0.45", "accent": "#7aa2f7" }"##);
        assert_eq!(wrong.tokens.glass_tint, None);
        assert_eq!(wrong.tokens.accent, hex("#7aa2f7"));
        assert_eq!(
            codes(&wrong.diagnostics),
            vec![ThemeFileDiagnosticCode::WrongType]
        );
    }

    /// Border and waveform token bounds from the token table, not constants.
    #[test]
    fn border_and_waveform_tokens_parse_and_clamp() {
        let parsed = parse(
            r##"{
              "border": "FFF",
              "border_opacity": 0.25,
              "border_width": 2,
              "waveform_width": 5
            }"##,
        );

        assert_eq!(parsed.tokens.border, hex("#ffffff"));
        assert_eq!(parsed.tokens.border_opacity, Some(0.25));
        assert_eq!(parsed.tokens.border_width, Some(2));
        assert_eq!(parsed.tokens.waveform_width, Some(5));
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(
            parsed.owned_keys,
            vec!["border", "border_opacity", "border_width", "waveform_width"]
        );

        // Every one of them clamps to the table's bounds, and
        // `waveform_width` is the only px token with a floor above zero.
        let clamped = parse(
            r##"{
              "border_opacity": 4,
              "border_width": 9,
              "waveform_width": 1
            }"##,
        );
        assert_eq!(clamped.tokens.border_opacity, Some(1.0));
        assert_eq!(clamped.tokens.border_width, Some(4));
        assert_eq!(clamped.tokens.waveform_width, Some(2));
        assert_eq!(
            codes(&clamped.diagnostics),
            vec![
                ThemeFileDiagnosticCode::OutOfBounds,
                ThemeFileDiagnosticCode::OutOfBounds,
                ThemeFileDiagnosticCode::OutOfBounds,
            ]
        );

        // A fully transparent edge is in bounds rather than clamped. It is how
        // a theme asks for no visible border while keeping the width.
        let invisible = parse(r##"{ "border_opacity": 0, "border_width": 0 }"##);
        assert_eq!(invisible.tokens.border_opacity, Some(0.0));
        assert_eq!(invisible.tokens.border_width, Some(0));
        assert!(invisible.diagnostics.is_empty());
    }

    /// The shadow's two tokens and the element gap follow the same two
    /// readers every other number does, so this pins their own bounds.
    #[test]
    fn the_shadow_and_gap_tokens_parse_and_clamp() {
        let parsed = parse(
            r##"{
              "shadow_strength": 0.35,
              "shadow_offset_y": 6,
              "element_gap": 8
            }"##,
        );
        assert_eq!(parsed.tokens.shadow_strength, Some(0.35));
        assert_eq!(parsed.tokens.shadow_offset_y, Some(6));
        assert_eq!(parsed.tokens.element_gap, Some(8));
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(
            parsed.owned_keys,
            vec!["shadow_strength", "shadow_offset_y", "element_gap"]
        );

        let clamped = parse(
            r##"{
              "shadow_strength": 4,
              "shadow_offset_y": 99,
              "element_gap": -3
            }"##,
        );
        assert_eq!(clamped.tokens.shadow_strength, Some(1.0));
        assert_eq!(clamped.tokens.shadow_offset_y, Some(16));
        assert_eq!(clamped.tokens.element_gap, Some(0));
        assert_eq!(
            codes(&clamped.diagnostics),
            vec![
                ThemeFileDiagnosticCode::OutOfBounds,
                ThemeFileDiagnosticCode::OutOfBounds,
                ThemeFileDiagnosticCode::OutOfBounds,
            ]
        );

        // Zero is in bounds for both, and is what a Flat card and today's row
        // already are, so a document may spell them out.
        let off = parse(r##"{ "shadow_strength": 0, "shadow_offset_y": 0, "element_gap": 0 }"##);
        assert_eq!(off.tokens.shadow_strength, Some(0.0));
        assert_eq!(off.tokens.shadow_offset_y, Some(0));
        assert_eq!(off.tokens.element_gap, Some(0));
        assert!(off.diagnostics.is_empty());
    }

    /// The contract's first booleans. `true` and `false` and nothing else: a
    /// string or a number is a theming tool emitting the wrong JSON type, and
    /// gets the same `WrongType` diagnostic a mistyped number would.
    #[test]
    fn the_visibility_switches_take_json_booleans_only() {
        let parsed = parse(r##"{ "show_waveform": false, "show_cancel": true }"##);
        assert_eq!(parsed.tokens.show_waveform, Some(false));
        assert_eq!(parsed.tokens.show_cancel, Some(true));
        assert!(parsed.diagnostics.is_empty());
        // `false` is a value the file owns, not an absence, so the tab locks
        // the row rather than leaving it editable.
        assert_eq!(parsed.owned_keys, vec!["show_waveform", "show_cancel"]);

        for bad in ["\"true\"", "1", "0", "null", "[]", "{}"] {
            let rejected = parse(&format!(r##"{{ "show_waveform": {bad} }}"##));
            assert_eq!(rejected.tokens.show_waveform, None, "{bad}");
            assert!(rejected.owned_keys.is_empty(), "{bad}");
            // `null` is the contract's own spelling of inherit, so it is the
            // one that passes silently.
            let expected = if bad == "null" {
                Vec::new()
            } else {
                vec![ThemeFileDiagnosticCode::WrongType]
            };
            assert_eq!(codes(&rejected.diagnostics), expected, "{bad}");
        }
    }

    /// `glass_material` is hand-written, so the reader takes human separators.
    #[test]
    fn glass_material_spelling_is_lenient_but_closed() {
        for spelling in ["hud_window", "HUD_WINDOW", "hud-window", "HUD Window"] {
            let parsed = parse(&format!(r##"{{ "glass_material": "{spelling}" }}"##));
            assert_eq!(
                parsed.tokens.glass_material,
                Some(GlassMaterial::HudWindow),
                "{spelling}"
            );
            assert!(parsed.diagnostics.is_empty(), "{spelling}");
        }

        assert_eq!(
            parse(r##"{ "glass_material": "under_window_background" }"##)
                .tokens
                .glass_material,
            Some(GlassMaterial::UnderWindowBackground)
        );

        // A material this build does not have is one bad key, not a bad file,
        // and the diagnostic lists what it could have been.
        let unknown = parse(r##"{ "glass_material": "liquid_glass", "radius": 12 }"##);
        assert_eq!(unknown.tokens.glass_material, None);
        assert_eq!(unknown.tokens.radius, Some(12));
        assert_eq!(unknown.owned_keys, vec!["radius"]);
        assert_eq!(
            codes(&unknown.diagnostics),
            vec![ThemeFileDiagnosticCode::WrongType]
        );
        let message = messages(&unknown.diagnostics);
        assert!(message.contains("\"popover\""), "{message}");
    }

    /// `glass_style` is as lenient as `glass_material` and as closed, so a
    /// style this build does not have loses that one key, not the whole file.
    #[test]
    fn glass_style_spelling_is_lenient_but_closed() {
        for spelling in ["clear", "CLEAR", " Clear "] {
            let parsed = parse(&format!(r##"{{ "glass_style": "{spelling}" }}"##));
            assert_eq!(
                parsed.tokens.glass_style,
                Some(GlassStyle::Clear),
                "{spelling}"
            );
            assert!(parsed.diagnostics.is_empty(), "{spelling}");
        }

        assert_eq!(
            parse(r##"{ "glass_style": "regular" }"##)
                .tokens
                .glass_style,
            Some(GlassStyle::Regular)
        );

        let unknown = parse(r##"{ "glass_style": "frosted", "radius": 12 }"##);
        assert_eq!(unknown.tokens.glass_style, None);
        assert_eq!(unknown.tokens.radius, Some(12));
        assert_eq!(unknown.owned_keys, vec!["radius"]);
        assert_eq!(
            codes(&unknown.diagnostics),
            vec![ThemeFileDiagnosticCode::WrongType]
        );
        let message = messages(&unknown.diagnostics);
        assert!(message.contains("\"regular\""), "{message}");

        // A wrong JSON type, not a wrong spelling, costs the same one key.
        let wrong_type = parse(r##"{ "glass_style": 1 }"##);
        assert_eq!(wrong_type.tokens.glass_style, None);
        assert_eq!(
            codes(&wrong_type.diagnostics),
            vec![ThemeFileDiagnosticCode::WrongType]
        );
    }

    /// A typo and a key from a newer schema are indistinguishable here, and a
    /// silently ignored typo is this feature's most likely failure.
    #[test]
    fn unknown_keys_are_ignored_with_a_diagnostic() {
        let parsed = parse(
            r##"{
              "accennt": "#ff0000",
              "accent": "#7aa2f7",
              "app_theme": "dark"
            }"##,
        );

        assert_eq!(parsed.tokens.accent, hex("#7aa2f7"));
        assert_eq!(parsed.owned_keys, vec!["accent"]);

        assert_eq!(
            codes(&parsed.diagnostics),
            vec![ThemeFileDiagnosticCode::UnknownKey]
        );
        // The key list doubles as the parameter of the tab's translated line.
        let key = parsed.diagnostics[0].key.clone().expect("names the keys");
        assert!(key.contains("accennt"), "{key}");
        assert!(key.contains("app_theme"), "{key}");
    }

    /// Leniency is confined to how a colour or an enum is spelled. Everything
    /// else is strict JSON typing, and a bad key costs only itself.
    #[test]
    fn bad_values_cost_only_their_own_key() {
        let parsed = parse(
            r##"{
              "accent": 5,
              "surface": "#1a1b26",
              "text": "rebeccapurple",
              "material": "GLASS",
              "radius": "12px"
            }"##,
        );

        assert_eq!(parsed.tokens.accent, None);
        assert_eq!(parsed.tokens.text, None);
        assert_eq!(parsed.tokens.radius, None);
        // …while the well-formed siblings survive, enum case included.
        assert_eq!(parsed.tokens.surface, hex("#1a1b26"));
        assert_eq!(parsed.tokens.material, Some(Material::Glass));
        assert_eq!(parsed.owned_keys, vec!["surface", "material"]);

        assert_eq!(
            codes(&parsed.diagnostics),
            vec![
                ThemeFileDiagnosticCode::WrongType,    // accent: 5
                ThemeFileDiagnosticCode::InvalidColor, // text: a colour name
                ThemeFileDiagnosticCode::WrongType,    // radius: "12px"
            ]
        );

        // An alpha-carrying colour points at the tokens that do carry alpha.
        let alpha = parse(r##"{ "surface": "#1a1b26ff" }"##);
        assert_eq!(alpha.tokens.surface, None);
        assert!(
            messages(&alpha.diagnostics).contains("glass_tint"),
            "{}",
            messages(&alpha.diagnostics)
        );

        // An unknown enum value names what was allowed.
        let material = parse(r##"{ "material": "frosted" }"##);
        assert_eq!(material.tokens.material, None);
        assert!(
            messages(&material.diagnostics).contains("flat"),
            "{}",
            messages(&material.diagnostics)
        );
    }

    /// A missing version means 1; a newer document is parsed best-effort so a
    /// downgrade loses the keys it cannot read, not the whole theme.
    #[test]
    fn version_is_optional_and_newer_documents_still_apply() {
        let newer = parse(r##"{ "version": 2, "accent": "#7aa2f7" }"##);
        assert_eq!(newer.version, Some(2));
        assert_eq!(newer.tokens.accent, hex("#7aa2f7"));
        assert_eq!(
            codes(&newer.diagnostics),
            vec![ThemeFileDiagnosticCode::UnsupportedVersion]
        );

        // A non-positive-integer `version` is ignored, the document read as 1.
        for bad in [r##""1""##, "0", "-1", "1.5"] {
            let document = format!(r##"{{ "version": {bad}, "accent": "#7aa2f7" }}"##);
            let parsed = parse(&document);

            assert_eq!(parsed.version, None, "{document}");
            assert_eq!(parsed.tokens.accent, hex("#7aa2f7"), "{document}");
            assert_eq!(
                codes(&parsed.diagnostics),
                vec![ThemeFileDiagnosticCode::UnsupportedVersion],
                "{document}"
            );
        }
    }

    /// A UTF-8 BOM is what PowerShell's `>` and several Windows editors write;
    /// `serde_json` rejects it, so the reader strips it before parsing.
    #[test]
    fn a_byte_order_mark_is_stripped() {
        let parsed = parse(&format!("\u{feff}{EXAMPLE_CUSTOM}"));

        assert_eq!(parsed.tokens.accent, hex("#7aa2f7"));
        assert!(parsed.diagnostics.is_empty());
    }

    /// Anything but a small regular file is skipped; the search carries on.
    #[test]
    fn a_directory_at_the_path_is_skipped_with_a_warning() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let blocked = dir.path().join("blocked");
        std::fs::create_dir(&blocked).expect("the temp dir is writable");
        let fallback = write(dir.path(), THEME_FILE_NAME, EXAMPLE_CUSTOM);

        let state = read_candidates(
            &[blocked.clone(), fallback.clone()],
            false,
            Some(&fallback),
            None,
        );

        // The lower-priority file wins, and the skipped one is still reported.
        assert!(state.present);
        assert_eq!(state.path, fallback.display().to_string());
        assert_eq!(state.tokens.accent, hex("#7aa2f7"));
        assert_eq!(
            codes(&state.diagnostics),
            vec![ThemeFileDiagnosticCode::Unreadable]
        );
        assert!(
            messages(&state.diagnostics).contains("not a regular file"),
            "{}",
            messages(&state.diagnostics)
        );
    }

    /// A dangling symlink, target missing, counts as absent like a directory,
    /// but is reported, since silence would look like "nothing has ever been
    /// here" and a tool's symlink pointing at nothing is worth one `warn!`.
    #[test]
    #[cfg(unix)]
    fn a_dangling_symlink_is_skipped_with_a_warning() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let dangling = dir.path().join("dangling");
        std::os::unix::fs::symlink(dir.path().join("does-not-exist"), &dangling)
            .expect("the temp dir supports symlinks");
        let fallback = write(dir.path(), THEME_FILE_NAME, EXAMPLE_CUSTOM);

        let state = read_candidates(
            &[dangling.clone(), fallback.clone()],
            false,
            Some(&fallback),
            None,
        );

        // The lower-priority file still wins, and the dangling link is
        // reported rather than silently skipped like plain absence.
        assert!(state.present);
        assert_eq!(state.path, fallback.display().to_string());
        assert_eq!(state.tokens.accent, hex("#7aa2f7"));
        assert_eq!(
            codes(&state.diagnostics),
            vec![ThemeFileDiagnosticCode::Unreadable]
        );
        assert!(
            messages(&state.diagnostics).contains("symlink"),
            "{}",
            messages(&state.diagnostics)
        );

        // A path with no symlink and no file stays silent, as before.
        let nothing = dir.path().join("never-existed");
        let quiet = read_candidates(
            &[nothing.clone(), fallback.clone()],
            false,
            Some(&fallback),
            None,
        );
        assert!(quiet.diagnostics.is_empty());
    }

    /// The payload is capped so a hostile document cannot push an unbounded
    /// list into the event. The log still gets every line, and
    /// `diagnostics_total` lets the tab say "…and N more" instead of losing
    /// count.
    #[test]
    fn the_diagnostics_payload_is_capped() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = write(
            dir.path(),
            THEME_FILE_NAME,
            r##"{
              "accent": 1,
              "surface": 2,
              "text": 3,
              "material": 4,
              "size_scale": "x",
              "radius": "x",
              "padding": "x"
            }"##,
        );

        let state = read_candidates(std::slice::from_ref(&path), false, Some(&path), None);

        assert!(state.present);
        assert_eq!(state.tokens, OverlayTheme::default());
        // Seven problems in the document, five of them in the capped payload.
        assert_eq!(state.diagnostics.len(), MAX_DIAGNOSTICS);
        assert_eq!(state.diagnostics_total, 7);
    }

    /// The absent and stale paths through
    /// [`log_truncate_and_attach_diagnostics`] also carry a correct
    /// `diagnostics_total`, which the test above checks only on the happy
    /// path. An env-exclusive miss reports its one diagnostic, and a retained
    /// document counts the retention's cause.
    #[test]
    fn diagnostics_total_is_correct_off_the_happy_path() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = write(dir.path(), THEME_FILE_NAME, EXAMPLE_CUSTOM);
        let candidates = [path.clone()];
        let good = read_candidates(&candidates, false, Some(&path), None);
        assert_eq!(good.diagnostics_total, 0);

        write(dir.path(), THEME_FILE_NAME, r##"{"version":1,"acc"##);
        let broken = read_candidates(&candidates, false, Some(&path), Some(&good));
        assert_eq!(broken.diagnostics_total, 1);

        let missing = dir.path().join("absent.json");
        let state = read_candidates(std::slice::from_ref(&missing), true, Some(&missing), None);
        assert_eq!(state.diagnostics_total, 1);

        assert_eq!(ThemeFileState::absent_at("").diagnostics_total, 0);
    }
}
