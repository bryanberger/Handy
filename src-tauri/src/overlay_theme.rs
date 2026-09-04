//! Overlay theme: storage, per-key merge, and delivery.
//!
//! Sixteen optional tokens decide how the recording overlay's card looks.
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
/// Highest accepted `edge_margin`, in points. Two hundred points is already a
/// fifth of a laptop screen's height; past that the overlay reads as floating
/// in the middle rather than sitting at an edge.
pub const EDGE_MARGIN_MAX: u16 = 200;
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

/// The `border_width` an unset token inherits, in px at scale 1: today's
/// hairline (`--ov-border-w: 1px`, `RecordingOverlay.css`).
pub const BORDER_WIDTH_INHERIT: u16 = 1;
/// The `padding` an unset token inherits, in px at scale 1: today's inset
/// (`--ov-pad: 10px`, `RecordingOverlay.css`). At this value the control row
/// is the 40 px it has always been and the Live transcript's inset is 12 px.
pub const PADDING_INHERIT: u16 = 10;
/// The `waveform_width` an unset token inherits, in px at scale 1: today's
/// bar (`--ov-wave-w: 4px`, `RecordingOverlay.css`).
///
/// Test-only, unlike [`BORDER_WIDTH_INHERIT`], since a bar's width never
/// reaches the native window geometry. It exists so
/// `overlay_window_constants_match_overlay_css` can pin the stylesheet's
/// declared inherit value to the token table.
#[cfg(test)]
pub const WAVEFORM_WIDTH_INHERIT: u16 = 4;

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

/// The seventeen overlay-theme tokens. `None` means inherit.
///
/// Field names are the theme-file keys, and every field deserializes
/// leniently: a wrong type or shape degrades to `None` with a `warn!`, so one
/// bad token never costs the other sixteen, as `salvage_settings` does one
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
    /// Gap between waveform bars at scale 1, 0 to 5 px.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub waveform_gap: Option<u16>,
    /// Width of each waveform bar at scale 1, 2 to 6 px.
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub waveform_width: Option<u16>,
    /// The gap between the usable screen edge and the card, in points, 0 to
    /// 200. Zero sits the card flush against that edge.
    ///
    /// The one token measured against the screen rather than the card, so it
    /// is *not* multiplied by `size_scale`: the screen does not zoom, and the
    /// card's distance from the edge stays what the user chose at every scale.
    /// Which edge it is measured from follows the `overlay_position` setting,
    /// so an unset value inherits per platform *and* per position; see
    /// [`Self::edge_margin`].
    #[serde(default, deserialize_with = "inherit_on_error")]
    pub edge_margin: Option<u16>,
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

    /// The gap to the usable screen edge in points, for the edge `position`
    /// anchors to: unset ⇒ the per-platform, per-position value that puts the
    /// card where it sits today ([`crate::overlay_geometry::inherit_edge_margin`]),
    /// otherwise clamped to `0..=EDGE_MARGIN_MAX`.
    ///
    /// The single clamp, like [`Self::padding`]. The inherit table lives in
    /// the geometry module with the placement formulas it belongs to, so the
    /// six platform/position pairs are asserted on any host.
    pub fn edge_margin(&self, position: crate::settings::OverlayPosition) -> u16 {
        match self.edge_margin {
            Some(margin) => margin.min(EDGE_MARGIN_MAX),
            None => crate::overlay_geometry::inherit_edge_margin(
                crate::overlay_geometry::Platform::current(),
                position,
            ),
        }
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
            size_scale: clamp_float(
                self.size_scale,
                SIZE_SCALE_MIN,
                SIZE_SCALE_MAX,
                "size_scale",
            ),
            radius: self.radius.map(|value| value.min(RADIUS_MAX)),
            border_width: self.border_width.map(|value| value.min(BORDER_WIDTH_MAX)),
            padding: self.padding.map(|value| value.min(PADDING_MAX)),
            waveform_gap: self.waveform_gap.map(|value| value.min(WAVEFORM_GAP_MAX)),
            waveform_width: self
                .waveform_width
                .map(|value| value.clamp(WAVEFORM_WIDTH_MIN, WAVEFORM_WIDTH_MAX)),
            edge_margin: self.edge_margin.map(|value| value.min(EDGE_MARGIN_MAX)),
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
    /// The file's contribution to the merge.
    pub tokens: OverlayTheme,
    /// The keys the file actually sets. These are the tab's lock markers: a
    /// file-owned token cannot be edited from the settings window.
    pub owned_keys: Vec<String>,
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
    /// `file ?? settings ?? inherit`, per key, clamped. `None` still means
    /// inherit: the apply layer writes no custom property for it.
    pub theme: OverlayTheme,
    /// Concrete, never `None`: the requested Material downgraded to Flat when
    /// Glass is unavailable.
    pub effective_material: Material,
    /// Whether Glass is offerable and whether it can render right now. Read by
    /// the Appearance tab instead of a TypeScript platform check, so the two
    /// sides cannot disagree.
    pub glass_support: GlassSupport,
    /// The gap to the usable screen edge actually in effect, in points: the
    /// `edge_margin` token, or the per-platform, per-position value an unset
    /// one inherits. Concrete, never `None`.
    ///
    /// Carried for the same reason as `glass_support`: the number depends on
    /// the platform and on the anchored edge, and the Appearance tab's slider
    /// has to show it while the token is unset. Shipping the answer keeps a
    /// six-cell platform table out of TypeScript, where it could disagree with
    /// the one the window is placed from.
    pub effective_edge_margin: u16,
    /// What the theme file contributed, including which tokens it owns and what
    /// the reader had to ignore.
    pub file: ThemeFileState,
}

/// Merge two token sets per key: `file` wins wherever it sets a token, and an
/// absent token falls through to `settings` and then to inherit.
///
/// One token breaks that shape: `glass_material` has no row in the Appearance
/// tab, so the merge reads it from the file alone. See its field doc.
///
/// Pure, so the precedence rule is testable without an `AppHandle`.
pub fn merge(file: &OverlayTheme, settings: &OverlayTheme) -> OverlayTheme {
    OverlayTheme {
        accent: file.accent.clone().or_else(|| settings.accent.clone()),
        surface: file.surface.clone().or_else(|| settings.surface.clone()),
        surface_opacity: file.surface_opacity.or(settings.surface_opacity),
        glass_tint: file.glass_tint.or(settings.glass_tint),
        text: file.text.clone().or_else(|| settings.text.clone()),
        border: file.border.clone().or_else(|| settings.border.clone()),
        border_opacity: file.border_opacity.or(settings.border_opacity),
        material: file.material.or(settings.material),
        // No `.or(settings.glass_material)` here. The tab has no control for
        // it, so the store cannot be the source.
        glass_material: file.glass_material,
        glass_style: file.glass_style.or(settings.glass_style),
        size_scale: file.size_scale.or(settings.size_scale),
        radius: file.radius.or(settings.radius),
        border_width: file.border_width.or(settings.border_width),
        padding: file.padding.or(settings.padding),
        waveform_gap: file.waveform_gap.or(settings.waveform_gap),
        waveform_width: file.waveform_width.or(settings.waveform_width),
        edge_margin: file.edge_margin.or(settings.edge_margin),
    }
}

/// Merge `theme file ?? settings ?? inherit` per key and clamp.
///
/// Uses the theme-file cache, warmed by the launch-time read before any window
/// exists, so this does no filesystem IO and is safe on the main thread. (The
/// one cold-cache read inside [`crate::overlay_theme_file::cached`] can only
/// happen before that.)
pub fn resolve(app: &AppHandle) -> ResolvedOverlayTheme {
    resolve_from(
        settings_theme(app),
        crate::overlay_theme_file::cached(app),
        glass_support(app),
        crate::overlay::current_overlay_position(),
    )
}

/// [`resolve`] for a caller that already holds the stored tokens.
///
/// The commit path just wrote the settings-level theme; the preview path was
/// handed a draft that never reaches the store. Neither needs to read it back.
pub fn resolve_with(app: &AppHandle, settings_theme: OverlayTheme) -> ResolvedOverlayTheme {
    resolve_from(
        settings_theme,
        crate::overlay_theme_file::cached(app),
        glass_support(app),
        crate::overlay::current_overlay_position(),
    )
}

/// [`resolve`], preceded by a fresh read of the theme file.
///
/// The authoritative resolve: whatever the file says right now is what the
/// overlay shows. It touches the filesystem, so call it only off the main
/// thread. Its two callers, the overlay show path and Reload, do.
pub fn resolve_reloading(app: &AppHandle) -> ResolvedOverlayTheme {
    resolve_reloading_for(app, settings_theme(app))
}

/// [`resolve_reloading`] for a caller that has already loaded the settings.
///
/// Reading the settings store is a full deserialize plus the migration pass
/// (`settings::get_settings`). The overlay show path has already read them for
/// `overlay_style` and whether to show at all, so it passes the tokens it holds
/// instead of a second read on every recording.
pub fn resolve_reloading_for(
    app: &AppHandle,
    settings_theme: OverlayTheme,
) -> ResolvedOverlayTheme {
    resolve_from(
        settings_theme,
        crate::overlay_theme_file::read(app),
        glass_support(app),
        crate::overlay::current_overlay_position(),
    )
}

/// The overlay theme as persisted, before the theme file and the clamping.
fn settings_theme(app: &AppHandle) -> OverlayTheme {
    crate::settings::get_overlay_theme(app)
}

/// The whole resolution rule with nothing to look up: merge the file over the
/// settings, clamp once, decide the Material actually rendered, and resolve
/// the edge margin against the edge the overlay is anchored to.
///
/// Pure, so the merge order, the clamping, the Glass downgrade and the
/// margin's per-position inherit are testable together without an `AppHandle`.
pub fn resolve_from(
    settings_theme: OverlayTheme,
    file: ThemeFileState,
    support: GlassSupport,
    position: crate::settings::OverlayPosition,
) -> ResolvedOverlayTheme {
    let theme = merge(&file.tokens, &settings_theme).normalized();
    let effective_material = effective_material(theme.material(), support);
    let effective_edge_margin = theme.edge_margin(position);

    ResolvedOverlayTheme {
        theme,
        effective_material,
        glass_support: support,
        file,
        effective_edge_margin,
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
        "Delivering overlay theme: material={:?}, size_scale={}, edge_margin={}",
        resolved.effective_material,
        resolved.theme.size_scale(),
        resolved.effective_edge_margin
    );

    if let Err(error) = resolved.emit(app) {
        warn!("Failed to emit the resolved overlay theme: {error}");
    }

    deliver_native(app, resolved);
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
        tsx_const, APPLY_LAYER_TS, OVERLAY_CSS, THEME_CSS,
    };
    use crate::overlay_geometry::{inherit_edge_margin, Platform};
    use crate::settings::OverlayPosition;
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
        assert_eq!(theme.size_scale, None);
        assert_eq!(theme.radius, None);
        assert_eq!(theme.border_width, None);
        assert_eq!(theme.padding, None);
        assert_eq!(theme.waveform_gap, None);
        assert_eq!(theme.waveform_width, None);
        assert_eq!(theme.edge_margin, None);

        // The accessors' inherit values.
        assert_eq!(theme.size_scale(), 1.0);
        assert_eq!(theme.material(), Material::Flat);
        assert_eq!(theme.glass_material(), GlassMaterial::HudWindow);
        assert_eq!(theme.glass_style(), GlassStyle::Regular);
        assert_eq!(theme.border_width(), 1);
        // The one accessor whose inherit is not a constant: it follows the
        // platform and the edge the overlay is anchored to.
        for position in [OverlayPosition::Top, OverlayPosition::Bottom] {
            assert_eq!(
                theme.edge_margin(position),
                inherit_edge_margin(Platform::current(), position)
            );
        }

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
    /// other fifteen survive untouched.
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
            size_scale: Some(3.0),
            radius: Some(99),
            border_width: Some(99),
            padding: Some(99),
            waveform_gap: Some(99),
            waveform_width: Some(99),
            edge_margin: Some(9999),
        }
        .normalized();

        assert_eq!(over.surface_opacity, Some(1.00));
        assert_eq!(over.glass_tint, Some(1.00));
        assert_eq!(over.border_opacity, Some(1.00));
        assert_eq!(over.size_scale, Some(1.50));
        assert_eq!(over.radius, Some(32));
        assert_eq!(over.border_width, Some(4));
        assert_eq!(over.padding, Some(20));
        assert_eq!(over.waveform_gap, Some(5));
        assert_eq!(over.waveform_width, Some(6));
        assert_eq!(over.edge_margin, Some(EDGE_MARGIN_MAX));
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
            ..Default::default()
        }
        .normalized();
        assert_eq!(non_finite.surface_opacity, None);
        assert_eq!(non_finite.glass_tint, None);
        assert_eq!(non_finite.size_scale, None);
        assert_eq!(non_finite.size_scale(), 1.0);
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
            glass_tint: Some(0.6),
            radius: Some(12),
            border: hex("#ffffff"),
            border_width: Some(3),
            glass_style: Some(GlassStyle::Clear),
            waveform_width: Some(5),
            ..Default::default()
        };

        let merged = merge(&file, &settings);

        // The file wins the keys it sets…
        assert_eq!(merged.accent, hex("#7aa2f7"));
        assert_eq!(merged.size_scale, Some(1.1));
        // …the settings fill the gaps…
        assert_eq!(merged.surface, hex("#1a1b26"));
        assert_eq!(merged.glass_tint, Some(0.6));
        assert_eq!(merged.radius, Some(12));
        assert_eq!(merged.border, hex("#ffffff"));
        assert_eq!(merged.border_width, Some(3));
        assert_eq!(merged.glass_style, Some(GlassStyle::Clear));
        assert_eq!(merged.waveform_width, Some(5));
        // …and a key neither of them sets still inherits.
        assert_eq!(merged.text, None);

        // Merging with an absent file gives the settings back unchanged, for
        // every token the settings can carry, which is all but the one below.
        assert_eq!(merge(&OverlayTheme::default(), &settings), settings);
    }

    /// `glass_material` is the one token the settings store cannot supply. Its
    /// eight-option dropdown left the Appearance tab when Liquid Glass
    /// arrived, so the file is the only source; the field survives only so
    /// documents an older build stored keep deserializing.
    #[test]
    fn the_glass_material_is_taken_from_the_theme_file_alone() {
        let settings = OverlayTheme {
            glass_material: Some(GlassMaterial::Menu),
            radius: Some(12),
            ..Default::default()
        };

        let stored_only = merge(&OverlayTheme::default(), &settings);
        assert_eq!(stored_only.glass_material, None);
        // …and its neighbours in the same struct still fall through.
        assert_eq!(stored_only.radius, Some(12));

        let file = OverlayTheme {
            glass_material: Some(GlassMaterial::Popover),
            ..Default::default()
        };
        assert_eq!(
            merge(&file, &settings).glass_material,
            Some(GlassMaterial::Popover)
        );
    }

    /// The resolver is the only place the three rules meet, so pin them
    /// together: the file outranks the settings, the result is clamped once,
    /// and a Glass request renders Flat while `available` is false, the state
    /// this build ships in before the native Glass module.
    #[test]
    fn resolve_clamps_once_and_downgrades_glass_when_unavailable() {
        let mut file = ThemeFileState::absent_at("");
        file.present = true;
        file.owned_keys = vec!["size_scale".to_string()];
        file.tokens = OverlayTheme {
            size_scale: Some(9.0),
            ..Default::default()
        };

        let settings_theme = OverlayTheme {
            accent: hex("#7aa2f7"),
            surface_opacity: Some(0.05),
            glass_tint: Some(1.9),
            material: Some(Material::Glass),
            size_scale: Some(1.0),
            radius: Some(99),
            ..Default::default()
        };

        let unavailable = GlassSupport {
            supported: true,
            available: false,
            engine: GlassEngine::VisualEffect,
        };
        let resolved = resolve_from(
            settings_theme.clone(),
            file.clone(),
            unavailable,
            OverlayPosition::Bottom,
        );

        // The file's out-of-range value wins the key and is then clamped.
        assert_eq!(resolved.theme.size_scale, Some(1.50));
        assert_eq!(resolved.theme.size_scale(), 1.50);
        // The settings' own out-of-range values are clamped in the same pass.
        assert_eq!(resolved.theme.surface_opacity, Some(0.30));
        assert_eq!(resolved.theme.glass_tint, Some(1.00));
        assert_eq!(resolved.theme.radius, Some(32));
        assert_eq!(resolved.theme.accent, hex("#7aa2f7"));

        // The request survives verbatim; only what is rendered is downgraded,
        // so turning Glass back on never has to re-ask the user.
        assert_eq!(resolved.theme.material, Some(Material::Glass));
        assert_eq!(resolved.effective_material, Material::Flat);
        assert_eq!(resolved.glass_support, unavailable);

        // The file state rides through untouched, drawing the tab's lock
        // markers.
        assert_eq!(resolved.file, file);

        // Same inputs, Glass actually available: now it renders.
        let available = GlassSupport {
            supported: true,
            available: true,
            engine: GlassEngine::Liquid,
        };
        let rendered = resolve_from(
            settings_theme,
            ThemeFileState::absent_at(""),
            available,
            OverlayPosition::Bottom,
        );
        assert_eq!(rendered.effective_material, Material::Glass);
        // With no file, the settings' own scale survives.
        assert_eq!(rendered.theme.size_scale, Some(1.0));
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

    /// The margin token's own rules: it clamps like the other lengths, and an
    /// unset one is *not* one number — it follows the platform and the edge
    /// the overlay is anchored to, so flipping Top/Bottom while nothing is set
    /// keeps today's look on both.
    #[test]
    fn the_edge_margin_clamps_and_inherits_per_position() {
        let unset = OverlayTheme::default();
        assert_eq!(
            unset.edge_margin(OverlayPosition::Top),
            inherit_edge_margin(Platform::current(), OverlayPosition::Top)
        );
        assert_eq!(
            unset.edge_margin(OverlayPosition::Bottom),
            inherit_edge_margin(Platform::current(), OverlayPosition::Bottom)
        );

        // A set value is the same on either edge, and out of range is clamped
        // by the accessor as well as by `normalized`, so nothing downstream
        // has to trust that the store was written through the clamp.
        let set = OverlayTheme {
            edge_margin: Some(64),
            ..Default::default()
        };
        assert_eq!(set.edge_margin(OverlayPosition::Top), 64);
        assert_eq!(set.edge_margin(OverlayPosition::Bottom), 64);

        let over = OverlayTheme {
            edge_margin: Some(9999),
            ..Default::default()
        };
        assert_eq!(over.edge_margin(OverlayPosition::Top), EDGE_MARGIN_MAX);
        assert_eq!(over.normalized().edge_margin, Some(EDGE_MARGIN_MAX));

        // Zero is a value, not "unset": flush against the usable edge.
        let flush = OverlayTheme {
            edge_margin: Some(0),
            ..Default::default()
        };
        assert_eq!(flush.edge_margin(OverlayPosition::Top), 0);
    }

    /// The number the Appearance tab's slider shows while the token is unset
    /// is the number the window is placed from, because both come out of one
    /// resolve. Without this the tab would need its own platform table.
    #[test]
    fn the_resolved_theme_carries_the_margin_actually_in_effect() {
        let support = GlassSupport {
            supported: false,
            available: false,
            engine: GlassEngine::None,
        };

        for position in [OverlayPosition::Top, OverlayPosition::Bottom] {
            let inherited = resolve_from(
                OverlayTheme::default(),
                ThemeFileState::absent_at(""),
                support,
                position,
            );
            assert_eq!(
                inherited.effective_edge_margin,
                inherit_edge_margin(Platform::current(), position)
            );

            // A set token wins on both edges, and the file wins over settings
            // like every other token.
            let set = resolve_from(
                OverlayTheme {
                    edge_margin: Some(72),
                    ..Default::default()
                },
                ThemeFileState::absent_at(""),
                support,
                position,
            );
            assert_eq!(set.effective_edge_margin, 72);

            let mut file = ThemeFileState::absent_at("");
            file.tokens.edge_margin = Some(8);
            let from_file = resolve_from(
                OverlayTheme {
                    edge_margin: Some(72),
                    ..Default::default()
                },
                file,
                support,
                position,
            );
            assert_eq!(from_file.theme.edge_margin, Some(8));
            assert_eq!(from_file.effective_edge_margin, 8);
        }
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
        assert_eq!(min("edge_margin"), 0.0);
        assert_eq!(max("edge_margin"), f64::from(EDGE_MARGIN_MAX));

        // ...and neither table has a token the other lacks. The ten
        // asserted above are every numeric token there is, on both sides.
        assert_eq!(
            bounds.matches("step:").count(),
            10,
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
    }
}
