//! Overlay theme: storage, per-key merge, and delivery.
//!
//! An *overlay theme* is the set of nine optional tokens that decide how the
//! recording overlay's card looks. Every token is optional and absent means
//! *inherit* — the overlay uses Handy's built-in, theme-aware value — so a
//! theme that sets nothing reproduces today's overlay exactly.
//!
//! The *resolved overlay theme* is what both webviews actually read: the
//! tokens after the per-key merge `theme file ?? settings ?? inherit`, clamped,
//! together with the Material actually rendered, whether Glass is available,
//! and the theme file's state. It is the return type of
//! [`crate::commands::overlay_theme::get_resolved_overlay_theme`] *and* the
//! payload of the `resolved-overlay-theme` event, so the pull and the push can
//! never diverge.
//!
//! All px tokens are expressed at `size_scale` = 1; the scale multiplies them.

use log::{debug, warn};
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use tauri::AppHandle;

/// A canonical `#rrggbb` colour.
///
/// Parsing is lenient — `#RGB` shorthand, a missing `#`, any case, surrounding
/// whitespace — and always yields lowercase `#rrggbb`; 4- and 8-digit forms
/// (which would carry alpha) and CSS colour names are rejected. This is the
/// only string that ever reaches a CSS custom property, and it is re-serialised
/// from this type rather than echoed, so no value from a settings store or a
/// theme file is ever passed through verbatim.
#[derive(Clone, Debug, PartialEq, Eq, Type)]
pub struct HexColor(String);

impl HexColor {
    /// The one lenient colour parser, shared by the settings store and the
    /// theme file. `None` means the value is not a colour this contract
    /// accepts; the caller decides whether that is a dropped token or a
    /// diagnostic.
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
        if !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
            return None;
        }

        let expanded = match digits.len() {
            // `#RGB` expands by digit doubling, exactly as CSS does.
            3 => digits.chars().flat_map(|digit| [digit, digit]).collect(),
            6 => digits.to_string(),
            // Anything else is either alpha (4 or 8 digits), which belongs to
            // `surface_opacity`, or not a colour at all.
            _ => return None,
        };

        Some(HexColor(format!("#{}", expanded.to_ascii_lowercase())))
    }

    /// The canonical value, always `#rrggbb`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for HexColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        HexColor::parse(&raw).ok_or_else(|| {
            serde::de::Error::custom(format!("expected a #rrggbb colour, got {raw:?}"))
        })
    }
}

/// How the overlay surface is rendered: Flat (opaque) or Glass (translucent,
/// blurring whatever is behind it).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "lowercase")]
pub enum Material {
    /// An opaque surface. The only Material outside macOS, and the fallback
    /// whenever Glass cannot render.
    #[default]
    Flat,
    /// A translucent surface backed by a native blur of whatever is behind the
    /// overlay window. macOS only.
    Glass,
}

/// Lowest accepted [`OverlayTheme::size_scale`].
pub const SIZE_SCALE_MIN: f64 = 0.80;
/// Highest accepted [`OverlayTheme::size_scale`].
pub const SIZE_SCALE_MAX: f64 = 1.50;
/// Lowest accepted `surface_opacity`.
pub const SURFACE_OPACITY_MIN: f64 = 0.30;
/// Highest accepted `surface_opacity`.
pub const SURFACE_OPACITY_MAX: f64 = 1.00;
/// Highest accepted `radius`, in px at scale 1.
pub const RADIUS_MAX: u16 = 32;
/// Highest accepted `padding`, in px at scale 1.
pub const PADDING_MAX: u16 = 20;
/// Highest accepted `waveform_gap`, in px at scale 1.
pub const WAVEFORM_GAP_MAX: u16 = 5;

/// A token whose stored value does not parse becomes `None` — inherit —
/// instead of failing the whole [`OverlayTheme`], which would make
/// `salvage_settings` drop the `overlay_theme` key and reset all nine tokens.
///
/// Safe because `AppSettings` is only ever deserialized from a
/// `serde_json::Value`, which is self-describing.
fn inherit_on_error<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }

    match serde_json::from_value::<T>(value) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(error) => {
            warn!("Dropping invalid overlay theme token ({error}); inheriting");
            Ok(None)
        }
    }
}

/// The nine overlay-theme tokens. `None` means *inherit*.
///
/// Field names are literally the theme-file keys. Every field deserializes
/// leniently: a value of the wrong type or shape degrades to `None` with a
/// `warn!`, so one bad token can never cost the other eight — the same
/// principle `salvage_settings` applies one level up. The settings store's
/// leniency is silent salvage (log only); the theme file applies the same rules
/// but reports diagnostics, which is why it runs its own per-key pass instead
/// of deserializing an `OverlayTheme` directly.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default, Type)]
#[serde(default)]
pub struct OverlayTheme {
    /// Highlight colour: waveform bars, recording dot, caret, spinner arc.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub accent: Option<HexColor>,
    /// The card's background colour.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub surface: Option<HexColor>,
    /// The card background's alpha, 0.30–1.00.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub surface_opacity: Option<f64>,
    /// The card's foreground colour, and the base every neutral derives from.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub text: Option<HexColor>,
    /// Flat or Glass. See [`effective_material`] for what is actually rendered.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub material: Option<Material>,
    /// One factor multiplying every length in the card, 0.80–1.50.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub size_scale: Option<f64>,
    /// The card's corner radius at scale 1, 0–32 px.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub radius: Option<u16>,
    /// The card's inner horizontal padding at scale 1, 0–20 px.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub padding: Option<u16>,
    /// Gap between waveform bars at scale 1, 0–5 px.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub waveform_gap: Option<u16>,
}

impl OverlayTheme {
    /// The persisted size scale: unset ⇒ 1.0, non-finite ⇒ 1.0, otherwise
    /// clamped to [`SIZE_SCALE_MIN`]–[`SIZE_SCALE_MAX`].
    ///
    /// **The single clamp**, shared by the native window geometry, the theme
    /// file and the apply layer, so nothing can clamp differently.
    pub fn size_scale(&self) -> f64 {
        match self.size_scale {
            Some(scale) if scale.is_finite() => scale.clamp(SIZE_SCALE_MIN, SIZE_SCALE_MAX),
            _ => 1.0,
        }
    }

    /// The requested Material: unset ⇒ Flat. Not the effective one — see
    /// [`effective_material`].
    pub fn material(&self) -> Material {
        self.material.unwrap_or_default()
    }

    /// A copy with every token clamped to this module's bounds.
    ///
    /// Applied before persisting and again after merging, so no out-of-range
    /// value can reach the native geometry or the frontend. Non-finite floats
    /// become `None` (inherit) rather than a clamped number: they cannot be
    /// serialized to JSON, so dropping them here is also what keeps the
    /// settings store writable.
    pub fn normalized(&self) -> OverlayTheme {
        OverlayTheme {
            accent: self.accent.clone(),
            surface: self.surface.clone(),
            surface_opacity: clamp_float(
                self.surface_opacity,
                SURFACE_OPACITY_MIN,
                SURFACE_OPACITY_MAX,
                "surface_opacity",
            ),
            text: self.text.clone(),
            material: self.material,
            size_scale: clamp_float(
                self.size_scale,
                SIZE_SCALE_MIN,
                SIZE_SCALE_MAX,
                "size_scale",
            ),
            radius: self.radius.map(|value| value.min(RADIUS_MAX)),
            padding: self.padding.map(|value| value.min(PADDING_MAX)),
            waveform_gap: self.waveform_gap.map(|value| value.min(WAVEFORM_GAP_MAX)),
        }
    }
}

/// Clamp an optional float token, dropping a non-finite value to inherit.
fn clamp_float(value: Option<f64>, min: f64, max: f64, key: &str) -> Option<f64> {
    match value {
        Some(value) if value.is_finite() => Some(value.clamp(min, max)),
        Some(_) => {
            warn!("Overlay theme token '{key}' is not a finite number; inheriting");
            None
        }
        None => None,
    }
}

/// Whether Glass can render.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
pub struct GlassSupport {
    /// The platform can render Glass at all. macOS only; a compile-time fact.
    /// Drives whether the Appearance tab's Glass option is selectable.
    pub supported: bool,
    /// Glass renders right now: `supported`, the effect view is installed, and
    /// macOS "Reduce transparency" is off. Drives what is actually painted.
    pub available: bool,
}

/// The Material actually in effect: the requested token, downgraded to Flat
/// whenever Glass cannot render.
pub fn effective_material(requested: Material, support: GlassSupport) -> Material {
    if requested == Material::Glass && support.available {
        Material::Glass
    } else {
        Material::Flat
    }
}

/// Whether Glass can render on this machine.
///
/// A stub until the native Glass module lands: `supported` is the compile-time
/// platform fact, `available` is false because no effect view is installed yet.
/// A persisted or file-driven `material: "glass"` therefore round-trips intact
/// and renders Flat.
pub fn glass_support(_app: &AppHandle) -> GlassSupport {
    GlassSupport {
        supported: cfg!(target_os = "macos"),
        available: false,
    }
}

/// What kind of thing the theme file got wrong.
///
/// A stable, translatable identity for a diagnostic: the Appearance tab looks
/// up an i18n string by code and passes [`ThemeFileDiagnostic::key`] as a
/// parameter, so the user reads their own language while
/// [`ThemeFileDiagnostic::message`] keeps the English detail for the log.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeFileDiagnosticCode {
    /// The document is not valid JSON, or not a JSON object.
    MalformedDocument,
    /// The document declares a `version` this build does not know; it is parsed
    /// best-effort.
    UnsupportedVersion,
    /// A top-level key that is not a token and not `version`; ignored.
    UnknownKey,
    /// A token whose value is not the JSON type the contract requires.
    WrongType,
    /// A colour that is not a `#rrggbb` value this contract accepts.
    InvalidColor,
    /// A number outside the token's bounds; clamped to the nearest bound.
    OutOfBounds,
    /// The file exists but could not be read (permissions, size, encoding).
    Unreadable,
}

/// One thing the theme file got wrong, reported to the Appearance tab.
///
/// `Deserialize` is required because this rides in the `resolved-overlay-theme`
/// event payload, and listening for an event deserializes it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct ThemeFileDiagnostic {
    /// What went wrong, as a stable identity the tab can translate.
    pub code: ThemeFileDiagnosticCode,
    /// The token key, or `None` for a document-level problem. Doubles as the
    /// parameter for the translated message.
    pub key: Option<String>,
    /// English, deliberately untranslated: it names JSON keys and values, and
    /// it is what goes to the log.
    pub message: String,
}

/// What the theme file currently contributes.
///
/// Populated by the theme-file reader; until that lands every resolve uses
/// [`ThemeFileState::absent`], so the payload shape — and therefore the
/// generated TypeScript bindings — is already final.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct ThemeFileState {
    /// The file in effect, or the path Handy would read if one appeared.
    pub path: String,
    /// Whether a theme file was actually found and read at [`Self::path`].
    pub present: bool,
    /// The document's declared `version`, or `None` when it is absent or the
    /// file is not present. A missing version means 1.
    pub version: Option<u32>,
    /// The file's contribution to the merge.
    pub tokens: OverlayTheme,
    /// The keys the file actually sets. These are the tab's lock markers: a
    /// file-owned token cannot be edited from the settings window.
    pub owned_keys: Vec<String>,
    /// Everything the reader had to ignore or clamp, in document order. The tab
    /// renders a capped list of these; all of them also go to the log.
    pub diagnostics: Vec<ThemeFileDiagnostic>,
    /// True when a failed read kept the previous, good document.
    pub stale: bool,
}

impl ThemeFileState {
    /// No theme file: contributes nothing, so the merge falls through to the
    /// settings and then to inherit.
    pub fn absent() -> Self {
        ThemeFileState {
            path: String::new(),
            present: false,
            version: None,
            tokens: OverlayTheme::default(),
            owned_keys: Vec::new(),
            diagnostics: Vec::new(),
            stale: false,
        }
    }
}

/// The whole answer to "how does the overlay look right now".
///
/// Command result **and** event payload, so the overlay's pull on show and the
/// push on change carry the identical type.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type, tauri_specta::Event)]
pub struct ResolvedOverlayTheme {
    /// `file ?? settings ?? inherit`, per key, clamped. `None` still means
    /// inherit: the apply layer writes no custom property for it.
    pub theme: OverlayTheme,
    /// Concrete, never `None`: the requested Material downgraded to Flat when
    /// Glass is unavailable.
    pub effective_material: Material,
    /// Whether Glass is offerable and whether it can render right now. Read by
    /// the Appearance tab instead of a platform check in TypeScript, so the two
    /// sides cannot disagree.
    pub glass_support: GlassSupport,
    /// What the theme file contributed, including which tokens it owns and what
    /// the reader had to ignore.
    pub file: ThemeFileState,
}

/// Merge two token sets per key: `file` wins wherever it sets a token, and an
/// absent token falls through to `settings` and then to inherit.
///
/// Pure, so the precedence rule is testable without an `AppHandle`.
pub fn merge(file: &OverlayTheme, settings: &OverlayTheme) -> OverlayTheme {
    OverlayTheme {
        accent: file.accent.clone().or_else(|| settings.accent.clone()),
        surface: file.surface.clone().or_else(|| settings.surface.clone()),
        surface_opacity: file.surface_opacity.or(settings.surface_opacity),
        text: file.text.clone().or_else(|| settings.text.clone()),
        material: file.material.or(settings.material),
        size_scale: file.size_scale.or(settings.size_scale),
        radius: file.radius.or(settings.radius),
        padding: file.padding.or(settings.padding),
        waveform_gap: file.waveform_gap.or(settings.waveform_gap),
    }
}

/// Merge `theme file ?? settings ?? inherit` per key and clamp.
///
/// Uses the theme-file cache — no filesystem IO — so it is safe on the main
/// thread. Until the theme file exists there is no cache to read and the file
/// contributes nothing.
pub fn resolve(app: &AppHandle) -> ResolvedOverlayTheme {
    resolve_with_file(app, ThemeFileState::absent())
}

/// [`resolve`], preceded by a fresh read of the theme file.
///
/// **Today this is byte-identical to [`resolve`]**: it is a placeholder seam so
/// the callers that must re-read (the overlay show path, the Reload button) are
/// already calling the right function. The theme-file slice adds the read here,
/// at which point this must only ever be called off the main thread.
pub fn resolve_reloading(app: &AppHandle) -> ResolvedOverlayTheme {
    resolve_with_file(app, ThemeFileState::absent())
}

fn resolve_with_file(app: &AppHandle, file: ThemeFileState) -> ResolvedOverlayTheme {
    let settings = crate::settings::get_settings(app);
    resolve_from(settings.overlay_theme, file, glass_support(app))
}

/// The whole resolution rule with nothing to look up: merge the file over the
/// settings, clamp once, and decide the Material actually rendered.
///
/// Pure, so the merge order, the clamping and the Glass downgrade are testable
/// together without an `AppHandle`.
pub fn resolve_from(
    settings_theme: OverlayTheme,
    file: ThemeFileState,
    support: GlassSupport,
) -> ResolvedOverlayTheme {
    let theme = merge(&file.tokens, &settings_theme).normalized();
    let effective_material = effective_material(theme.material(), support);

    ResolvedOverlayTheme {
        theme,
        effective_material,
        glass_support: support,
        file,
    }
}

/// Broadcast the resolved theme and apply its native side effects.
///
/// Order matters: the webviews are told first because a repaint is the slowest
/// link, then the native window is resized, because a change to `size_scale`
/// changes how much room the card needs. Repositioning unconditionally is
/// deliberate — it is cheap, the window is almost always hidden, and skipping
/// it would need a previous-resolved snapshot to diff against.
///
/// Never call this from inside a `run_on_main_thread` closure: every native
/// call it reaches hops to the main thread itself.
pub fn deliver(app: &AppHandle, resolved: &ResolvedOverlayTheme) {
    use tauri_specta::Event;

    // The two values the native steps below consume, logged before they are
    // applied so a report of "the overlay is the wrong size" can be read
    // straight out of the log.
    debug!(
        "Delivering overlay theme: material={:?}, size_scale={}",
        resolved.effective_material,
        resolved.theme.size_scale()
    );

    if let Err(error) = resolved.emit(app) {
        warn!("Failed to emit the resolved overlay theme: {error}");
    }

    // The Material's native window effect is applied between these two steps
    // once the Glass module exists: window slack, and therefore the window
    // size, depends on it.
    crate::utils::update_overlay_position(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hex(raw: &str) -> Option<HexColor> {
        Some(HexColor::parse(raw).expect("test colours are valid"))
    }

    fn canonical(raw: &str) -> Option<String> {
        HexColor::parse(raw).map(|colour| colour.as_str().to_string())
    }

    fn scaled(size_scale: Option<f64>) -> OverlayTheme {
        OverlayTheme {
            size_scale,
            ..Default::default()
        }
    }

    /// The inherit shape is what makes "defaults reproduce today's overlay"
    /// an invariant rather than a transcription of the current CSS: an unset
    /// token writes no custom property at all.
    #[test]
    fn default_overlay_theme_is_all_inherit() {
        let theme = OverlayTheme::default();

        assert_eq!(theme.accent, None);
        assert_eq!(theme.surface, None);
        assert_eq!(theme.surface_opacity, None);
        assert_eq!(theme.text, None);
        assert_eq!(theme.material, None);
        assert_eq!(theme.size_scale, None);
        assert_eq!(theme.radius, None);
        assert_eq!(theme.padding, None);
        assert_eq!(theme.waveform_gap, None);

        // The accessors' inherit values.
        assert_eq!(theme.size_scale(), 1.0);
        assert_eq!(theme.material(), Material::Flat);

        // A store written before this field existed, and an explicit
        // all-null document, are the same thing.
        let missing: OverlayTheme =
            serde_json::from_value(json!({})).expect("every token needs a serde default");
        assert_eq!(missing, theme);

        let explicit_nulls: OverlayTheme = serde_json::from_value(json!({
            "accent": null,
            "surface": null,
            "surface_opacity": null,
            "text": null,
            "material": null,
            "size_scale": null,
            "radius": null,
            "padding": null,
            "waveform_gap": null
        }))
        .expect("null is the explicit spelling of inherit");
        assert_eq!(explicit_nulls, theme);
    }

    /// Salvage tier one: a token whose value is unusable inherits, and the
    /// other eight survive untouched.
    #[test]
    fn one_bad_token_inherits_and_keeps_its_siblings() {
        let parsed: OverlayTheme = serde_json::from_value(json!({
            "accent": 5,                  // wrong JSON type
            "surface": "#1a1b26",
            "surface_opacity": 0.92,
            "text": "rebeccapurple",      // a CSS colour name, not a hex value
            "material": "Glass",          // the store's enums are case-sensitive
            "size_scale": 1.1,
            "radius": 12.5,               // a float where integer px is required
            "padding": 14,
            "waveform_gap": 2
        }))
        .expect("one bad token must never fail the whole theme");

        assert_eq!(parsed.accent, None);
        assert_eq!(parsed.text, None);
        assert_eq!(parsed.material, None);
        assert_eq!(parsed.radius, None);

        assert_eq!(parsed.surface, hex("#1a1b26"));
        assert_eq!(parsed.surface_opacity, Some(0.92));
        assert_eq!(parsed.size_scale, Some(1.1));
        assert_eq!(parsed.padding, Some(14));
        assert_eq!(parsed.waveform_gap, Some(2));
    }

    #[test]
    fn hex_color_parses_leniently_and_canonicalises() {
        // Shorthand expands by digit doubling, exactly as CSS does.
        assert_eq!(canonical("#ABC"), Some("#aabbcc".to_string()));
        assert_eq!(canonical("abc"), Some("#aabbcc".to_string()));
        // The `#` is optional, case is free, whitespace is trimmed.
        assert_eq!(canonical("7aa2f7"), Some("#7aa2f7".to_string()));
        assert_eq!(canonical("  #7AA2F7\n"), Some("#7aa2f7".to_string()));

        // Alpha belongs to surface_opacity, so 4- and 8-digit forms are out.
        assert_eq!(canonical("#7aa2f7ff"), None);
        assert_eq!(canonical("#7aa2"), None);
        // Named colours cannot round-trip through a hex field.
        assert_eq!(canonical("red"), None);
        assert_eq!(canonical(""), None);
        assert_eq!(canonical("#12345"), None);
        assert_eq!(canonical("#gggggg"), None);

        // The canonical form is what is serialized, never the raw input.
        let value = serde_json::to_value(HexColor::parse("#ABC").expect("valid"))
            .expect("a colour serializes to a string");
        assert_eq!(value, json!("#aabbcc"));

        let parsed: HexColor =
            serde_json::from_value(json!("ABC")).expect("deserialization is lenient too");
        assert_eq!(parsed.as_str(), "#aabbcc");
        assert!(serde_json::from_value::<HexColor>(json!("red")).is_err());
    }

    /// Bounds are written as the literals from the token contract, not as the
    /// module's constants: a mistyped constant must fail this test rather than
    /// redefine what it is checking.
    #[test]
    fn size_scale_clamps_and_survives_nan() {
        assert_eq!(OverlayTheme::default().size_scale(), 1.0);
        assert_eq!(scaled(Some(1.25)).size_scale(), 1.25);

        assert_eq!(scaled(Some(3.0)).size_scale(), 1.50);
        assert_eq!(scaled(Some(0.1)).size_scale(), 0.80);
        assert_eq!(scaled(Some(0.80)).size_scale(), 0.80);
        assert_eq!(scaled(Some(1.50)).size_scale(), 1.50);

        assert_eq!(scaled(Some(f64::NAN)).size_scale(), 1.0);
        assert_eq!(scaled(Some(f64::INFINITY)).size_scale(), 1.0);
        assert_eq!(scaled(Some(f64::NEG_INFINITY)).size_scale(), 1.0);
    }

    #[test]
    fn normalized_clamps_every_token() {
        let over = OverlayTheme {
            accent: hex("#7aa2f7"),
            surface: hex("#1a1b26"),
            surface_opacity: Some(2.0),
            text: hex("#c0caf5"),
            material: Some(Material::Glass),
            size_scale: Some(3.0),
            radius: Some(99),
            padding: Some(99),
            waveform_gap: Some(99),
        }
        .normalized();

        assert_eq!(over.surface_opacity, Some(1.00));
        assert_eq!(over.size_scale, Some(1.50));
        assert_eq!(over.radius, Some(32));
        assert_eq!(over.padding, Some(20));
        assert_eq!(over.waveform_gap, Some(5));
        // Colours and the enum are already canonical; clamping leaves them be.
        assert_eq!(over.accent, hex("#7aa2f7"));
        assert_eq!(over.surface, hex("#1a1b26"));
        assert_eq!(over.text, hex("#c0caf5"));
        assert_eq!(over.material, Some(Material::Glass));

        let under = OverlayTheme {
            surface_opacity: Some(0.1),
            size_scale: Some(0.1),
            ..Default::default()
        }
        .normalized();
        assert_eq!(under.surface_opacity, Some(0.30));
        assert_eq!(under.size_scale, Some(0.80));

        // Unset stays unset: clamping must never invent a value, or every
        // token would start writing a custom property.
        assert_eq!(
            OverlayTheme::default().normalized(),
            OverlayTheme::default()
        );

        // A non-finite float has no sensible place on the scale, so it
        // inherits rather than clamping to a bound: unset stays unset, and the
        // apply layer writes no custom property for it.
        let non_finite = OverlayTheme {
            surface_opacity: Some(f64::NAN),
            size_scale: Some(f64::INFINITY),
            ..Default::default()
        }
        .normalized();
        assert_eq!(non_finite.surface_opacity, None);
        assert_eq!(non_finite.size_scale, None);
        assert_eq!(non_finite.size_scale(), 1.0);
        // Not `to_value(..).is_ok()`: serde_json turns a non-finite float into
        // `null`, so that would pass even without the drop. What matters is
        // that the token is gone, and therefore serializes as an absent value.
        assert_eq!(
            serde_json::to_value(&non_finite).expect("a normalized theme serializes"),
            serde_json::to_value(OverlayTheme::default()).expect("the default serializes")
        );
    }

    #[test]
    fn merge_prefers_the_file_per_key() {
        let file = OverlayTheme {
            accent: hex("#7aa2f7"),
            size_scale: Some(1.1),
            ..Default::default()
        };
        let settings = OverlayTheme {
            accent: hex("#ff0000"),
            surface: hex("#1a1b26"),
            radius: Some(12),
            ..Default::default()
        };

        let merged = merge(&file, &settings);

        // The file wins the keys it sets…
        assert_eq!(merged.accent, hex("#7aa2f7"));
        assert_eq!(merged.size_scale, Some(1.1));
        // …the settings fill the gaps…
        assert_eq!(merged.surface, hex("#1a1b26"));
        assert_eq!(merged.radius, Some(12));
        // …and a key neither of them sets still inherits.
        assert_eq!(merged.text, None);

        // Merging with an absent file is the settings, unchanged.
        assert_eq!(merge(&OverlayTheme::default(), &settings), settings);
    }

    /// The resolver is the only place the three rules meet, so pin them
    /// together: the file outranks the settings, the result is clamped once,
    /// and a Glass request renders Flat while `available` is false — which is
    /// exactly the state this build ships in, before the native Glass module.
    #[test]
    fn resolve_clamps_once_and_downgrades_glass_when_unavailable() {
        let mut file = ThemeFileState::absent();
        file.present = true;
        file.owned_keys = vec!["size_scale".to_string()];
        file.tokens = OverlayTheme {
            size_scale: Some(9.0),
            ..Default::default()
        };

        let settings_theme = OverlayTheme {
            accent: hex("#7aa2f7"),
            surface_opacity: Some(0.05),
            material: Some(Material::Glass),
            size_scale: Some(1.0),
            radius: Some(99),
            ..Default::default()
        };

        let unavailable = GlassSupport {
            supported: true,
            available: false,
        };
        let resolved = resolve_from(settings_theme.clone(), file.clone(), unavailable);

        // The file's out-of-range value wins the key and is then clamped.
        assert_eq!(resolved.theme.size_scale, Some(1.50));
        assert_eq!(resolved.theme.size_scale(), 1.50);
        // The settings' own out-of-range values are clamped in the same pass.
        assert_eq!(resolved.theme.surface_opacity, Some(0.30));
        assert_eq!(resolved.theme.radius, Some(32));
        assert_eq!(resolved.theme.accent, hex("#7aa2f7"));

        // The request survives verbatim; only what is rendered is downgraded,
        // so turning Glass back on never has to re-ask the user.
        assert_eq!(resolved.theme.material, Some(Material::Glass));
        assert_eq!(resolved.effective_material, Material::Flat);
        assert_eq!(resolved.glass_support, unavailable);

        // The file state rides through untouched: it is what draws the tab's
        // lock markers.
        assert_eq!(resolved.file, file);

        // Same inputs, Glass actually available: now it renders.
        let available = GlassSupport {
            supported: true,
            available: true,
        };
        let rendered = resolve_from(settings_theme, ThemeFileState::absent(), available);
        assert_eq!(rendered.effective_material, Material::Glass);
        // With no file, the settings' own scale survives.
        assert_eq!(rendered.theme.size_scale, Some(1.0));
    }

    #[test]
    fn effective_material_downgrades_glass_when_unavailable() {
        let unavailable = GlassSupport {
            supported: true,
            available: false,
        };
        let available = GlassSupport {
            supported: true,
            available: true,
        };

        assert_eq!(
            effective_material(Material::Glass, available),
            Material::Glass
        );
        assert_eq!(
            effective_material(Material::Glass, unavailable),
            Material::Flat
        );
        assert_eq!(
            effective_material(Material::Flat, available),
            Material::Flat
        );
    }
}
