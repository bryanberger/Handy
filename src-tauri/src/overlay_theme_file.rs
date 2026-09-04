//! The theme file `overlay_theme.json`, a read-only input from outside Handy.
//!
//! A theme file lets an external theming tool drive the overlay without the
//! settings window. Handy only ever reads it. Nothing here writes, moves or
//! rewrites the file; the one thing this module creates is the folder the
//! Appearance tab's Open button offers, `~/.config/handy/`, which is Handy's
//! own documented location and never a path [`THEME_FILE_ENV_VAR`] named. And
//! nothing but typed tokens ever comes out of the document:
//! canonical `#rrggbb` colours, the closed enums `flat | glass`, the eight
//! macOS `glass_material` values and the two `glass_style` values, and numbers
//! rounded and clamped to the token contract's bounds. No CSS, stylesheet,
//! script, font, path, URL or command is ever read from this document, so a
//! hostile file can at worst cost the overlay its styling for one session.
//!
//! Two tiers of failure, mirroring `salvage_settings` one level up. A
//! document-level problem (unreadable, not UTF-8, malformed JSON, not an
//! object) keeps the last good document and marks it [`ThemeFileState::stale`],
//! while a key-level problem costs exactly that one key, which then inherits.
//! Deleting the file is how a tool says "stop overriding", so it clears the
//! cache instead of counting as a failure.
//!
//! The read is cheap. One `open`, whose handle serves both the metadata check
//! and a bounded sub-KiB read, so a candidate cannot be swapped out between
//! the two. It happens at launch, on every overlay show (off the main
//! thread), and whenever the Appearance tab asks. There is no file watcher.
//!
//! Forward compatibility, as promised to the tools that write this file.
//! Colour values are `"#RRGGBB"` strings today. A future schema version may
//! also accept `{ "light": "#RRGGBB", "dark": "#RRGGBB" }` for the same keys,
//! and the key names will not change. Writers that emit a single string stay
//! valid. Readers should tolerate either shape.

use crate::overlay_theme::{
    GlassMaterial, GlassStyle, HexColor, Material, OverlayTheme, ThemeFileDiagnostic,
    ThemeFileDiagnosticCode, ThemeFileState, BORDER_OPACITY_MAX, BORDER_OPACITY_MIN,
    BORDER_WIDTH_MAX, GLASS_TINT_MAX, GLASS_TINT_MIN, PADDING_MAX, RADIUS_MAX, SIZE_SCALE_MAX,
    SIZE_SCALE_MIN, SURFACE_OPACITY_MAX, SURFACE_OPACITY_MIN, WAVEFORM_GAP_MAX, WAVEFORM_WIDTH_MAX,
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

/// The schema version this build writes into its documentation and accepts
/// without comment. A missing `version` means this; a newer one is parsed
/// best-effort.
pub const CURRENT_OVERLAY_THEME_FILE_VERSION: u32 = 1;

/// The environment variable that names a theme file outright.
///
/// It takes a path rather than a flag, so this module reads it with
/// `std::env::var_os` rather than `utils::env_flag_enabled`. `std::env::var`
/// is no good either, because its `Result` collapses "unset" and "set to
/// something that is not valid Unicode" to the same `Err` behind a careless
/// `.ok()`. When it is set (to anything, including a value that cannot become
/// a path), it is the only candidate tried, because an explicit instruction
/// must never quietly resolve to a different file. See [`EnvOverride`].
pub const THEME_FILE_ENV_VAR: &str = "HANDY_OVERLAY_THEME_FILE";

/// The directory under the user's config home that theming tools write to.
/// Deliberately the bare product name rather than the bundle identifier,
/// because `~/.config/handy/` is what Discussion #1802 asked for.
const CONFIG_SUBDIR: &str = "handy";

/// The environment variable that moves the config home off `~/.config`.
///
/// Read on every platform rather than Linux only. The name is XDG's, but a
/// dotfile manager that sets it on macOS or Windows is exactly the setup
/// `~/.config/handy/` exists to serve, and honouring it there costs one
/// lookup. Per the XDG spec, empty means unset, and a relative value is
/// invalid and ignored.
const CONFIG_HOME_ENV_VAR: &str = "XDG_CONFIG_HOME";

/// Anything larger than this is not a sixteen-key document; refused unread.
const MAX_THEME_FILE_BYTES: u64 = 64 * 1024;

/// How many diagnostics ride along in [`ThemeFileState`]. Every diagnostic
/// reaches the log; this only bounds the payload, because unknown keys are
/// unbounded in a hostile document.
const MAX_DIAGNOSTICS: usize = 5;

/// One token as this reader sees it: the key it is written under, and how a
/// value for it becomes a field of [`OverlayTheme`].
struct TokenSpec {
    key: &'static str,
    /// Parse `value` into `tokens`, pushing a diagnostic for anything it had
    /// to reject or clamp, and report whether the key ended up owned, that is,
    /// whether the file actually set it. Reached through [`TokenSpec::parse`],
    /// which is what hands it the key.
    parser: fn(&str, &Value, &mut OverlayTheme, &mut Vec<ThemeFileDiagnostic>) -> bool,
}

impl TokenSpec {
    /// Read this row's value out of the document.
    ///
    /// The key its diagnostics are filed under is the row's own. The caller
    /// has no say in it, which is why it is not a parameter.
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

/// The sixteen tokens, in the token contract's order, which is also the order
/// the Appearance tab lists them in. This is the order
/// [`ThemeFileState::owned_keys`] and the per-key diagnostics come out in, so
/// the payload does not depend on how `serde_json` orders an object's keys.
///
/// One table rather than a key list beside a match on it. The key, its parser
/// and its bounds are one fact about a token, and splitting them made "a key in
/// one list and not the other" a thing a debug assertion had to catch at
/// runtime instead of a thing that cannot be written down.
const TOKENS: [TokenSpec; 16] = [
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
/// First candidate that resolves to a readable regular file wins, and its
/// document is the only one used. The locations are never merged. The list is
/// empty when [`THEME_FILE_ENV_VAR`] is set to a value that cannot become a
/// path, since there is nothing to look at; [`read`] reports that case
/// directly rather than through this list.
pub fn candidate_paths(app: &AppHandle) -> Vec<PathBuf> {
    match env_override() {
        EnvOverride::Invalid => Vec::new(),
        EnvOverride::Unset => candidates_from(Locations {
            env: None,
            portable_data: portable_data_dir().as_deref(),
            config_home: config_home(app).as_deref(),
            app_data: platform_app_data_dir(app).as_deref(),
        }),
        // `candidates_from` already returns the singleton list for `env`;
        // routed through it so "the env var is exclusive" has one
        // implementation.
        EnvOverride::Path(path) => candidates_from(Locations {
            env: Some(&path),
            ..Locations::default()
        }),
    }
}

/// What the Appearance tab's Open button should do with a theme file that is
/// not there yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevealTarget {
    /// Create this directory, parents and all, if it is missing, then open
    /// it. Only ever Handy's own documented location, `~/.config/handy/`.
    Create(PathBuf),
    /// Open this directory exactly as it is, because it already exists.
    /// Nothing is created on this branch.
    Open(PathBuf),
}

/// Where the Open button should land, from the real environment.
///
/// The impure half of the decision: the two lookups, then
/// [`location_to_reveal`], which is the rule itself.
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

/// The Open button's rule, pure over the two paths it turns on and a "is this
/// a directory" predicate, so every branch is testable without an
/// `AppHandle` or a temp tree.
///
/// Handy creates a directory only under its own documented location, the
/// `~/.config/handy/` the tab printed and told the user to create. A path from
/// [`THEME_FILE_ENV_VAR`] belongs to whoever set the variable — a Nix store
/// path, a volume that is not mounted, a typo — and building a tree there
/// would be Handy writing into somewhere it was only ever told to read from.
/// So an env-named path opens the nearest folder that already exists, and when
/// not even the root of it does, the error says which variable is at fault
/// rather than silently opening somewhere else.
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
/// Reads only: it walks the ancestors and asks, and creates nothing. `None`
/// when nothing along the path is a directory, which includes a relative path
/// whose ancestors run out at `""`.
fn nearest_existing(directory: &Path, is_directory: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    directory
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .find(|ancestor| is_directory(ancestor))
        .map(Path::to_path_buf)
}

/// Create `dir` and any missing parent, so a path Handy has only ever printed
/// becomes a folder the user can drop a file into.
///
/// The one place in this module that writes to the filesystem, and it writes a
/// directory, never the theme file itself: Handy still never creates, rewrites
/// or deletes `overlay_theme.json`. Reached only for [`RevealTarget::Create`],
/// so the directory is always Handy's own `~/.config/handy/`. An existing
/// directory is success.
pub fn ensure_location(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("Cannot create {}: {error}", dir.display()))
}

/// The directory holding `path`, when it names one. A bare file name has a
/// parent of `""`, which is not a directory anyone can open, so it is `None`
/// rather than the process's working directory.
fn containing_directory(path: &Path) -> Option<PathBuf> {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

/// Read the theme file, update the cache, and return what it contributes.
///
/// Never panics, never writes, and never falls back to a different file than
/// the one [`THEME_FILE_ENV_VAR`] named. Does filesystem IO, so it belongs off
/// the main thread everywhere except the one launch-time call that warms the
/// cache before any window exists.
pub fn read(app: &AppHandle) -> ThemeFileState {
    let state = match env_override() {
        // The variable is set but cannot become a path at all, so there is
        // nothing to search, and `THEME_FILE_ENV_VAR`'s contract says nothing
        // else is tried either. This is a document-level diagnostic naming the
        // variable, not a per-candidate one, because no candidate list was
        // ever built.
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
            // not. It is exclusive, so it is also the file the user must
            // create when it is missing.
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
/// The cold path only exists before the launch-time [`read`], so in a running
/// app this is a lock and a clone, which is what lets the resolver run on the
/// main thread.
pub fn cached(app: &AppHandle) -> ThemeFileState {
    match cached_state() {
        Some(state) => state,
        None => read(app),
    }
}

/// The directories a candidate list is built from, with the lookups already
/// done. Every field is a directory rather than a file path, except `env`,
/// which names a file outright.
///
/// A struct rather than four positional `Option<&Path>` arguments, because
/// four same-typed parameters make swapping two of them a silent priority
/// change that still compiles.
#[derive(Default)]
struct Locations<'a> {
    /// [`THEME_FILE_ENV_VAR`]'s target, a file. Exclusive.
    env: Option<&'a Path>,
    /// `<exe dir>/Data`, present only for a portable install.
    portable_data: Option<&'a Path>,
    /// `$XDG_CONFIG_HOME`, or `~/.config`. The `handy/` component is joined
    /// here.
    config_home: Option<&'a Path>,
    /// The OS app data directory, `<data dir>/com.pais.handy`.
    app_data: Option<&'a Path>,
}

/// The candidate list, in priority order.
///
/// Pure, so the order and the env var's exclusivity are testable without an
/// `AppHandle`. The `handy/` component and the file name are joined here, so
/// the callers only have to find directories.
fn candidates_from(locations: Locations) -> Vec<PathBuf> {
    // The env var is exclusive. An explicit path is the whole list, and a
    // missing target is a warning rather than a silent fallback to a file the
    // user did not name.
    if let Some(path) = locations.env {
        return vec![path.to_path_buf()];
    }

    let mut candidates = Vec::with_capacity(3);
    // A portable install's promise is that everything it needs sits beside the
    // executable, so its own `Data/` outranks anything in the user's home.
    if let Some(dir) = locations.portable_data {
        candidates.push(dir.join(THEME_FILE_NAME));
    }
    // The one location documented on every platform, and the one the tab
    // prints when no file exists.
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
    /// Not set, or set to an empty string. The other candidates are tried
    /// normally.
    Unset,
    /// Set, to a path.
    Path(PathBuf),
    /// Set to a value that is not valid Unicode. `std::env::var_os` still
    /// returns it, since environment variables are just bytes on every
    /// platform Handy ships for, but this module only ever builds `Path`s
    /// from `str`, so there is no path here to try, join or display.
    /// Exclusive like `Path`, so the search stops with a diagnostic rather
    /// than silently falling back to app data or XDG.
    Invalid,
}

/// The theme file named by [`THEME_FILE_ENV_VAR`], read from the real process
/// environment.
fn env_override() -> EnvOverride {
    env_candidate_os(std::env::var_os(THEME_FILE_ENV_VAR).as_deref())
}

/// [`env_override`]'s logic, pure over `std::env::var_os`'s result so the
/// non-Unicode branch is testable without touching a real environment
/// variable. That branch cannot be expressed as `Option<&str>`.
fn env_candidate_os(value: Option<&OsStr>) -> EnvOverride {
    match value {
        None => EnvOverride::Unset,
        Some(value) if value.is_empty() => EnvOverride::Unset,
        Some(value) => match value.to_str() {
            // Delegates to `env_candidate` rather than building the `PathBuf`
            // here directly, so "what counts as a usable path" is defined in
            // exactly one place. `text` is already known non-empty, so this
            // always lands in `Path`; the fallback is defensive, not reachable.
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
/// Deliberately not [`crate::portable::app_data_dir`], which stands in for the
/// OS directory when the marker is present. Here the two are separate
/// candidates at different priorities, so a portable install reads its own
/// `Data/` first and the OS app data directory last.
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
/// Not Tauri's `config_dir()`, which is only `~/.config` on Linux and returns
/// `~/Library/Application Support` and `%APPDATA%` on macOS and Windows. The
/// point of this candidate is that one documented path, `~/.config/handy/`,
/// works everywhere, so the home directory is what it is built from:
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

/// [`config_home`]'s rule, pure over its two inputs so both branches are
/// testable with an injected home and an injected environment value.
///
/// XDG's own rules for the variable: empty means unset, and a relative value
/// is invalid and ignored. Ignoring a relative one matters here because
/// joining it would resolve against Handy's working directory, which is
/// wherever the launcher happened to start the app.
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
/// A missing home directory is only a `debug!` in [`config_home`], because
/// `$XDG_CONFIG_HOME` can still answer without one. Coming out of both with
/// nothing is worth a warning: the location Handy documents, the one the
/// README and the Appearance tab name, does not exist for this process, so
/// the tab has to name the app data directory instead.
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
/// Ordinarily the documented `~/.config/handy/overlay_theme.json`, so the tab
/// tells the user where to create a file, rather than the app data directory,
/// which stays readable but is now only the fallback for files already there.
/// Without a home directory there is no such path, and the app data candidate
/// stands in: it is still somewhere real, and a tab showing an empty path
/// tells the user nothing at all.
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
/// Opens the file exactly once and takes the metadata and the bytes from that
/// one handle, rather than an `fs::metadata` call followed by a separate
/// `fs::read`. Two calls check one file and then read whatever the path
/// resolves to a moment later, so a candidate swapped out from under the
/// search between the two (a non-atomic replace, or a symlink retargeted)
/// could dodge the type check or the size cap. A single handle keeps every
/// check and the read itself pinned to the same file. On a POSIX system, an
/// open file descriptor keeps referring to the original file even if the path
/// is unlinked or replaced out from under it. [`Read::take`] at one byte over
/// [`MAX_THEME_FILE_BYTES`] is a second, independent cap on top of the
/// metadata check, so even a file whose reported size does not match what it
/// actually yields can never land more than one byte over the limit in
/// memory.
///
/// Opening follows symlinks, so the symlink-to-a-real-file that a Nix or
/// Home-Manager setup needs still works, while the same check that rejects a
/// directory or a device directly also rejects a symlink to one. A symlink
/// whose target does not exist is handled separately, by
/// [`dangling_symlink_or_absent`].
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
    // Belt-and-braces against a file that grew after the metadata check above
    // but before this read finished. `Read::take` stops at one byte over the
    // limit no matter what `metadata.len()` said, so a swapped-in larger file
    // can never reach `String::from_utf8` at all.
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
/// Called only after `File::open` has already failed with `NotFound`, which a
/// dangling symlink also produces (opening follows the link, and the target
/// is not there). `symlink_metadata` does not follow links, so it succeeds
/// even when the target is missing, which is what lets the two cases be told
/// apart. A dangling symlink counts as absent, the same as a directory, and
/// like a directory it is worth one `warn!` rather than the silent not-found
/// path. A theming tool's symlink pointing at nothing is a misconfiguration
/// rather than the ordinary "no file here yet".
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
    // Warnings collected while skipping candidates. They survive into whatever
    // state the search ends in. A directory sitting where a theme file belongs
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
    // overriding, so this clears the tokens rather than keeping the last good
    // document. It is the one failure mode that is not a failure.
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

/// A document that failed to parse or to be read keeps the previous one, so a
/// tool writing the file non-atomically cannot snap the overlay back to the
/// settings theme for one dictation. On the first read of a process there is
/// no previous document, so the tokens simply stay empty.
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

/// Log every diagnostic, then truncate to [`MAX_DIAGNOSTICS`] and attach both
/// the capped list and the pre-cap count to the state.
///
/// The log gets every diagnostic; [`ThemeFileState::diagnostics`] gets at most
/// [`MAX_DIAGNOSTICS`] of them; [`ThemeFileState::diagnostics_total`] is the
/// count before capping, which is what lets the tab say "…and N more" for the
/// rest instead of just "more".
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
    // PowerShell's `>` and several Windows editors emit a BOM, which
    // `serde_json` rejects outright.
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

    // Reported as one line naming every offender, because a typo and a key from
    // a newer schema are indistinguishable here and a silently ignored typo is
    // this feature's most likely failure. `key` carries the list so the tab can
    // pass it straight into a translated message.
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

    // Fixed contract order, not the document's. `serde_json` sorts an object's
    // keys unless the `preserve_order` feature is on, so document order is not
    // a thing this reader could honour even if it wanted to.
    for token in &TOKENS {
        let Some(value) = object.get(token.key) else {
            continue;
        };
        // Explicit null is the spelling of inherit, not a value to complain
        // about.
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
/// contract rather than in the number. Unknown keys and unparseable values
/// already cost only themselves, so a v1 build applies what it understands
/// instead of blanking the theme.
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
/// silently dropping the alpha would misapply the user's intent, and the two
/// alpha tokens are where that intent belongs.
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
/// case-insensitively and ignoring `-`/` ` so `"HUD Window"`, `"hud-window"`
/// and `"hud_window"` all land on the same value. This document is written by
/// hand and by third-party tools.
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
/// case-insensitively and ignoring `-`/` ` like [`parse_glass_material`], so a
/// document written by hand or by a third-party tool spells them however it
/// likes.
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
/// `size_scale`), read as a JSON number and clamped to the contract's bounds
/// with a diagnostic when it had to move.
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
/// `waveform_width`), read as a JSON number, rounded half away from zero,
/// then clamped to `min..=max`. A float is accepted, a numeric string is not.
///
/// `min` is 0 for every token but `waveform_width`, whose bars disappear
/// below 2 px.
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

/// A JSON string, or a `WrongType` diagnostic. Numbers, booleans, objects and
/// arrays are all type errors. Leniency is confined to how a colour or an
/// enum is spelled, never to what JSON type carries it.
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

    /// Serializes every test that touches the real `HANDY_OVERLAY_THEME_FILE`
    /// process environment variable, so two such tests can never interleave
    /// under the test harness's default multi-threaded runner and leave one
    /// another's value behind. Almost every test in this module instead goes
    /// through the pure `env_candidate` / `env_candidate_os` seams, which
    /// touch no real environment variable and need no lock at all. Reach for
    /// this only when a test must exercise `env_override` itself.
    static ENV_VAR_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Sets [`THEME_FILE_ENV_VAR`] for the life of the guard and restores its
    /// prior value on drop, including on panic, since a `Drop` impl still runs
    /// while a test's assertion failure unwinds. A failing test therefore
    /// cannot leak a mutated environment variable into whatever test the
    /// harness runs next in this process. Holds [`ENV_VAR_TEST_LOCK`] for the
    /// same reason.
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

    /// The contract's inherit-everything example, byte-identical. Frozen, so
    /// if a schema change breaks it, add tolerance rather than editing the
    /// fixture.
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
  "size_scale": null,
  "radius": null,
  "border_width": null,
  "padding": null,
  "waveform_gap": null,
  "waveform_width": null
}"##;

    /// The contract's fully custom theme, byte-identical to the token
    /// contract's worked example.
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
  "size_scale": 1.1,
  "radius": 12,
  "border_width": 1,
  "padding": 14,
  "waveform_gap": 2,
  "waveform_width": 4
}"##;

    /// The contract's theming-tool document, with every leniency at once plus
    /// a comment key and a key from a schema this build does not have.
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

    /// `~/.config/handy/` is the location Handy documents, so it outranks the
    /// app data directory, which stays on the list only so a file written
    /// where older builds pointed still drives the overlay. A portable
    /// install's own `Data/` outranks both: everything it needs sits beside
    /// the executable.
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

        // The ordinary install: no portable marker, so two candidates, with
        // `~/.config/handy/` first.
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
    /// lookups return. `~/.config/handy/` is not Linux-only any more, and it
    /// is never Tauri's `config_dir()`, which is the app data directory again
    /// on macOS and Windows.
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
            // separator of whatever host runs the test: a literal
            // `C:\Users\user\.config\...` passes on Windows and fails on
            // macOS, and the components are what this test is about anyway.
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

    /// `$XDG_CONFIG_HOME` moves the config location, on every platform.
    /// XDG's own rules for the variable: empty is unset, and a relative value
    /// is invalid and ignored rather than resolved against whatever directory
    /// the launcher started Handy in.
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
        // A set variable still answers when there is no home directory at all;
        // without one there is nothing to fall back to.
        assert_eq!(
            config_home_from(Some(OsStr::new("/etc/xdg")), None),
            Some(PathBuf::from("/etc/xdg"))
        );
        assert_eq!(config_home_from(None, None), None);
    }

    /// First found wins, and the app data file is only reached when
    /// `~/.config/handy/` has none. Two real files, one in each location.
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

    /// With no file anywhere, the reported path is the one the tab tells the
    /// user to create: `~/.config/handy/overlay_theme.json`, never the app
    /// data directory it used to name.
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

    /// Without a home directory there is no `~/.config/handy/` to name, and
    /// the tab has to say something: the app data candidate, which is still a
    /// real location, rather than an empty path that tells the user nothing.
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
        // With a home, the documented location wins, as it does everywhere
        // else in this module.
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

    /// Where the Open button lands, and, the point of the branch, where it is
    /// not allowed to create anything. Pure: an injected env path, an
    /// injected config path and an injected notion of which directories
    /// exist.
    #[test]
    fn revealing_creates_only_under_the_config_location() {
        let existing = ["/Volumes", "/Volumes/backup"];
        let is_directory = |dir: &Path| existing.iter().any(|known| Path::new(known) == dir);
        let config_file = Path::new("/Users/user/.config/handy/overlay_theme.json");

        // No override: the documented folder, created if it is missing. The
        // one branch that may create anything, and it does not consult the
        // filesystem at all, since the whole point is a folder that is not
        // there yet.
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
        // whose external drive holds no `themes/` yet still lands next to it,
        // and Handy has still written nothing under a path it was only told
        // to read.
        assert_eq!(
            location_to_reveal(
                Some(Path::new("/Volumes/backup/themes/overlay_theme.json")),
                Some(config_file),
                is_directory
            ),
            Ok(RevealTarget::Open(PathBuf::from("/Volumes/backup")))
        );

        // Nothing along the path exists: an error naming the variable, never
        // the config folder as a consolation prize, which would open
        // somewhere the user did not ask about.
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

        // A bare file name has no folder at all, and must not resolve against
        // the process's working directory.
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
    /// path the tab has only ever printed becomes somewhere to drop a file.
    /// Creating an existing directory is success, and the theme file itself is
    /// never written.
    #[test]
    fn revealing_an_absent_location_creates_the_directory() {
        let root = tempfile::tempdir().expect("a temp dir");
        let handy_config = root.path().join(".config").join(CONFIG_SUBDIR);

        // The directory the Open button is handed, derived the same way the
        // command derives it: the folder of the reported path.
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

        // A bare file name has no directory to open, and must not resolve
        // against the process's working directory.
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

        // The env var names a path verbatim, not a directory to join the file
        // name onto.
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

        // The whole read against a file only the env var knows about, with a
        // name that is not `overlay_theme.json`, in a directory that is
        // neither the app data nor the XDG one. This is the Nix/Home-Manager
        // case, and the one path that cannot be checked on screen (the dev app
        // cannot be started with an extra variable through the test harness).
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

        // …and the variable is what feeds that candidate, checked through the
        // real `std::env::var_os` call, guarded so this cannot race or leak
        // into any other test touching the same variable.
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

    /// [`env_candidate_os`] is the seam that tells "unset" apart from "set to
    /// something that cannot become a path", which `std::env::var`'s `Result`
    /// cannot. A `VarError::NotUnicode` collapsing to `None` behind an `.ok()`
    /// is exactly the bug this fixes (it used to fall back to app data / XDG
    /// instead of stopping the search). Entirely pure, touching no real
    /// environment variable, so this needs no lock.
    #[test]
    fn env_candidate_os_tells_unset_a_path_and_invalid_unicode_apart() {
        assert_eq!(env_candidate_os(None), EnvOverride::Unset);
        assert_eq!(env_candidate_os(Some(OsStr::new(""))), EnvOverride::Unset);
        assert_eq!(
            env_candidate_os(Some(OsStr::new("/tmp/t.json"))),
            EnvOverride::Path(PathBuf::from("/tmp/t.json"))
        );

        // Bytes that are not valid UTF-8 cannot be turned into a `&str`, and
        // therefore not into a `Path` either. `env_candidate_os` reports this
        // as `Invalid` rather than quietly treating it as unset.
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;

            let invalid = OsStr::from_bytes(&[0x66, 0xff, 0x66]);
            assert!(invalid.to_str().is_none(), "fixture must be non-Unicode");
            assert_eq!(env_candidate_os(Some(invalid)), EnvOverride::Invalid);
        }
    }

    /// [`env_override`] is a thin wrapper over `std::env::var_os`; this pins
    /// that it actually reaches the real environment, including for a value
    /// that is not valid Unicode. That is the one branch [`EnvVarGuard`]
    /// exists for, since `std::env::set_var` is how a real non-Unicode value
    /// gets into the process at all. Guarded and serialized so it cannot race
    /// or leak into any other test touching the same variable.
    #[test]
    #[cfg(unix)]
    fn env_override_reports_invalid_for_a_real_non_unicode_value() {
        use std::os::unix::ffi::OsStrExt;

        let invalid = OsStr::from_bytes(&[0x66, 0xff, 0x66]);
        let _guard = EnvVarGuard::set(invalid);

        assert_eq!(env_override(), EnvOverride::Invalid);
    }

    /// The inherit-everything document is the correct spelling of "today's
    /// overlay". It must contribute nothing at all, or the defaults stop
    /// reproducing today's look.
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
                size_scale: Some(1.1),
                radius: Some(12),
                border_width: Some(1),
                padding: Some(14),
                waveform_gap: Some(2),
                waveform_width: Some(4),
            }
        );
        // Every key is owned, so the tab locks all sixteen.
        assert_eq!(
            parsed.owned_keys,
            TOKENS.iter().map(|token| token.key).collect::<Vec<_>>()
        );
        assert!(parsed.diagnostics.is_empty());
    }

    /// What a theming tool plausibly emits: 3-digit shorthand, a missing `#`,
    /// uppercase, an integer where a float is expected, a comment key, and a
    /// key from a newer schema.
    #[test]
    fn the_theming_tool_example_exercises_every_leniency() {
        let parsed = parse(EXAMPLE_THEMING_TOOL);

        assert_eq!(parsed.tokens.accent, hex("#8aadf4"));
        assert_eq!(parsed.tokens.surface, hex("#24273a"));
        assert_eq!(parsed.tokens.text, hex("#ccaadd"));
        assert_eq!(parsed.tokens.surface_opacity, Some(1.0));
        assert_eq!(parsed.tokens.material, Some(Material::Flat));

        // The eleven tokens it does not mention still inherit.
        assert_eq!(parsed.tokens.glass_tint, None);
        assert_eq!(parsed.tokens.border, None);
        assert_eq!(parsed.tokens.border_opacity, None);
        assert_eq!(parsed.tokens.glass_material, None);
        assert_eq!(parsed.tokens.glass_style, None);
        assert_eq!(parsed.tokens.size_scale, None);
        assert_eq!(parsed.tokens.radius, None);
        assert_eq!(parsed.tokens.border_width, None);
        assert_eq!(parsed.tokens.padding, None);
        assert_eq!(parsed.tokens.waveform_gap, None);
        assert_eq!(parsed.tokens.waveform_width, None);

        assert_eq!(
            parsed.owned_keys,
            vec!["accent", "surface", "surface_opacity", "text", "material"]
        );

        // Both non-token keys are reported in one line, so a typo cannot be
        // mistaken for a comment.
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

        // With no previous document there is nothing to keep, so the file
        // simply contributes nothing.
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
        // The path still points at where a file would go, so the tab can say
        // where to create one.
        assert_eq!(cleared.path, path.display().to_string());

        store_cache(&cleared);
        assert_eq!(
            cached_state().map(|state| state.tokens),
            Some(OverlayTheme::default())
        );
    }

    /// The reader clamps a number outside the bounds rather than dropping it,
    /// so the user's intent (bigger, rounder) survives and the overlay cannot
    /// be sized to cover the screen.
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

        // In-bounds values are silent, and a float px value rounds half away
        // from zero before it is judged.
        let quiet = parse(r##"{ "size_scale": 1.13, "radius": 12.5, "surface_opacity": 0.3 }"##);
        assert_eq!(quiet.tokens.size_scale, Some(1.13));
        assert_eq!(quiet.tokens.radius, Some(13));
        assert_eq!(quiet.tokens.surface_opacity, Some(0.3));
        assert!(quiet.diagnostics.is_empty());
    }

    /// The Glass tint, with its bounds spelled out from the token table
    /// rather than read from the module's constants. Its floor is zero, where
    /// `surface_opacity`'s is 0.30. That is the one place the two alpha
    /// tokens' contracts visibly differ, and the reason a document can ask for
    /// untinted glass but not for an invisible Flat card.
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

    /// The border and waveform tokens, with the bounds spelled out from the
    /// token table rather than read from the module's constants.
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

    /// `glass_material` is spelled the way a human would write it by hand, so
    /// the reader accepts the separators a human reaches for.
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

    /// `glass_style` is spelled as leniently as `glass_material` and just as
    /// closed, so a document that names a style this build does not have
    /// loses that one key rather than the whole file.
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

        // When `version` is not a positive integer, the reader ignores the
        // field and reads the document as version 1.
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

    /// Anything that is not a small regular file is skipped, and the search
    /// carries on to the next candidate.
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

    /// A dangling symlink (its target does not exist) counts as absent, the
    /// same as a directory, but it is reported rather than silent. Silence
    /// would make it indistinguishable from "nothing has ever been here", and
    /// a theming tool's symlink pointing at nothing is worth the one `warn!`.
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

        // A symlink is not the only way to reach "nothing here at all". A
        // path with no symlink and no file either stays silent, as before.
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
    /// list into the event; the log still gets every line, and
    /// `diagnostics_total` is what lets the tab say "…and N more" instead of
    /// losing count of what it could not show.
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

    /// The absent and stale paths through [`log_truncate_and_attach_diagnostics`]
    /// also carry a correct `diagnostics_total`, which the test above only
    /// checks on the happy path. An env-exclusive miss reports exactly the one
    /// diagnostic it logs, and a retained last-good document counts the
    /// failure that caused the retention.
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
