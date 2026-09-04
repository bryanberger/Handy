//! Overlay theme: storage, per-key merge, and delivery.
//!
//! Twenty-two optional tokens decide how the recording overlay's card looks.
//! Absent means inherit, Handy's built-in theme-aware value, so a theme that
//! sets nothing reproduces today's overlay exactly.
//!
//! The resolved overlay theme, what both webviews read, is those tokens merged
//! (`theme file ?? settings ?? inherit` per key, clamped) plus the Material
//! rendered, whether Glass is available and the theme file's state. It is the
//! `resolved-overlay-theme` event payload and the return type of
//! [`crate::commands::overlay_theme::get_resolved_overlay_theme`], so pull and
//! push cannot diverge.
//!
//! All px tokens are expressed at `size_scale` = 1; the scale multiplies them.

use log::{debug, warn};
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use tauri::AppHandle;

/// A canonical `#rrggbb` colour.
///
/// Lenient parsing, always lowercase `#rrggbb`: `#RGB` shorthand, a missing
/// `#`, any case and whitespace all work; 4- and 8-digit forms (alpha) and CSS
/// colour names do not. The only string reaching a CSS custom property, and
/// re-serialised from this type, so no stored value is echoed verbatim.
#[derive(Clone, Debug, PartialEq, Eq, Type)]
pub struct HexColor(String);

impl HexColor {
    /// The one lenient colour parser, shared by the settings store and the
    /// theme file. `None` means the contract rejects the value; the caller
    /// decides whether that is a dropped token or a diagnostic.
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
        if !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
            return None;
        }

        let expanded = match digits.len() {
            // `#RGB` expands by digit doubling, as CSS does.
            3 => digits.chars().flat_map(|digit| [digit, digit]).collect(),
            6 => digits.to_string(),
            // Anything else is alpha (4 or 8 digits), which belongs to
            // `surface_opacity` or `glass_tint`, or not a colour at all.
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
    /// A translucent surface backed by a native blur of what is behind the
    /// overlay window. macOS only.
    Glass,
}

/// Which macOS material the Glass blur is drawn with.
///
/// `material` is a live setter on the one `NSVisualEffectView`, so a swap
/// costs one property assignment. Read only while the effective Material is
/// Glass; merged and ignored on Flat and off macOS.
///
/// The eight `NSVisualEffectMaterial` cases that suit a small floating card,
/// most see-through first. The default measured the most backdrop transmission
/// on macOS 26, in both app themes, at the tint an unset `glass_tint` gives.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum GlassMaterial {
    /// `NSVisualEffectMaterialHUDWindow`: the most see-through of the eight in
    /// both app themes, and the default. It alone ignores the system
    /// appearance, a fixed dark recipe that reads as contrast under the thin
    /// default tint. Over a white backdrop under a Light theme it is within
    /// 3 levels of Popover; over a dark one it darkens 13 levels more.
    #[default]
    HudWindow,
    /// `NSVisualEffectMaterialPopover`: follows the appearance, about two
    /// thirds of HudWindow's transmission, and the pick for a card that does.
    Popover,
    /// `NSVisualEffectMaterialMenu`: follows the appearance, denser again.
    Menu,
    /// `NSVisualEffectMaterialSidebar`: follows the appearance, softer.
    Sidebar,
    /// `NSVisualEffectMaterialUnderWindowBackground`: the widest blur radius,
    /// little transmission left.
    UnderWindowBackground,
    /// `NSVisualEffectMaterialSheet`: opaque in both themes on macOS 26.
    Sheet,
    /// `NSVisualEffectMaterialToolTip`: follows the appearance, very light.
    Tooltip,
    /// `NSVisualEffectMaterialContentBackground`: opaque in both themes on
    /// macOS 26.
    ContentBackground,
}

impl GlassMaterial {
    /// Declaration order, which is the order the Appearance tab's dropdown and
    /// the theme file's documentation use.
    pub const ALL: [GlassMaterial; 8] = [
        Self::HudWindow,
        Self::Popover,
        Self::Menu,
        Self::Sidebar,
        Self::UnderWindowBackground,
        Self::Sheet,
        Self::Tooltip,
        Self::ContentBackground,
    ];

    /// The theme-file spelling, also the serde representation and the value
    /// the frontend's bindings carry.
    pub fn as_key(self) -> &'static str {
        match self {
            Self::HudWindow => "hud_window",
            Self::Popover => "popover",
            Self::Menu => "menu",
            Self::Sidebar => "sidebar",
            Self::UnderWindowBackground => "under_window_background",
            Self::Sheet => "sheet",
            Self::Tooltip => "tooltip",
            Self::ContentBackground => "content_background",
        }
    }
}

/// Which Liquid Glass recipe `NSGlassEffectView` draws.
///
/// macOS 26 replaced the frosted `NSVisualEffectView` look with Liquid Glass,
/// whose two published styles are the whole choice: `Regular`, standard glass
/// dimming itself so content stays legible over anything, and `Clear`, thinner
/// and leaning on the backdrop. Read only while the liquid engine draws
/// (macOS 26 and later); merged and ignored on the fallback engine and off
/// macOS, where `GlassMaterial` is its equivalent.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum GlassStyle {
    /// `NSGlassEffectViewStyleRegular`: the default, and the one that keeps a
    /// transcript readable over a bright desktop.
    #[default]
    Regular,
    /// `NSGlassEffectViewStyleClear`: thinner glass, more backdrop.
    Clear,
}

impl GlassStyle {
    /// Declaration order, which is the order the Appearance tab's segmented
    /// control and the theme file's documentation use.
    pub const ALL: [GlassStyle; 2] = [Self::Regular, Self::Clear];

    /// The theme-file spelling, also the serde representation and the value
    /// the frontend's bindings carry.
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Clear => "clear",
        }
    }
}

/// How the control row's waveform is drawn.
///
/// `Bars` is today's nine capsules, the inherit, and the only value drawn as
/// DOM elements; the other five are drawn on one canvas in the waveform lane.
/// The lane is the same width whatever draws in it, so the style never changes
/// the card's footprint and no window is a function of it.
///
/// Four of the five read `waveform_width` and one reads `waveform_gap`; the
/// Appearance tab hides the rows a style ignores. `WAVEFORM_STYLE_TOKENS` in
/// `src/overlay/waveform/waveformStyles.ts` is the same table, pinned by
/// `the_waveform_styles_match_the_frontends`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaveformStyle {
    /// Nine centred capsules, one per bucket, each as tall as its level.
    /// Handy's own meter, unchanged, and what an unset token inherits.
    #[default]
    Bars,
    /// One continuous ribbon mirrored about the centre line, its thickness
    /// following the levels while a slow drift carries it sideways.
    Ribbon,
    /// A single rounded lozenge whose outline deforms per bucket and breathes
    /// with the overall level: one living thing rather than a graph.
    Bloom,
    /// A field of soft round motes drifting up out of the lane, loudness
    /// lighting more of them and throwing them further.
    Motes,
    /// A dot-matrix VU: each bucket a column of square dots lit from the
    /// centre outward in quantised steps.
    Matrix,
    /// A contiguous stepped histogram, square corners and no gaps, heights
    /// quantised to fixed levels.
    Steps,
}

impl WaveformStyle {
    /// Declaration order, which is the order the Appearance tab's dropdown and
    /// the theme file's documentation use.
    pub const ALL: [WaveformStyle; 6] = [
        Self::Bars,
        Self::Ribbon,
        Self::Bloom,
        Self::Motes,
        Self::Matrix,
        Self::Steps,
    ];

    /// The theme-file spelling, also the serde representation and the value
    /// the frontend's bindings carry.
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Bars => "bars",
            Self::Ribbon => "ribbon",
            Self::Bloom => "bloom",
            Self::Motes => "motes",
            Self::Matrix => "matrix",
            Self::Steps => "steps",
        }
    }
}

/// Which native implementation is drawing the Glass surface.
///
/// Not a token but a fact about the running machine, riding alongside
/// `GlassSupport` so the Appearance tab offers what the engine honours instead
/// of guessing from a macOS version in TypeScript. On Liquid Glass that is the
/// Glass style; on the fallback, nothing.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum GlassEngine {
    /// Nothing is installed: off macOS, or the install failed. Always paired
    /// with `available: false`.
    #[default]
    None,
    /// One `NSVisualEffectView`, the pre-macOS-26 blur. Honours `GlassMaterial`.
    VisualEffect,
    /// One `NSGlassEffectView`, which is Liquid Glass on macOS 26 and later.
    /// Honours `GlassStyle` and tints itself from the surface.
    Liquid,
}

/// The card surface an unset `surface` token inherits under a light app
/// appearance: `--light-color-background` in `src/styles/theme.css`, what the
/// apply layer's `var(--color-background)` resolves to there.
///
/// Rust needs the literal because the liquid engine paints the surface tint
/// natively, inside the glass, out of reach of any CSS variable. Pinned by
/// `overlay::tests::inherit_surface_matches_the_app_palette`.
///
/// macOS-only, like the four items below it. An unconditional item would be
/// dead code on Windows and Linux, which compose no native tint. `test` keeps
/// the pin and the composition tests running everywhere.
#[cfg(any(target_os = "macos", test))]
pub(crate) const INHERIT_SURFACE_LIGHT: &str = "#fbfbfb";
/// As [`INHERIT_SURFACE_LIGHT`], for a dark app appearance:
/// `--dark-color-background` in `src/styles/theme.css`.
#[cfg(any(target_os = "macos", test))]
pub(crate) const INHERIT_SURFACE_DARK: &str = "#2c2b29";

/// The alpha an unset `glass_tint` resolves to.
///
/// Liquid Glass paints the colour twice, once by the card and once by the
/// glass, whose `tintColor` lenses it rather than laying it on flat. This is
/// the second half, composed by [`liquid_tint`] when the token is unset, and
/// must match `GLASS_TINT_INHERIT` in `src/lib/overlayTheme.ts`, the first
/// half. Measured on macOS 26, 0.45 holds the transcript at 5.6 to 9.6:1
/// across both Glass styles and app themes; 0.30 drops it to 4.3:1 under a
/// Light theme. Both engines use it, the fallback blur measuring 0.45 too.
#[cfg(any(target_os = "macos", test))]
pub(crate) const GLASS_TINT_INHERIT: f64 = 0.45;

/// A straight-alpha sRGB colour, every component 0 to 1: what the liquid
/// engine's `tintColor` is built from.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TintColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

/// The tint Liquid Glass paints inside the glass: the resolved `surface`, or
/// the app background an unset one inherits, at the resolved `glass_tint`.
///
/// `surface_opacity` is deliberately not an input. It is Flat's control, and
/// under Glass only `glass_tint` sets the tint's strength, which is what lets
/// an opaque Flat card and see-through Glass coexist in one theme.
///
/// Pure, so the composition is testable without AppKit. `dark` is the overlay
/// window's effective appearance, the one native read this needs. `None` is
/// untinted glass, Apple's own look, asked for by a zero tint and permitted by
/// [`GLASS_TINT_MIN`].
#[cfg(any(target_os = "macos", test))]
pub(crate) fn liquid_tint(
    surface: Option<&HexColor>,
    glass_tint: Option<f64>,
    dark: bool,
) -> Option<TintColor> {
    let alpha = glass_tint
        .filter(|value| value.is_finite())
        .unwrap_or(GLASS_TINT_INHERIT)
        .clamp(GLASS_TINT_MIN, GLASS_TINT_MAX);
    if alpha <= 0.0 {
        return None;
    }

    let inherited = if dark {
        INHERIT_SURFACE_DARK
    } else {
        INHERIT_SURFACE_LIGHT
    };
    let hex = surface.map(HexColor::as_str).unwrap_or(inherited);
    let channel = |offset: usize| {
        u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap_or_default() as f64 / 255.0
    };

    Some(TintColor {
        red: channel(1),
        green: channel(3),
        blue: channel(5),
        alpha,
    })
}

/// Lowest accepted [`OverlayTheme::size_scale`].
pub const SIZE_SCALE_MIN: f64 = 0.80;
/// Highest accepted [`OverlayTheme::size_scale`].
pub const SIZE_SCALE_MAX: f64 = 1.50;
/// Lowest accepted `surface_opacity`. Flat's card may dim but never vanish.
/// That is what Glass is for.
pub const SURFACE_OPACITY_MIN: f64 = 0.30;
/// Highest accepted `surface_opacity`.
pub const SURFACE_OPACITY_MAX: f64 = 1.00;
/// Lowest accepted `glass_tint`. Zero is legitimate here, unlike
/// `surface_opacity`'s floor. It asks for untinted glass, Apple's own look,
/// and the blur behind it still makes the card visible.
pub const GLASS_TINT_MIN: f64 = 0.00;
/// Highest accepted `glass_tint`. At 1.00 the tint is opaque and the glass
/// transmits nothing, a way of saying "Flat" the theme is allowed to say.
pub const GLASS_TINT_MAX: f64 = 1.00;
/// Highest accepted `radius`, in px at scale 1.
pub const RADIUS_MAX: u16 = 32;
/// Highest accepted `padding`, in px at scale 1.
pub const PADDING_MAX: u16 = 20;
/// Highest accepted `waveform_gap`, in px at scale 1.
pub const WAVEFORM_GAP_MAX: u16 = 5;
/// Lowest accepted `border_opacity`. Zero is legitimate. It is how a theme
/// asks for a card with no visible edge without giving up the width.
pub const BORDER_OPACITY_MIN: f64 = 0.00;
/// Highest accepted `border_opacity`.
pub const BORDER_OPACITY_MAX: f64 = 1.00;
/// Highest accepted `border_width`, in px at scale 1. Past 4 the stroke reads
/// as a second surface, not an edge.
pub const BORDER_WIDTH_MAX: u16 = 4;
/// Lowest accepted `waveform_width`, in px at scale 1. Below 2 the bars all
/// but vanish at the smallest size scale.
pub const WAVEFORM_WIDTH_MIN: u16 = 2;
/// Highest accepted `waveform_width`, in px at scale 1.
///
/// The invariant: the control row's centre column is at most
/// `9 * WAVEFORM_WIDTH_MAX + 8 * WAVEFORM_GAP_MAX + 8` (nine bars, eight gaps,
/// `.swave`'s 8 px right padding). With the row's two `PADDING_MAX` insets and
/// the two 22 px side columns for the dot and the cancel button that is
/// 186 px, inside the 216 px working pill. So these tokens cannot widen the
/// card, leaving `size_scale` and `border_width` the only ones that do, while
/// `padding` changes the height. Pinned by
/// `overlay::tests::the_waveform_never_outgrows_the_working_pill`.
pub const WAVEFORM_WIDTH_MAX: u16 = 6;
/// Lowest accepted `shadow_strength`. Zero is legitimate: what Flat inherits,
/// today's card casting no shadow.
pub const SHADOW_STRENGTH_MIN: f64 = 0.00;
/// Highest accepted `shadow_strength`.
pub const SHADOW_STRENGTH_MAX: f64 = 1.00;
/// Highest accepted `shadow_offset_y`, in px at scale 1. Past 16 the card
/// stops reading as something sitting above the desktop.
pub const SHADOW_OFFSET_Y_MAX: u16 = 16;
/// Highest accepted `element_gap`, in px at scale 1. At the bound the row's two
/// gaps add 80 px to every card: an airy pill by design, not a broken one.
pub const ELEMENT_GAP_MAX: u16 = 40;

/// The `border_width` an unset token inherits, in px at scale 1: today's
/// hairline (`--ov-border-w: 1px`, `RecordingOverlay.css`).
pub const BORDER_WIDTH_INHERIT: u16 = 1;
/// The `padding` an unset token inherits, in px at scale 1: today's inset
/// (`--ov-pad: 10px`, `RecordingOverlay.css`). At this value the control row
/// is the 40 px it has always been and the Live transcript's inset is 12 px.
pub const PADDING_INHERIT: u16 = 10;
/// The `waveform_gap` an unset token inherits, in px at scale 1: today's gap
/// (`--ov-wave-gap: 3px`, `RecordingOverlay.css`).
pub const WAVEFORM_GAP_INHERIT: u16 = 3;
/// The `waveform_width` an unset token inherits, in px at scale 1: today's
/// bar (`--ov-wave-w: 4px`, `RecordingOverlay.css`).
pub const WAVEFORM_WIDTH_INHERIT: u16 = 4;
/// The `shadow_offset_y` an unset token inherits, in px at scale 1
/// (`--ov-shadow-y: 4px`, `RecordingOverlay.css`). Invisible at Flat's
/// inherit strength of 0, and never read under Glass, where macOS owns the
/// shadow and offers no offset.
pub const SHADOW_OFFSET_Y_INHERIT: u16 = 4;
/// The `element_gap` an unset token inherits, in px at scale 1
/// (`--ov-elem-gap: 0px`, `RecordingOverlay.css`): today's row.
pub const ELEMENT_GAP_INHERIT: u16 = 0;

/// The `shadow_strength` an unset token inherits, the one token whose inherit
/// differs per Material.
///
/// Flat inherits 0, today's shadowless card. Glass inherits 1, macOS's own
/// window shadow, which a Glass overlay has always cast. So the token adds a
/// shadow to Flat and takes one from Glass, and unset reproduces both. Must
/// match `SHADOW_STRENGTH_INHERIT` in `src/lib/overlayTheme.ts`.
pub const fn shadow_strength_inherit(material: Material) -> f64 {
    match material {
        Material::Flat => 0.0,
        Material::Glass => 1.0,
    }
}

/// A token whose stored value does not parse becomes `None`, meaning inherit,
/// rather than failing the whole [`OverlayTheme`] and making `salvage_settings`
/// drop the `overlay_theme` key and reset every token.
///
/// Safe because `AppSettings` is only ever deserialized from a self-describing
/// `serde_json::Value`.
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

/// The twenty-two overlay-theme tokens. `None` means inherit.
///
/// Field names are the theme-file keys, and every field deserializes
/// leniently: a wrong type or shape degrades to `None` with a `warn!`, so one
/// bad token never costs the other twenty-one, as `salvage_settings` does one
/// level up. The store salvages silently (log only); the theme file applies
/// the same rules but reports diagnostics, so it runs its own per-key pass
/// instead of deserializing an `OverlayTheme`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default, Type)]
#[serde(default)]
pub struct OverlayTheme {
    /// Highlight colour: waveform bars, recording dot, caret, spinner arc.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub accent: Option<HexColor>,
    /// The card's background colour.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub surface: Option<HexColor>,
    /// The card background's alpha under Flat, 0.30 to 1.00.
    ///
    /// Read only while the effective Material is Flat; under Glass the card's
    /// alpha is `glass_tint`, so one theme holds both an opaque Flat card and
    /// a see-through Glass one. Before the split, Glass at a high opacity
    /// painted an opaque card and nothing said why.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub surface_opacity: Option<f64>,
    /// How much of the `surface` colour covers the glass, 0.00 to 1.00.
    ///
    /// Glass's half of the pair above: the alpha the card paints its surface
    /// at while the effective Material is Glass, and the alpha the liquid
    /// engine's native `tintColor` is composed at. Ignored under Flat.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub glass_tint: Option<f64>,
    /// The card's foreground colour, and the base every neutral derives from.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub text: Option<HexColor>,
    /// The card's border colour, before `border_opacity`. Unset it derives
    /// from `text` on both Materials, only at a stronger alpha under Glass.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub border: Option<HexColor>,
    /// The card border's alpha, 0.00 to 1.00.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub border_opacity: Option<f64>,
    /// Flat or Glass. Glass renders as Flat wherever it is unavailable, so
    /// what is actually painted is the resolved theme's effective material.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub material: Option<Material>,
    /// Which macOS material the Glass blur uses. Read only by the
    /// `visual_effect` engine, so ignored under Flat and on macOS 26.
    ///
    /// Theme-file only. Its row left the Appearance tab when Liquid Glass
    /// arrived, so the merge takes it from the file, never the settings store,
    /// where a value an older build persisted would drive the fallback engine
    /// with no control to show or clear it. The field stays so those documents
    /// deserialize and a theme copied out of the tab still round-trips.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub glass_material: Option<GlassMaterial>,
    /// Which Liquid Glass style the Glass surface uses. Read only by the
    /// `liquid` engine, so ignored under Flat and before macOS 26.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub glass_style: Option<GlassStyle>,
    /// How heavy the card's drop shadow is, 0.00 to 1.00.
    ///
    /// The two Materials draw a shadow in two different places, so this token
    /// means two things. Under Flat it shapes a CSS `box-shadow` on the card,
    /// and the window grows a symmetric margin for it to fall into. Under
    /// Glass, where the window is the card, the shadow is macOS's own and
    /// `NSWindow` offers no strength, so any value above zero switches it on
    /// and zero switches it off.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub shadow_strength: Option<f64>,
    /// How far the card's shadow is pushed below it at scale 1, 0 to 16 px.
    ///
    /// Flat only. macOS places its own window shadow, so this is ignored under
    /// Glass. It sizes the window's shadow slack together with the fixed blur
    /// radius, so it is one of the values the native window is built from.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub shadow_offset_y: Option<u16>,
    /// Whether the control row shows the waveform. Unset means it does.
    ///
    /// Hiding it empties the row's centre column, and the two resting shapes
    /// (the Minimal pill and the Live pill) shrink to what the row still
    /// holds. The working pill and the open panel keep their tuned widths.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub show_waveform: Option<bool>,
    /// Whether the control row shows the cancel button. Unset means it does.
    ///
    /// The keyboard shortcut and `--cancel` still cancel; only the button on
    /// the card goes. With it the row's side columns lose the 22 px floor that
    /// existed to hold it.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub show_cancel: Option<bool>,
    /// One factor multiplying every length in the card, 0.80 to 1.50.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub size_scale: Option<f64>,
    /// The card's corner radius at scale 1, 0 to 32 px.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub radius: Option<u16>,
    /// The card's border width at scale 1, 0 to 4 px. One of the two tokens
    /// besides `size_scale` that change the card's footprint, so the native
    /// window is computed from it.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub border_width: Option<u16>,
    /// The card's inner padding on all four sides at scale 1, 0 to 20 px. The
    /// control row is a fixed core plus one of these above and below, and the
    /// Live transcript's inset follows, so the card grows taller with it and
    /// the native window is computed from it too.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub padding: Option<u16>,
    /// Extra horizontal space between the control row's elements (the dot, the
    /// waveform, the timer and the cancel button) at scale 1, 0 to 40 px.
    ///
    /// The row has two of these, so every card is twice the gap wider and the
    /// native window follows. The centre column's room is unchanged, since the
    /// card gains exactly what the two gaps take.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub element_gap: Option<u16>,
    /// How the waveform is drawn. Unset means today's bars, which is the
    /// enum's own default.
    ///
    /// The only token nothing native reads: the waveform lane is the same
    /// width whatever draws in it, so a style can never move a window, and the
    /// card alone resolves the inherit. Hence no accessor beside the others.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub waveform_style: Option<WaveformStyle>,
    /// Gap between waveform bars at scale 1, 0 to 5 px.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub waveform_gap: Option<u16>,
    /// Width of each waveform bar at scale 1, 2 to 6 px.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub waveform_width: Option<u16>,
}

impl OverlayTheme {
    /// The persisted size scale: unset ⇒ 1.0, non-finite ⇒ 1.0, otherwise
    /// clamped between [`SIZE_SCALE_MIN`] and [`SIZE_SCALE_MAX`].
    ///
    /// The single clamp, shared by the native window geometry, the theme file
    /// and the apply layer, so nothing clamps differently.
    pub fn size_scale(&self) -> f64 {
        match self.size_scale {
            Some(scale) if scale.is_finite() => scale.clamp(SIZE_SCALE_MIN, SIZE_SCALE_MAX),
            _ => 1.0,
        }
    }

    /// The requested Material: unset ⇒ Flat. This is not the effective one;
    /// see [`effective_material`].
    pub fn material(&self) -> Material {
        self.material.unwrap_or_default()
    }

    /// The macOS material the Glass blur is drawn with: unset ⇒ the measured
    /// default. Only consulted while the effective Material is Glass.
    pub fn glass_material(&self) -> GlassMaterial {
        self.glass_material.unwrap_or_default()
    }

    /// The Liquid Glass style the blur is drawn with: unset ⇒ Regular. Only
    /// consulted while the liquid engine is drawing.
    pub fn glass_style(&self) -> GlassStyle {
        self.glass_style.unwrap_or_default()
    }

    /// The border width in px at `size_scale` 1: unset ⇒
    /// [`BORDER_WIDTH_INHERIT`], otherwise clamped to `0..=BORDER_WIDTH_MAX`.
    ///
    /// The single clamp, like [`Self::size_scale`]. The native window footprint
    /// includes two of these, so the geometry and the card must agree on how
    /// wide a border may get.
    pub fn border_width(&self) -> u16 {
        self.border_width
            .unwrap_or(BORDER_WIDTH_INHERIT)
            .min(BORDER_WIDTH_MAX)
    }

    /// The padding in px at `size_scale` 1: unset ⇒ [`PADDING_INHERIT`],
    /// otherwise clamped to `0..=PADDING_MAX`.
    ///
    /// The single clamp, like [`Self::border_width`]. The card's height is a
    /// function of this, so the geometry and the card must agree on how far a
    /// padding may go.
    pub fn padding(&self) -> u16 {
        self.padding.unwrap_or(PADDING_INHERIT).min(PADDING_MAX)
    }

    /// The `waveform_gap` in px at `size_scale` 1: unset ⇒
    /// [`WAVEFORM_GAP_INHERIT`], otherwise clamped to `0..=WAVEFORM_GAP_MAX`.
    ///
    /// The card's resting shapes are sized from it (see
    /// `overlay_geometry::CardMetrics`), so the geometry and the card must
    /// agree on how wide the waveform lane may get.
    pub fn waveform_gap(&self) -> u16 {
        self.waveform_gap
            .unwrap_or(WAVEFORM_GAP_INHERIT)
            .min(WAVEFORM_GAP_MAX)
    }

    /// The `waveform_width` in px at `size_scale` 1: unset ⇒
    /// [`WAVEFORM_WIDTH_INHERIT`], otherwise clamped to the bar's bounds.
    pub fn waveform_width(&self) -> u16 {
        self.waveform_width
            .unwrap_or(WAVEFORM_WIDTH_INHERIT)
            .clamp(WAVEFORM_WIDTH_MIN, WAVEFORM_WIDTH_MAX)
    }

    /// The `shadow_strength` under `material`: unset ⇒
    /// [`shadow_strength_inherit`], otherwise clamped to `0.00..=1.00`.
    ///
    /// The Material is a parameter: this is the one token whose inherit depends
    /// on it. Asking unconditionally keeps callers from having to know it.
    pub fn shadow_strength(&self, material: Material) -> f64 {
        match self.shadow_strength {
            Some(value) if value.is_finite() => {
                value.clamp(SHADOW_STRENGTH_MIN, SHADOW_STRENGTH_MAX)
            }
            _ => shadow_strength_inherit(material),
        }
    }

    /// The `shadow_offset_y` in px at `size_scale` 1: unset ⇒
    /// [`SHADOW_OFFSET_Y_INHERIT`], otherwise clamped to
    /// `0..=SHADOW_OFFSET_Y_MAX`. Half of the window's shadow slack.
    pub fn shadow_offset_y(&self) -> u16 {
        self.shadow_offset_y
            .unwrap_or(SHADOW_OFFSET_Y_INHERIT)
            .min(SHADOW_OFFSET_Y_MAX)
    }

    /// The `element_gap` in px at `size_scale` 1: unset ⇒
    /// [`ELEMENT_GAP_INHERIT`], otherwise clamped to `0..=ELEMENT_GAP_MAX`.
    pub fn element_gap(&self) -> u16 {
        self.element_gap
            .unwrap_or(ELEMENT_GAP_INHERIT)
            .min(ELEMENT_GAP_MAX)
    }

    /// Whether the control row draws the waveform: unset ⇒ yes.
    pub fn show_waveform(&self) -> bool {
        self.show_waveform.unwrap_or(true)
    }

    /// Whether the control row draws the cancel button: unset ⇒ yes.
    pub fn show_cancel(&self) -> bool {
        self.show_cancel.unwrap_or(true)
    }

    /// A copy with every token clamped to this module's bounds.
    ///
    /// Applied before persisting and again after merging, so no out-of-range
    /// value reaches the native geometry or the frontend. Non-finite floats
    /// inherit instead of clamping to a bound; they cannot be serialized to
    /// JSON, so dropping them here also keeps the settings store writable.
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
            glass_tint: clamp_float(
                self.glass_tint,
                GLASS_TINT_MIN,
                GLASS_TINT_MAX,
                "glass_tint",
            ),
            text: self.text.clone(),
            border: self.border.clone(),
            border_opacity: clamp_float(
                self.border_opacity,
                BORDER_OPACITY_MIN,
                BORDER_OPACITY_MAX,
                "border_opacity",
            ),
            material: self.material,
            glass_material: self.glass_material,
            glass_style: self.glass_style,
            shadow_strength: clamp_float(
                self.shadow_strength,
                SHADOW_STRENGTH_MIN,
                SHADOW_STRENGTH_MAX,
                "shadow_strength",
            ),
            shadow_offset_y: self
                .shadow_offset_y
                .map(|value| value.min(SHADOW_OFFSET_Y_MAX)),
            show_waveform: self.show_waveform,
            show_cancel: self.show_cancel,
            size_scale: clamp_float(
                self.size_scale,
                SIZE_SCALE_MIN,
                SIZE_SCALE_MAX,
                "size_scale",
            ),
            radius: self.radius.map(|value| value.min(RADIUS_MAX)),
            border_width: self.border_width.map(|value| value.min(BORDER_WIDTH_MAX)),
            padding: self.padding.map(|value| value.min(PADDING_MAX)),
            element_gap: self.element_gap.map(|value| value.min(ELEMENT_GAP_MAX)),
            waveform_style: self.waveform_style,
            waveform_gap: self.waveform_gap.map(|value| value.min(WAVEFORM_GAP_MAX)),
            waveform_width: self
                .waveform_width
                .map(|value| value.clamp(WAVEFORM_WIDTH_MIN, WAVEFORM_WIDTH_MAX)),
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
    /// Which native view is installed, and so which of the two engine-specific
    /// tokens means anything here. `None` until an install succeeds, which is
    /// what off-macOS and a failed install both report.
    pub engine: GlassEngine,
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

/// Whether Glass can render on this machine, and whether it can right now.
///
/// Delegates to [`crate::overlay_glass::support`], which owns the installed
/// effect view and the live macOS "Reduce transparency" read, so every caller
/// here keeps one name whichever module answers.
pub fn glass_support(app: &AppHandle) -> GlassSupport {
    crate::overlay_glass::support(app)
}

/// What kind of thing the theme file got wrong.
///
/// A stable, translatable identity for a diagnostic. The Appearance tab looks
/// up an i18n string by code and passes `key` as a parameter, so the user reads
/// their own language while `message` keeps the English detail for the log.
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
    /// English, deliberately untranslated. It names JSON keys and values, and
    /// goes to the log.
    pub message: String,
}

/// Why Handy reads the theme file but will not write it.
///
/// A stable, translatable identity, like `ThemeFileDiagnosticCode`. The
/// Appearance tab looks up an i18n string by reason and passes the ownership's
/// `target` as its parameter. Plain text, not an intra-doc link: this doc
/// comment is copied verbatim into `src/bindings.ts`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum ManagedReason {
    /// The file itself is a symlink. Whoever made the link owns the document,
    /// which is how a dotfile or Omarchy-style setup says "this one is mine",
    /// so Handy follows it to read and never writes through it.
    Symlink,
    /// The file is there and its permissions refuse a write.
    ReadOnly,
    /// There is no file, and this is not a path Handy creates. Only
    /// `HANDY_OVERLAY_THEME_FILE` names such a path: it was given to be read.
    NotCreatable,
    /// The path could not be inspected at all. Reading may still have
    /// succeeded from the last good document, so this locks the tab rather
    /// than assuming a write would land.
    Unknown,
}

/// Whether the Appearance tab's changes are written to the theme file.
///
/// Handy owns the path when it is absent in one of Handy's own locations, or a
/// regular, writable file. Anything else belongs to the user or to a tool, and
/// Handy reads it without ever writing back.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type)]
pub struct ThemeFileOwnership {
    /// True when a committed change from the Appearance tab writes this path.
    pub writable: bool,
    /// Why not, when `writable` is false.
    pub reason: Option<ManagedReason>,
    /// A symlink's target, the one reason carrying a detail. The tab shows it,
    /// so the user can find the file really in charge.
    pub target: Option<String>,
}

impl ThemeFileOwnership {
    /// Handy writes this path.
    pub fn owned() -> Self {
        ThemeFileOwnership {
            writable: true,
            reason: None,
            target: None,
        }
    }

    /// Handy reads this path and leaves it alone.
    pub fn managed(reason: ManagedReason, target: Option<String>) -> Self {
        ThemeFileOwnership {
            writable: false,
            reason: Some(reason),
            target,
        }
    }
}

/// What the theme file currently contributes.
///
/// Only the theme-file reader reads the file, and it fills this in. Everything
/// downstream consumes this state instead of the document.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type)]
pub struct ThemeFileState {
    /// The file in effect, or the path Handy would read if one appeared.
    pub path: String,
    /// Whether a theme file was actually found and read at `path`.
    pub present: bool,
    /// The document's declared `version`, or `None` when it is absent or the
    /// file is not present. A missing version means 1.
    pub version: Option<u32>,
    /// The overlay theme this document holds, which is the overlay theme.
    pub tokens: OverlayTheme,
    /// The keys the document actually sets, in contract order. What the tab's
    /// "sets N of M values" line counts; its other tokens inherit.
    pub owned_keys: Vec<String>,
    /// Whether Handy writes this path, and why not when it does not. A managed
    /// file locks every token row in the Appearance tab.
    pub ownership: ThemeFileOwnership,
    /// Everything the reader had to ignore or clamp, in contract order (the
    /// token table's, not the document's, since `serde_json` sorts an object's
    /// keys unless `preserve_order` is on). Capped at a handful of entries for
    /// the payload; `diagnostics_total` is the count before the cap, and every
    /// diagnostic reaches the log uncapped.
    pub diagnostics: Vec<ThemeFileDiagnostic>,
    /// How many diagnostics the reader found before `diagnostics` was capped.
    /// Equal to `diagnostics.len()` when nothing was capped, larger when the
    /// tab needs to say "…and N more", `0` when the file is absent.
    pub diagnostics_total: u32,
    /// True when a failed read kept the previous, good document.
    pub stale: bool,
}

impl ThemeFileState {
    /// No theme file at `path`: contributes nothing, so the merge falls
    /// through to the settings and then to inherit.
    ///
    /// The path is still carried, because "no file" is the state the tab shows
    /// a path for, and where to create one.
    pub fn absent_at(path: impl Into<String>) -> Self {
        ThemeFileState {
            path: path.into(),
            present: false,
            version: None,
            tokens: OverlayTheme::default(),
            owned_keys: Vec::new(),
            // Absent in one of Handy's own locations is the ordinary first
            // launch, and the first committed change creates the file. The
            // reader overrides this for a path Handy was told only to read.
            ownership: ThemeFileOwnership::owned(),
            diagnostics: Vec::new(),
            diagnostics_total: 0,
            stale: false,
        }
    }
}

/// The whole answer to "how does the overlay look right now".
///
/// Both the command result and the event payload, so the overlay's pull on
/// show and the push on change carry the identical type.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type, tauri_specta::Event)]
pub struct ResolvedOverlayTheme {
    /// The theme file's tokens, clamped. `None` still means inherit: the apply
    /// layer writes no custom property for it.
    pub theme: OverlayTheme,
    /// Concrete, never `None`: the requested Material downgraded to Flat when
    /// Glass is unavailable.
    pub effective_material: Material,
    /// Whether Glass is offerable and whether it can render right now. Read by
    /// the Appearance tab instead of a TypeScript platform check, so the two
    /// sides cannot disagree.
    pub glass_support: GlassSupport,
    /// How far the overlay window may reach past the card towards the screen
    /// edge it is anchored to, in logical points, already scaled and whole.
    ///
    /// Derived, like the Material above, and for the same reason: only Rust
    /// knows the room the card has to the usable edge on this platform at this
    /// overlay position, and the overlay page must inset the card by exactly
    /// the same number or the card would move the moment a shadow is switched
    /// on. The apply layer writes it straight into `--ov-shadow-edge-slack`;
    /// the native window is sized and placed from it. Zero under Glass and
    /// whenever the shadow strength is zero.
    #[serde(default)]
    pub shadow_edge_slack: f64,
    /// The theme file behind `theme`: where it is, whether Handy writes it,
    /// which tokens it sets and what the reader had to ignore.
    pub file: ThemeFileState,
    /// Whether a file watcher is delivering changes to that file. False means
    /// the Appearance tab keeps its Reload button, the only way a hand edit
    /// reaches the screen then. Process-wide, so it rides here rather than on
    /// the file state, which is cached per read.
    pub watching: bool,
}

/// The overlay theme as the theme file has it, clamped.
///
/// Uses the theme-file cache, warmed by the launch-time read before any window
/// exists, so this does no filesystem IO and is safe on the main thread. (The
/// one cold-cache read inside [`crate::overlay_theme_file::cached`] can only
/// happen before that.)
pub fn resolve(app: &AppHandle) -> ResolvedOverlayTheme {
    let file = crate::overlay_theme_file::cached(app);
    resolve_from(
        file.tokens.clone(),
        file,
        glass_support(app),
        crate::overlay::anchored_edge_room(app),
        crate::overlay_theme_watch::is_watching(),
    )
}

/// [`resolve`] for tokens nobody has written to the file yet.
///
/// The preview path was handed a draft that never reaches disk. The theme is
/// taken as given and the file rides along as metadata only, so a draft paints
/// over the file rather than losing to it.
pub fn resolve_authored(app: &AppHandle, theme: OverlayTheme) -> ResolvedOverlayTheme {
    resolve_from(
        theme,
        crate::overlay_theme_file::cached(app),
        glass_support(app),
        crate::overlay::anchored_edge_room(app),
        crate::overlay_theme_watch::is_watching(),
    )
}

/// [`resolve`], preceded by a fresh read of the theme file.
///
/// The authoritative resolve: whatever the file says right now is what the
/// overlay shows. It touches the filesystem, so call it only off the main
/// thread. Its callers, the overlay show path, the watcher, a commit and
/// Reload, all do.
pub fn resolve_reloading(app: &AppHandle) -> ResolvedOverlayTheme {
    let file = crate::overlay_theme_file::read(app);
    resolve_from(
        file.tokens.clone(),
        file,
        glass_support(app),
        crate::overlay::anchored_edge_room(app),
        crate::overlay_theme_watch::is_watching(),
    )
}

/// The whole resolution rule with nothing to look up: clamp the tokens once,
/// decide the Material actually rendered, and work out how far the window may
/// grow towards the anchored screen edge.
///
/// `theme` is normally `file.tokens`, the file being the theme; a draft passes
/// its own, the file riding along as metadata. `edge_room` is the gap the card
/// has to the usable edge, which only the placement knows; the rest is here.
///
/// Pure, so the clamping, the Glass downgrade and the shadow's anchored-side
/// slack are testable together without an `AppHandle`.
pub fn resolve_from(
    theme: OverlayTheme,
    file: ThemeFileState,
    support: GlassSupport,
    edge_room: f64,
    watching: bool,
) -> ResolvedOverlayTheme {
    let theme = theme.normalized();
    let effective_material = effective_material(theme.material(), support);

    ResolvedOverlayTheme {
        shadow_edge_slack: crate::overlay_geometry::shadow_edge_slack(
            &theme,
            effective_material,
            edge_room,
        ),
        theme,
        effective_material,
        glass_support: support,
        file,
        watching,
    }
}

/// A theme being edited, on its way to the overlay window alone.
///
/// The same payload as `ResolvedOverlayTheme` under a second name, since the
/// name is the whole distinction. A draft is not persisted, so the overlay
/// paints it without mirroring it to localStorage, and the Appearance tab,
/// listening for the delivered theme to keep its controls honest, ignores it.
/// Wrapped, not aliased, so the two events stay two types in the bindings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Type, tauri_specta::Event)]
pub struct OverlayThemeDraft {
    pub resolved: ResolvedOverlayTheme,
}

/// Broadcast the resolved theme and apply its native side effects.
///
/// Order matters. It emits to the webviews first, a repaint being the slowest
/// link, then resizes the native window, `size_scale` having changed how much
/// room the card needs. The resize reuses the theme resolved here, so nothing
/// is resolved twice, and runs only when something the window is built from
/// changed. See [`crate::overlay::update_overlay_position_for_theme`].
///
/// Never call this from inside a `run_on_main_thread` closure. Every native
/// call it reaches hops to the main thread itself.
pub fn deliver(app: &AppHandle, resolved: &ResolvedOverlayTheme) {
    use tauri_specta::Event;

    // The two values the native steps below consume, logged before they are
    // applied so "the overlay is the wrong size" can be read out of the log.
    debug!(
        "Delivering overlay theme: material={:?}, size_scale={}",
        resolved.effective_material,
        resolved.theme.size_scale()
    );

    record_delivery(resolved);

    if let Err(error) = resolved.emit(app) {
        warn!("Failed to emit the resolved overlay theme: {error}");
    }

    deliver_native(app, resolved);
}

/// The last theme [`deliver`] sent, so a re-read resolving to it stays quiet.
///
/// The watcher sees Handy's own writes too. A commit writes, re-reads and
/// delivers; the watcher's event lands a moment later on the same document,
/// and this makes that pass a no-op, not a second repaint and window resize.
static LAST_DELIVERED: std::sync::Mutex<Option<ResolvedOverlayTheme>> = std::sync::Mutex::new(None);

/// Deliver only when this is not the theme already on screen.
///
/// What the watcher calls. Returns whether anything went out, which it logs.
pub fn deliver_if_changed(app: &AppHandle, resolved: &ResolvedOverlayTheme) -> bool {
    if !is_new_delivery(resolved) {
        return false;
    }
    deliver(app, resolved);
    true
}

/// Whether `resolved` differs from the last delivery. Reads the record without
/// updating it, so the caller decides.
fn is_new_delivery(resolved: &ResolvedOverlayTheme) -> bool {
    match LAST_DELIVERED.lock() {
        Ok(last) => last.as_ref() != Some(resolved),
        // Nothing panics while holding this lock, so this is unreachable in
        // practice; delivering again beats propagating a panic to the overlay.
        Err(poisoned) => poisoned.into_inner().as_ref() != Some(resolved),
    }
}

/// Remember what was delivered. Only [`deliver`] calls this: a draft is not
/// persisted, so it must not make the file's own theme look already-delivered.
fn record_delivery(resolved: &ResolvedOverlayTheme) {
    match LAST_DELIVERED.lock() {
        Ok(mut last) => *last = Some(resolved.clone()),
        Err(poisoned) => *poisoned.into_inner() = Some(resolved.clone()),
    }
}

/// Put a draft on the overlay without persisting it.
///
/// What makes live editing live: the Appearance tab sends the token it is
/// dragging every animation frame and this paints it, while the settling
/// debounce still owns the store. Only the overlay window hears it, since the
/// tab already shows the draft and a push back would fight the control under
/// the finger.
pub fn deliver_draft(app: &AppHandle, resolved: &ResolvedOverlayTheme) {
    use tauri_specta::Event;

    let draft = OverlayThemeDraft {
        resolved: resolved.clone(),
    };
    if let Err(error) = draft.emit_to(app, "recording_overlay") {
        warn!("Failed to emit the overlay theme draft: {error}");
    }

    deliver_native(app, resolved);
}

/// The native half of a delivery: one main-thread hop that applies the
/// Material's window effect and resizes the window, skipped when neither could
/// have changed. The two cannot be separated, the Material being what sets
/// window slack, and so the window size, to zero under Glass.
fn deliver_native(app: &AppHandle, resolved: &ResolvedOverlayTheme) {
    crate::overlay::update_overlay_position_for_theme(app, resolved);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend_source::{
        css_color, css_number, css_px, ts_declaration_block, ts_entry_block, ts_number_field,
        tsx_const, APPLY_LAYER_TS, OVERLAY_CSS, THEME_CSS, WAVEFORM_STYLES_TS,
    };
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

    /// The inherit shape makes "defaults reproduce today's overlay" an
    /// invariant rather than a transcription of the current CSS. An unset
    /// token writes no custom property.
    #[test]
    fn default_overlay_theme_is_all_inherit() {
        let theme = OverlayTheme::default();

        assert_eq!(theme.accent, None);
        assert_eq!(theme.surface, None);
        assert_eq!(theme.surface_opacity, None);
        assert_eq!(theme.glass_tint, None);
        assert_eq!(theme.text, None);
        assert_eq!(theme.border, None);
        assert_eq!(theme.border_opacity, None);
        assert_eq!(theme.material, None);
        assert_eq!(theme.glass_material, None);
        assert_eq!(theme.glass_style, None);
        assert_eq!(theme.shadow_strength, None);
        assert_eq!(theme.shadow_offset_y, None);
        assert_eq!(theme.show_waveform, None);
        assert_eq!(theme.show_cancel, None);
        assert_eq!(theme.size_scale, None);
        assert_eq!(theme.radius, None);
        assert_eq!(theme.border_width, None);
        assert_eq!(theme.padding, None);
        assert_eq!(theme.element_gap, None);
        assert_eq!(theme.waveform_gap, None);
        assert_eq!(theme.waveform_width, None);

        // The accessors' inherit values.
        assert_eq!(theme.size_scale(), 1.0);
        assert_eq!(theme.material(), Material::Flat);
        assert_eq!(theme.glass_material(), GlassMaterial::HudWindow);
        assert_eq!(theme.glass_style(), GlassStyle::Regular);
        assert_eq!(theme.border_width(), 1);
        assert_eq!(theme.padding(), 10);
        assert_eq!(theme.waveform_gap(), 3);
        assert_eq!(theme.waveform_width(), 4);
        // The one token whose inherit differs per Material: no shadow on
        // today's Flat card, macOS's own on today's Glass one.
        assert_eq!(theme.shadow_strength(Material::Flat), 0.0);
        assert_eq!(theme.shadow_strength(Material::Glass), 1.0);
        assert_eq!(theme.shadow_offset_y(), 4);
        assert_eq!(theme.element_gap(), 0);
        assert!(theme.show_waveform());
        assert!(theme.show_cancel());

        // A store written before this field existed, and an explicit
        // all-null document, are the same thing.
        let missing: OverlayTheme =
            serde_json::from_value(json!({})).expect("every token needs a serde default");
        assert_eq!(missing, theme);

        let explicit_nulls: OverlayTheme = serde_json::from_value(json!({
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
        }))
        .expect("null is the explicit spelling of inherit");
        assert_eq!(explicit_nulls, theme);
    }

    /// The liquid engine paints the surface tint itself, so this is the only
    /// place the `surface`/`glass_tint` pair becomes a colour outside CSS. A
    /// set colour wins; an unset one inherits the app background for the
    /// appearance on screen, keeping a dark card dark over a bright desktop.
    #[test]
    fn the_liquid_tint_is_the_surface_at_the_glass_tint() {
        let set = liquid_tint(HexColor::parse("#7aa2f7").as_ref(), Some(0.5), false)
            .expect("a set surface always tints");
        assert!((set.red - 0x7a as f64 / 255.0).abs() < 1e-9);
        assert!((set.green - 0xa2 as f64 / 255.0).abs() < 1e-9);
        assert!((set.blue - 0xf7 as f64 / 255.0).abs() < 1e-9);
        assert_eq!(set.alpha, 0.5);

        // The app theme picks the inherited colour, and nothing else does.
        let light = liquid_tint(None, Some(0.5), false).expect("inherit still tints");
        let dark = liquid_tint(None, Some(0.5), true).expect("inherit still tints");
        assert!(light.red > dark.red, "{light:?} vs {dark:?}");
        assert_eq!(light.alpha, dark.alpha);
        assert_eq!(
            light,
            liquid_tint(
                HexColor::parse(INHERIT_SURFACE_LIGHT).as_ref(),
                Some(0.5),
                true
            )
            .expect("the inherited colour is a colour like any other")
        );
    }

    /// The alpha rules the Appearance tab's slider and the theme file both
    /// have to agree with.
    #[test]
    fn the_liquid_tint_alpha_inherits_clamps_and_can_vanish() {
        assert_eq!(
            liquid_tint(None, None, false).map(|tint| tint.alpha),
            Some(GLASS_TINT_INHERIT)
        );
        // Above the contract's ceiling and non-finite values cannot reach an
        // `NSColor`, so one is clamped and the other inherits.
        assert_eq!(
            liquid_tint(None, Some(4.0), false).map(|tint| tint.alpha),
            Some(GLASS_TINT_MAX)
        );
        assert_eq!(
            liquid_tint(None, Some(f64::NAN), false).map(|tint| tint.alpha),
            Some(GLASS_TINT_INHERIT)
        );
        // Zero is untinted glass, Apple's own look, not a colour with no
        // alpha, so the setter is handed nil. Unlike the surface's 0.30 floor,
        // a theme can ask for this one.
        assert_eq!(GLASS_TINT_MIN, 0.00);
        assert_eq!(liquid_tint(None, Some(0.0), false), None);
        assert_eq!(liquid_tint(None, Some(-1.0), false), None);
    }

    /// The split this token exists for. An opaque card under Flat must not
    /// follow the user into Glass. `surface_opacity` is not an input to the
    /// native tint, so the theme below, what a user who set Flat to 1.00 then
    /// picked Glass has, composes the glassy default, not an opaque pane.
    #[test]
    fn an_opaque_flat_surface_does_not_reach_the_glass_tint() {
        use crate::overlay_glass::GlassAppearance;

        let theme = OverlayTheme {
            surface: hex("#000000"),
            surface_opacity: Some(1.0),
            material: Some(Material::Glass),
            ..Default::default()
        };

        let appearance = GlassAppearance::from_theme(&theme);
        assert_eq!(appearance.glass_tint, None);
        assert_eq!(
            liquid_tint(appearance.surface.as_ref(), appearance.glass_tint, true)
                .map(|tint| tint.alpha),
            Some(GLASS_TINT_INHERIT)
        );

        // …and a Glass tint the user does set is the alpha, whatever the Flat
        // opacity beside it says.
        let tinted = GlassAppearance::from_theme(&OverlayTheme {
            glass_tint: Some(0.15),
            ..theme
        });
        assert_eq!(
            liquid_tint(None, tinted.glass_tint, false).map(|tint| tint.alpha),
            Some(0.15)
        );
    }

    /// Salvage tier one: a token whose value is unusable inherits, and the
    /// other twenty survive untouched.
    #[test]
    fn one_bad_token_inherits_and_keeps_its_siblings() {
        let parsed: OverlayTheme = serde_json::from_value(json!({
            "accent": 5,                  // wrong JSON type
            "surface": "#1a1b26",
            "surface_opacity": 0.92,
            "glass_tint": 0.45,
            "text": "rebeccapurple",      // a CSS colour name, not a hex value
            "border": "#ffffff",
            "border_opacity": 0.25,
            "material": "Glass",          // the store's enums are case-sensitive
            "glass_material": "HUDWindow", // likewise
            "glass_style": "Clear",        // likewise
            "size_scale": 1.1,
            "radius": 12.5,               // a float where integer px is required
            "border_width": 2,
            "padding": 14,
            "waveform_gap": 2,
            "waveform_width": 5
        }))
        .expect("one bad token must never fail the whole theme");

        assert_eq!(parsed.accent, None);
        assert_eq!(parsed.text, None);
        assert_eq!(parsed.material, None);
        assert_eq!(parsed.glass_material, None);
        assert_eq!(parsed.glass_style, None);
        assert_eq!(parsed.radius, None);

        assert_eq!(parsed.surface, hex("#1a1b26"));
        assert_eq!(parsed.surface_opacity, Some(0.92));
        assert_eq!(parsed.glass_tint, Some(0.45));
        assert_eq!(parsed.border, hex("#ffffff"));
        assert_eq!(parsed.border_opacity, Some(0.25));
        assert_eq!(parsed.size_scale, Some(1.1));
        assert_eq!(parsed.border_width, Some(2));
        assert_eq!(parsed.padding, Some(14));
        assert_eq!(parsed.waveform_gap, Some(2));
        assert_eq!(parsed.waveform_width, Some(5));
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

    /// The bounds here are the literals from the token contract, not the
    /// module's constants, so a mistyped constant fails this test rather than
    /// redefining what it checks.
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

    /// `change_overlay_theme_setting` skips the write, the broadcast and the
    /// delivery when handed what is already stored, which rests on clamping
    /// being idempotent. If `normalized` moved a value twice, a re-sent theme
    /// would never compare equal, so the skip would never fire and an
    /// out-of-range value would be stored once, then "changed" on every
    /// commit.
    #[test]
    fn normalizing_an_already_stored_theme_changes_nothing() {
        let raw = OverlayTheme {
            accent: hex("#7AA2F7"),
            surface_opacity: Some(2.0),
            size_scale: Some(3.0),
            radius: Some(99),
            ..Default::default()
        };
        let stored = raw.normalized();
        assert_eq!(stored.normalized(), stored);
        assert_ne!(raw, stored, "the test value must actually be clamped");
    }

    #[test]
    fn normalized_clamps_every_token() {
        let over = OverlayTheme {
            accent: hex("#7aa2f7"),
            surface: hex("#1a1b26"),
            surface_opacity: Some(2.0),
            glass_tint: Some(2.0),
            text: hex("#c0caf5"),
            border: hex("#ffffff"),
            border_opacity: Some(2.0),
            material: Some(Material::Glass),
            glass_material: Some(GlassMaterial::Menu),
            glass_style: Some(GlassStyle::Clear),
            shadow_strength: Some(2.0),
            shadow_offset_y: Some(99),
            show_waveform: Some(false),
            show_cancel: Some(false),
            size_scale: Some(3.0),
            radius: Some(99),
            border_width: Some(99),
            padding: Some(99),
            element_gap: Some(99),
            waveform_style: Some(WaveformStyle::Motes),
            waveform_gap: Some(99),
            waveform_width: Some(99),
        }
        .normalized();

        assert_eq!(over.surface_opacity, Some(1.00));
        assert_eq!(over.glass_tint, Some(1.00));
        assert_eq!(over.border_opacity, Some(1.00));
        assert_eq!(over.size_scale, Some(1.50));
        assert_eq!(over.radius, Some(32));
        assert_eq!(over.border_width, Some(4));
        assert_eq!(over.padding, Some(20));
        assert_eq!(over.element_gap, Some(40));
        assert_eq!(over.waveform_gap, Some(5));
        assert_eq!(over.waveform_width, Some(6));
        assert_eq!(over.shadow_strength, Some(1.00));
        assert_eq!(over.shadow_offset_y, Some(16));
        // Booleans have no range to clamp; they only survive.
        assert_eq!(over.show_waveform, Some(false));
        assert_eq!(over.show_cancel, Some(false));
        assert_eq!(over.waveform_style, Some(WaveformStyle::Motes));
        // Colours and the enum are already canonical; clamping leaves them be.
        assert_eq!(over.accent, hex("#7aa2f7"));
        assert_eq!(over.surface, hex("#1a1b26"));
        assert_eq!(over.text, hex("#c0caf5"));
        assert_eq!(over.border, hex("#ffffff"));
        assert_eq!(over.material, Some(Material::Glass));
        assert_eq!(over.glass_material, Some(GlassMaterial::Menu));
        assert_eq!(over.glass_style, Some(GlassStyle::Clear));

        let under = OverlayTheme {
            surface_opacity: Some(0.1),
            glass_tint: Some(-0.5),
            border_opacity: Some(-0.5),
            size_scale: Some(0.1),
            waveform_width: Some(0),
            shadow_strength: Some(-0.5),
            ..Default::default()
        }
        .normalized();
        assert_eq!(under.surface_opacity, Some(0.30));
        // border_opacity's and glass_tint's floor is 0, unlike the surface's.
        // An invisible edge and untinted glass are legitimate themes, an
        // invisible Flat card is not.
        assert_eq!(under.border_opacity, Some(0.00));
        assert_eq!(under.glass_tint, Some(0.00));
        assert_eq!(under.size_scale, Some(0.80));
        assert_eq!(under.waveform_width, Some(2));
        // The shadow's floor is 0 too. "No shadow" is the value Flat inherits.
        assert_eq!(under.shadow_strength, Some(0.00));

        // Unset stays unset. Clamping must never invent a value, or every
        // token would start writing a custom property.
        assert_eq!(
            OverlayTheme::default().normalized(),
            OverlayTheme::default()
        );

        // A non-finite float has no place on the scale, so it inherits rather
        // than clamping to a bound, and the apply layer writes no custom
        // property for it.
        let non_finite = OverlayTheme {
            surface_opacity: Some(f64::NAN),
            glass_tint: Some(f64::NAN),
            size_scale: Some(f64::INFINITY),
            shadow_strength: Some(f64::NAN),
            ..Default::default()
        }
        .normalized();
        assert_eq!(non_finite.surface_opacity, None);
        assert_eq!(non_finite.glass_tint, None);
        assert_eq!(non_finite.size_scale, None);
        assert_eq!(non_finite.size_scale(), 1.0);
        assert_eq!(non_finite.shadow_strength, None);
        assert_eq!(non_finite.shadow_strength(Material::Flat), 0.0);
        // `to_value(..).is_ok()` would not do here, because serde_json turns a
        // non-finite float into `null` and would pass even without the drop.
        // What matters is that the token is gone, so it serializes as absent.
        assert_eq!(
            serde_json::to_value(&non_finite).expect("a normalized theme serializes"),
            serde_json::to_value(OverlayTheme::default()).expect("the default serializes")
        );
    }

    /// The border width is the second token the native window geometry reads,
    /// so its inherit value and clamp are pinned like `size_scale`'s, with the
    /// literals from the token table rather than the module's constants.
    #[test]
    fn border_width_inherits_one_and_clamps_to_four() {
        let width = |value: Option<u16>| {
            OverlayTheme {
                border_width: value,
                ..Default::default()
            }
            .border_width()
        };

        assert_eq!(width(None), 1);
        assert_eq!(width(Some(0)), 0);
        assert_eq!(width(Some(4)), 4);
        assert_eq!(width(Some(99)), 4);
    }

    /// The Glass material is a closed enum whose theme-file spelling is its
    /// serde representation; the tab, the file and the bindings all read the
    /// same eight strings, so drift in either direction has to fail here.
    #[test]
    fn glass_material_keys_are_the_serde_spelling() {
        for material in GlassMaterial::ALL {
            let value = serde_json::to_value(material).expect("an enum serializes");
            assert_eq!(value, json!(material.as_key()));
            let parsed: GlassMaterial =
                serde_json::from_value(value).expect("the spelling round-trips");
            assert_eq!(parsed, material);
        }

        assert_eq!(
            GlassMaterial::ALL.map(|material| material.as_key()),
            [
                "hud_window",
                "popover",
                "menu",
                "sidebar",
                "under_window_background",
                "sheet",
                "tooltip",
                "content_background",
            ]
        );
        // The default is the measured pick, the most see-through of the eight
        // in both app themes.
        assert_eq!(GlassMaterial::default(), GlassMaterial::HudWindow);
    }

    /// The waveform style is the third closed enum, so it owes the same
    /// round-trip: the key the theme file spells is the serde representation
    /// is the value the bindings carry.
    #[test]
    fn waveform_style_keys_are_the_serde_spelling() {
        for style in WaveformStyle::ALL {
            let value = serde_json::to_value(style).expect("an enum serializes");
            assert_eq!(value, json!(style.as_key()));
            let parsed: WaveformStyle =
                serde_json::from_value(value).expect("the spelling round-trips");
            assert_eq!(parsed, style);
        }

        assert_eq!(
            WaveformStyle::ALL.map(|style| style.as_key()),
            ["bars", "ribbon", "bloom", "motes", "matrix", "steps"]
        );
        // Unset is today's card, so the default has to be the DOM bars.
        assert_eq!(WaveformStyle::default(), WaveformStyle::Bars);
        // No accessor beside the other tokens': the one token nothing native
        // reads, so the frontend resolves the inherit itself
        // (`waveformStyleToken` in `src/lib/overlayTheme.ts`) and Rust only
        // carries and clamps it.
        assert_eq!(OverlayTheme::default().waveform_style, None);
    }

    /// The six styles are declared three times: this enum, the apply layer's
    /// value list (which re-validates a hand-edited localStorage mirror), and
    /// the renderers' token table (which lengths each style reads, and so which
    /// rows the tab shows). A value added here alone would paint an empty lane,
    /// so all three are pinned to each other.
    #[test]
    fn the_waveform_styles_match_the_frontends() {
        let list = {
            let start = APPLY_LAYER_TS
                .find("export const WAVEFORM_STYLES")
                .expect("the apply layer declares the style list");
            let rest = &APPLY_LAYER_TS[start..];
            let end = rest.find("];").expect("the style list is closed");
            &rest[..end]
        };
        let table = ts_declaration_block(WAVEFORM_STYLES_TS, "WAVEFORM_STYLE_TOKENS");

        // In order, so the dropdown lists them the way this enum declares them.
        let mut searched = 0;
        for style in WaveformStyle::ALL {
            let quoted = format!("\"{}\"", style.as_key());
            let at = list[searched..].find(&quoted).unwrap_or_else(|| {
                panic!("{quoted} is missing from the apply layer's style list, or is out of order")
            });
            searched += at + quoted.len();

            let entry = ts_entry_block(table, style.as_key());
            assert!(
                entry.contains("usesWidth:") && entry.contains("usesGap:"),
                "{} has no width/gap entry in the renderers' table",
                style.as_key()
            );
        }
    }

    /// The file is the theme. Nothing merges over it and nothing under it, so
    /// the resolved tokens are the document's own, clamped, and a document
    /// that sets nothing is today's overlay.
    ///
    /// This is what "the resolver no longer reads the settings store" looks
    /// like from the outside: [`resolve_from`] has no second token set to take
    /// one from. The store's `overlay_theme` is read in exactly one place now,
    /// `overlay_theme_write::migrate_once`.
    #[test]
    fn the_resolved_theme_is_the_files_own_tokens() {
        let mut file = ThemeFileState::absent_at("/tmp/overlay_theme.json");
        file.present = true;
        file.owned_keys = vec!["accent".to_string(), "glass_material".to_string()];
        file.tokens = OverlayTheme {
            accent: hex("#7aa2f7"),
            // The one token that never had a row in the Appearance tab. The
            // file was always its only source, and now it is every token's.
            glass_material: Some(GlassMaterial::Popover),
            show_waveform: Some(false),
            shadow_strength: Some(0.35),
            ..Default::default()
        };

        let resolved = resolve_from(
            file.tokens.clone(),
            file.clone(),
            NO_GLASS,
            15.0,
            /* watching */ true,
        );

        assert_eq!(resolved.theme.accent, hex("#7aa2f7"));
        assert_eq!(resolved.theme.glass_material, Some(GlassMaterial::Popover));
        // A falsy value is a value, not an absence.
        assert_eq!(resolved.theme.show_waveform, Some(false));
        assert_eq!(resolved.theme.shadow_strength, Some(0.35));
        // Everything the document is silent about still inherits.
        assert_eq!(resolved.theme.surface, None);
        assert_eq!(resolved.theme.radius, None);
        // The file state rides through untouched, and the watcher's flag with
        // it, which is what takes the tab's Reload button away.
        assert_eq!(resolved.file, file);
        assert!(resolved.watching);

        // No file at all is today's overlay, every token inherited.
        let bare = ThemeFileState::absent_at("/tmp/overlay_theme.json");
        let resolved = resolve_from(bare.tokens.clone(), bare, NO_GLASS, 15.0, false);
        assert_eq!(resolved.theme, OverlayTheme::default());
        assert!(!resolved.watching);
    }

    /// Glass is unavailable: what this build reports off macOS, and before the
    /// native effect view is installed.
    const NO_GLASS: GlassSupport = GlassSupport {
        supported: true,
        available: false,
        engine: GlassEngine::VisualEffect,
    };

    /// The resolver clamps once, and a Glass request renders Flat while
    /// `available` is false without losing the request.
    #[test]
    fn resolve_clamps_once_and_downgrades_glass_when_unavailable() {
        let tokens = OverlayTheme {
            accent: hex("#7aa2f7"),
            surface_opacity: Some(0.05),
            glass_tint: Some(1.9),
            material: Some(Material::Glass),
            size_scale: Some(9.0),
            radius: Some(99),
            ..Default::default()
        };

        let mut file = ThemeFileState::absent_at("");
        file.present = true;
        file.tokens = tokens.clone();

        let resolved = resolve_from(tokens.clone(), file, NO_GLASS, 15.0, false);

        // Every out-of-range value is moved to its bound in one pass.
        assert_eq!(resolved.theme.size_scale, Some(1.50));
        assert_eq!(resolved.theme.size_scale(), 1.50);
        assert_eq!(resolved.theme.surface_opacity, Some(0.30));
        assert_eq!(resolved.theme.glass_tint, Some(1.00));
        assert_eq!(resolved.theme.radius, Some(32));
        assert_eq!(resolved.theme.accent, hex("#7aa2f7"));

        // The request survives verbatim; only what is rendered is downgraded,
        // so turning Glass back on never has to re-ask the user.
        assert_eq!(resolved.theme.material, Some(Material::Glass));
        assert_eq!(resolved.effective_material, Material::Flat);
        assert_eq!(resolved.glass_support, NO_GLASS);

        // Same inputs, Glass actually available: now it renders.
        let available = GlassSupport {
            supported: true,
            available: true,
            engine: GlassEngine::Liquid,
        };
        let rendered = resolve_from(
            tokens,
            ThemeFileState::absent_at(""),
            available,
            15.0,
            false,
        );
        assert_eq!(rendered.effective_material, Material::Glass);
    }

    /// The gate that stops Handy's own write repainting twice. A commit
    /// delivers, the watcher then reads the identical document back, and the
    /// second pass has nothing to say.
    #[test]
    fn a_theme_already_delivered_is_not_delivered_again() {
        let file = ThemeFileState::absent_at("/tmp/overlay_theme.json");
        let resolved = resolve_from(file.tokens.clone(), file, NO_GLASS, 15.0, true);

        // Left exactly as found, because this record is process-wide.
        let previous = LAST_DELIVERED
            .lock()
            .map(|last| last.clone())
            .unwrap_or_default();

        record_delivery(&resolved);
        assert!(
            !is_new_delivery(&resolved),
            "the watcher re-reading Handy's own write has nothing to deliver"
        );

        // A token moving is a new theme, even by one clamped step.
        let mut changed = resolved.clone();
        changed.theme.radius = Some(12);
        assert!(is_new_delivery(&changed));

        // So is a diagnostic appearing, with the tokens unchanged: the tab has
        // an alert to show that it does not have yet.
        let mut broken = resolved;
        broken.file.stale = true;
        assert!(is_new_delivery(&broken));

        if let Ok(mut last) = LAST_DELIVERED.lock() {
            *last = previous;
        }
    }

    /// The resolved theme carries the one number the overlay page cannot work
    /// out: how far its window reaches past the card towards the anchored edge.
    ///
    /// Derived here, beside the effective Material, because the same resolve is
    /// what the window is sized and placed from and what the page paints, and
    /// the two must inset the card by the same integer or the card moves.
    #[test]
    fn a_resolved_theme_carries_the_shadows_anchored_side_slack() {
        let flat = GlassSupport {
            supported: false,
            available: false,
            engine: GlassEngine::None,
        };
        let glass = GlassSupport {
            supported: true,
            available: true,
            engine: GlassEngine::Liquid,
        };
        let file = ThemeFileState::absent_at("");
        let shadowed = OverlayTheme {
            shadow_strength: Some(0.5),
            shadow_offset_y: Some(4),
            ..Default::default()
        };

        // A Flat card casting a 24 point shadow, in each of the rooms the three
        // platforms' offsets can offer.
        for (room, expected) in [(0.0, 0.0), (4.0, 4.0), (15.0, 15.0), (40.0, 24.0)] {
            assert_eq!(
                resolve_from(shadowed.clone(), file.clone(), flat, room, false).shadow_edge_slack,
                expected,
                "{room} points of room"
            );
        }

        // Nothing is taken with no shadow to make room for, keeping an
        // untouched overlay's window byte-identical…
        assert_eq!(
            resolve_from(OverlayTheme::default(), file.clone(), flat, 40.0, false)
                .shadow_edge_slack,
            0.0
        );
        // …nor under Glass, where the shadow is macOS's own, outside a window
        // the card fills exactly. It is the *rendered* Material that decides,
        // so a Glass request downgraded to Flat still makes room.
        let wants_glass = OverlayTheme {
            material: Some(Material::Glass),
            ..shadowed.clone()
        };
        assert_eq!(
            resolve_from(wants_glass.clone(), file.clone(), glass, 40.0, false).shadow_edge_slack,
            0.0
        );
        assert_eq!(
            resolve_from(wants_glass, file, flat, 40.0, false).shadow_edge_slack,
            24.0
        );
    }

    #[test]
    fn effective_material_downgrades_glass_when_unavailable() {
        let unavailable = GlassSupport {
            supported: true,
            available: false,
            engine: GlassEngine::VisualEffect,
        };
        let available = GlassSupport {
            supported: true,
            available: true,
            engine: GlassEngine::Liquid,
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

    /// The liquid engine tints the glass in Rust, so the colour an unset
    /// `surface` inherits must match what the apply layer's
    /// `var(--color-background)` resolves to. The palette is the source;
    /// these two constants are the copy that follows it.
    #[test]
    fn inherit_surface_matches_the_app_palette() {
        assert_eq!(
            css_color(THEME_CSS, "--light-color-background"),
            INHERIT_SURFACE_LIGHT
        );
        assert_eq!(
            css_color(THEME_CSS, "--dark-color-background"),
            INHERIT_SURFACE_DARK
        );
    }

    /// The shadow's strength is the second token with a per-Material inherit,
    /// after the border's alpha, and the only one whose two inherits are its
    /// range's ends: Flat has never had a shadow, Glass has always had macOS's.
    #[test]
    fn the_shadow_strength_inherits_per_material_and_clamps() {
        let strength = |value: Option<f64>, material: Material| {
            OverlayTheme {
                shadow_strength: value,
                ..Default::default()
            }
            .shadow_strength(material)
        };

        assert_eq!(strength(None, Material::Flat), 0.00);
        assert_eq!(strength(None, Material::Glass), 1.00);
        // A set value is that value on both Materials; only the inherit splits.
        for material in [Material::Flat, Material::Glass] {
            assert_eq!(strength(Some(0.00), material), 0.00);
            assert_eq!(strength(Some(0.35), material), 0.35);
            assert_eq!(strength(Some(1.00), material), 1.00);
            assert_eq!(strength(Some(9.0), material), 1.00);
            assert_eq!(strength(Some(-9.0), material), 0.00);
        }
        // A non-finite value is no strength at all, so it inherits.
        assert_eq!(strength(Some(f64::NAN), Material::Glass), 1.00);
        assert_eq!(strength(Some(f64::INFINITY), Material::Flat), 0.00);
    }

    /// The offset, the gap and the two visibility switches, at their inherit
    /// values and at their bounds. The literals are the token table's, not the
    /// module's constants.
    #[test]
    fn the_shadow_offset_gap_and_switches_inherit_todays_row() {
        assert_eq!(OverlayTheme::default().shadow_offset_y(), 4);
        assert_eq!(
            OverlayTheme {
                shadow_offset_y: Some(99),
                ..Default::default()
            }
            .shadow_offset_y(),
            16
        );
        assert_eq!(OverlayTheme::default().element_gap(), 0);
        assert_eq!(
            OverlayTheme {
                element_gap: Some(99),
                ..Default::default()
            }
            .element_gap(),
            40
        );

        // Unset means shown, so a theme that says nothing draws today's row.
        assert!(OverlayTheme::default().show_waveform());
        assert!(OverlayTheme::default().show_cancel());
        assert!(!OverlayTheme {
            show_waveform: Some(false),
            ..Default::default()
        }
        .show_waveform());
        assert!(!OverlayTheme {
            show_cancel: Some(false),
            ..Default::default()
        }
        .show_cancel());
    }

    /// The token contract's bounds are written twice: here, where Rust clamps
    /// the store, the theme file and the native geometry, and in
    /// `OVERLAY_TOKEN_BOUNDS` (`src/lib/overlayTheme.ts`), where TypeScript
    /// re-validates the localStorage mirror and draws every slider. Specta
    /// exports types, not constants, so nothing can generate one from the
    /// other and this reads the TypeScript, naming the token that drifted.
    ///
    /// Without it, a bound widened on one side is a slider producing values
    /// the backend silently clamps, unseen until someone drags to the end.
    #[test]
    fn token_bounds_match_the_apply_layers_table() {
        let bounds = ts_declaration_block(APPLY_LAYER_TS, "OVERLAY_TOKEN_BOUNDS");
        let min = |token: &str| ts_number_field(ts_entry_block(bounds, token), "min");
        let max = |token: &str| ts_number_field(ts_entry_block(bounds, token), "max");

        assert_eq!(min("surface_opacity"), SURFACE_OPACITY_MIN);
        assert_eq!(max("surface_opacity"), SURFACE_OPACITY_MAX);
        assert_eq!(min("glass_tint"), GLASS_TINT_MIN);
        assert_eq!(max("glass_tint"), GLASS_TINT_MAX);
        assert_eq!(min("border_opacity"), BORDER_OPACITY_MIN);
        assert_eq!(max("border_opacity"), BORDER_OPACITY_MAX);
        assert_eq!(min("size_scale"), SIZE_SCALE_MIN);
        assert_eq!(max("size_scale"), SIZE_SCALE_MAX);
        assert_eq!(min("radius"), 0.0);
        assert_eq!(max("radius"), f64::from(RADIUS_MAX));
        assert_eq!(min("border_width"), 0.0);
        assert_eq!(max("border_width"), f64::from(BORDER_WIDTH_MAX));
        assert_eq!(min("padding"), 0.0);
        assert_eq!(max("padding"), f64::from(PADDING_MAX));
        assert_eq!(min("waveform_gap"), 0.0);
        assert_eq!(max("waveform_gap"), f64::from(WAVEFORM_GAP_MAX));
        assert_eq!(min("waveform_width"), f64::from(WAVEFORM_WIDTH_MIN));
        assert_eq!(max("waveform_width"), f64::from(WAVEFORM_WIDTH_MAX));
        assert_eq!(min("shadow_strength"), SHADOW_STRENGTH_MIN);
        assert_eq!(max("shadow_strength"), SHADOW_STRENGTH_MAX);
        assert_eq!(min("shadow_offset_y"), 0.0);
        assert_eq!(max("shadow_offset_y"), f64::from(SHADOW_OFFSET_Y_MAX));
        assert_eq!(min("element_gap"), 0.0);
        assert_eq!(max("element_gap"), f64::from(ELEMENT_GAP_MAX));

        // ...and neither table has a token the other lacks. The twelve
        // asserted above are every numeric token there is, on both sides.
        assert_eq!(
            bounds.matches("step:").count(),
            12,
            "a numeric token gained or lost a bound in the apply layer"
        );
    }

    /// What an unset numeric token inherits is declared in two places: the
    /// `:root` block of `RecordingOverlay.css`, which paints it, and
    /// `STATIC_NUMERIC_INHERIT` in the apply layer, which the Appearance tab's
    /// sliders show while the token is unset. A slider showing 24 for a card
    /// drawn at 20 lies silently, so the stylesheet is the source and this is
    /// the pin.
    ///
    /// The two alphas are absent from the CSS by design. `surface_opacity` is
    /// folded into `--s-surface`'s own `color-mix` and `glass_tint` is measured
    /// rather than declared, so both are pinned to the apply layer's exported
    /// constants, where the composition reads them.
    #[test]
    fn overlay_token_inherit_values_match_the_css() {
        let inherit = ts_declaration_block(APPLY_LAYER_TS, "STATIC_NUMERIC_INHERIT");
        for (token, property) in [
            ("radius", "--ov-radius"),
            ("border_width", "--ov-border-w"),
            ("padding", "--ov-pad"),
            ("waveform_gap", "--ov-wave-gap"),
            ("waveform_width", "--ov-wave-w"),
            ("shadow_offset_y", "--ov-shadow-y"),
            ("element_gap", "--ov-elem-gap"),
        ] {
            assert_eq!(
                ts_number_field(inherit, token),
                css_px(OVERLAY_CSS, property),
                "{token} and {property} have drifted"
            );
        }
        assert_eq!(
            ts_number_field(inherit, "size_scale"),
            css_number(OVERLAY_CSS, "--ov-scale")
        );

        // The two the stylesheet cannot declare read the apply layer's
        // exported constants rather than repeating a number, so the pin is
        // that they still do, and for the tint that Rust's copy agrees.
        for entry in [
            "surface_opacity: SURFACE_OPACITY_INHERIT",
            "glass_tint: GLASS_TINT_INHERIT",
        ] {
            assert!(
                inherit.contains(entry),
                "the apply layer no longer inherits `{entry}`"
            );
        }
        assert_eq!(
            tsx_const(APPLY_LAYER_TS, "export const GLASS_TINT_INHERIT = "),
            GLASS_TINT_INHERIT
        );

        // Clear glass's rim. Rust paints no border, so these two are a pin,
        // not a twin. They are the numbers the README quotes, measured against
        // Spotlight's own capsule, and this stops them drifting out of the one
        // file that composes them.
        assert!(
            APPLY_LAYER_TS.contains("export const BORDER_INHERIT_CLEAR = \"#ffffff\";"),
            "Clear glass no longer inherits a white rim"
        );
        assert_eq!(
            tsx_const(
                APPLY_LAYER_TS,
                "export const BORDER_OPACITY_INHERIT_CLEAR = "
            ),
            0.35
        );

        // The three Rust also owns, because the native geometry is built from
        // them.
        assert_eq!(
            ts_number_field(inherit, "border_width"),
            f64::from(BORDER_WIDTH_INHERIT)
        );
        assert_eq!(
            ts_number_field(inherit, "padding"),
            f64::from(PADDING_INHERIT)
        );
        assert_eq!(
            ts_number_field(inherit, "waveform_width"),
            f64::from(WAVEFORM_WIDTH_INHERIT)
        );
        assert_eq!(
            ts_number_field(inherit, "waveform_gap"),
            f64::from(WAVEFORM_GAP_INHERIT)
        );
        assert_eq!(
            ts_number_field(inherit, "shadow_offset_y"),
            f64::from(SHADOW_OFFSET_Y_INHERIT)
        );
        assert_eq!(
            ts_number_field(inherit, "element_gap"),
            f64::from(ELEMENT_GAP_INHERIT)
        );

        // The shadow's strength cannot sit in that table: its inherit differs
        // per Material, like the border's alpha. Its two numbers pin instead to
        // the apply layer's own record: what the tab shows and paints the card.
        let shadow_inherit = ts_declaration_block(APPLY_LAYER_TS, "SHADOW_STRENGTH_INHERIT");
        for material in [Material::Flat, Material::Glass] {
            let key = match material {
                Material::Flat => "flat",
                Material::Glass => "glass",
            };
            assert_eq!(
                ts_number_field(shadow_inherit, key),
                shadow_strength_inherit(material),
                "the {key} shadow inherit has drifted"
            );
        }
        // Flat's inherit is also the number the stylesheet paints while no
        // token is set, so the two agree that today's Flat card has no shadow.
        assert_eq!(
            css_number(OVERLAY_CSS, "--ov-shadow-strength"),
            shadow_strength_inherit(Material::Flat)
        );
    }
}
