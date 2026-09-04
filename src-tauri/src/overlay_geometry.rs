//! The card's footprint, and the native window built around it.
//!
//! Everything the overlay window's size and its blur's corner radius are a
//! function of lives here: the card's shapes, the transparent slack the window
//! keeps around them, and the two theme tokens that change how much room the
//! card needs. It is pure code, with no `AppHandle`, no platform `cfg` and no
//! AppKit, so the arithmetic that decides whether a card fits its window is
//! read and tested on its own, rather than through the window creation, the
//! monitor queries and the show/hide sequencing in [`crate::overlay`], the only
//! caller that turns these numbers into a window.
//!
//! The card constants mirror the `--ov-*` block in `RecordingOverlay.css`. The
//! `overlay_window_constants_match_overlay_css` test parses that file and fails
//! if either side drifts.

use crate::overlay_glass::GlassAppearance;
use crate::overlay_theme::{Material, OverlayTheme, ResolvedOverlayTheme};
use serde::{Deserialize, Serialize};

/// The card's border at size_scale 1, both sides. `.scard` is content-box and
/// draws a `border_width` px stroke on each edge that scales with everything
/// else, so the card's footprint is `(content + card_border(width)) × scale`.
///
/// A function rather than a constant because `border_width` is a token
/// (0-4 px, inherit 1). It is, with `size_scale`, one of the only two tokens
/// that change how much room the card needs. At the inherit width this is
/// 2.0, today's hairline on both edges.
const fn card_border(border_width: u16) -> f64 {
    2.0 * border_width as f64
}

/// Widest compact card content (Minimal / transcribing / processing) at
/// size_scale 1: `--ov-work-w`, the working pill. The pill animates its width
/// from `--ov-rest-w` 172 and expands from its centre, so the window must fit
/// this widest state.
const CARD_COMPACT_CONTENT_W: f64 = 216.0;
/// Compact card content height at size_scale 1: `--ov-base-h` 40, the control
/// row.
const CARD_COMPACT_CONTENT_H: f64 = 40.0;
/// Resting compact card content (Minimal at rest) at size_scale 1:
/// `--ov-rest-w` 172. Only [`OverlayCardShape::CompactRest`] uses this. Under
/// Flat the window still covers [`CARD_COMPACT_CONTENT_W`], the widest card
/// its own [`Card`] can reach.
const CARD_COMPACT_REST_CONTENT_W: f64 = 172.0;
/// Widest Live card content at size_scale 1: `--ov-open-w` 392, the open
/// panel. Live opens from `--ov-pill-w` 184, so this is again the widest
/// state.
const CARD_LIVE_CONTENT_W: f64 = 392.0;
/// Live pill content before it opens or collapses, at size_scale 1:
/// `--ov-pill-w` 184. Only [`OverlayCardShape::LivePill`] uses this. Under
/// Flat the window still covers [`CARD_LIVE_CONTENT_W`], the widest card the
/// panel can reach.
const CARD_LIVE_PILL_CONTENT_W: f64 = 184.0;
/// Tallest Live card content at size_scale 1: the control row `--ov-base-h`
/// 40, the live-text region `--ov-cap-max-h` 64 and its `--ov-cap-pad-y` 12.
const CARD_LIVE_CONTENT_H: f64 = 40.0 + 64.0 + 12.0;

/// How long the card's morph between two shapes takes, in milliseconds:
/// `--ov-morph-ms`, the duration of `.scard`'s width and border-radius
/// transitions. The overlay webview reads the same custom property and sends
/// it with every card-shape report, so the native window-frame animation
/// under Glass would run for exactly as long as the CSS morph does under
/// Flat. That animation is opt-in (`HANDY_GLASS_MORPH=1`), because the window
/// leads WebKit's repaint by a frame or two and the blur shows through.
pub(crate) const CARD_MORPH_MS: u32 = 460;
/// How long the card fades, in milliseconds: `--ov-fade-ms`, the duration of
/// `.ov-fade`'s opacity transition. Under Glass the native blur has to fade
/// over the same span, in and out, or it reads as a separate object.
pub(crate) const CARD_FADE_MS: u32 = 200;
/// The largest morph duration a card-shape report may ask for, in
/// milliseconds, roughly four times [`CARD_MORPH_MS`]. Anything beyond it is
/// a bug or a hostile call rather than a slower animation, and would pin a
/// native window animation on screen long after the card had settled.
pub(crate) const MAX_CARD_MORPH_MS: u32 = 2000;

/// Window slack for the pill, in logical points: 218 + 38 = 256 wide,
/// 42 + 4 = 46 tall, i.e. exactly the window this overlay has always used.
const COMPACT_SLACK: (f64, f64) = (38.0, 4.0);
/// Window slack for the Live panel: 394 + 6 = 400 wide, 118 + 2 = 120 tall,
/// again today's window.
const LIVE_SLACK: (f64, f64) = (6.0, 2.0);

// The four windows this overlay has always used, kept as named scale-1
// fixtures for the tests below and for `overlay`'s own Windows placement
// tests. They are `#[cfg(test)]` because no production path reads a fixed size
// any more. This module computes every window from the card and the resolved
// size scale.
/// The border on both sides at the inherit `border_width`, for the fixtures
/// below. Today's card, hairline included.
#[cfg(test)]
const CARD_BORDER_INHERIT: f64 = card_border(crate::overlay_theme::BORDER_WIDTH_INHERIT);
/// Compact window width at size_scale 1.
#[cfg(test)]
pub(crate) const OVERLAY_WIDTH: f64 =
    CARD_COMPACT_CONTENT_W + CARD_BORDER_INHERIT + COMPACT_SLACK.0;
/// Compact window height at size_scale 1.
#[cfg(test)]
pub(crate) const OVERLAY_HEIGHT: f64 =
    CARD_COMPACT_CONTENT_H + CARD_BORDER_INHERIT + COMPACT_SLACK.1;
/// Live window width at size_scale 1.
#[cfg(test)]
pub(crate) const OVERLAY_STREAM_WIDTH: f64 =
    CARD_LIVE_CONTENT_W + CARD_BORDER_INHERIT + LIVE_SLACK.0;
/// Live window height at size_scale 1.
#[cfg(test)]
pub(crate) const OVERLAY_STREAM_HEIGHT: f64 =
    CARD_LIVE_CONTENT_H + CARD_BORDER_INHERIT + LIVE_SLACK.1;

/// Which card an [`OverlayCardShape`] is a shape of: the pill, or the panel
/// that opens out of one.
///
/// The only distinction the native geometry makes. Every other difference
/// between the UI states happens inside the card, at a size the window already
/// covers. The pill never opens, so under Flat its window covers the widest
/// pill; the panel does, so its window covers the open panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Card {
    /// The pill: Minimal in every state, and the compact working card either
    /// overlay style shows while it finalizes.
    Pill,
    /// The Live panel, and the pill it opens from and collapses back to.
    Panel,
}

impl Card {
    /// The widest footprint this card can reach at size_scale 1, borders
    /// included. What the window covers under Flat, where the CSS morph
    /// happens inside a window sized for the widest card. Under Glass the
    /// window instead equals the exact current [`OverlayCardShape`], never
    /// a maximum.
    fn widest_footprint(self, border_width: u16) -> (f64, f64) {
        let border = card_border(border_width);
        match self {
            Self::Pill => (
                CARD_COMPACT_CONTENT_W + border,
                CARD_COMPACT_CONTENT_H + border,
            ),
            Self::Panel => (CARD_LIVE_CONTENT_W + border, CARD_LIVE_CONTENT_H + border),
        }
    }

    /// The transparent slack between the card's footprint and the edge of
    /// the native overlay window, in logical points.
    ///
    /// It exists so a card mid-morph is never clipped by the overlay page's
    /// `overflow: hidden`. Zero under Glass, where the window rectangle is the
    /// card. The native glass view fills the whole window, so any slack would
    /// paint blur outside it. This needs no `#[cfg(target_os = "macos")]`. The
    /// effective Material is never Glass off macOS (only a `support()` with
    /// `available: true` can produce it, and that only exists on macOS), so it
    /// reduces to today's per-card slack everywhere but a Mac with Glass
    /// actually available.
    fn slack(self, material: Material) -> (f64, f64) {
        if material == Material::Glass {
            return (0.0, 0.0);
        }
        match self {
            Self::Pill => COMPACT_SLACK,
            Self::Panel => LIVE_SLACK,
        }
    }
}

/// Which of the five card shapes the overlay is currently drawing.
///
/// Under Flat this is bookkeeping only. The window covers the widest card the
/// overlay style can reach, and the CSS morph happens inside it. Under Glass
/// it is the unit the native window is sized from, for two reasons: the window
/// slack is zero, so the window rectangle is the card, and the Live panel's
/// open/collapsed morph is a pure webview decision (driven by streamed text
/// and phase) that Rust cannot see any other way.
///
/// One shape per distinct `.scard` class combination in
/// `RecordingOverlay.tsx`; the footprints mirror the `--ov-*` block in
/// `RecordingOverlay.css` and are pinned to it by
/// `overlay_window_constants_match_overlay_css`. Must agree with
/// `cardShape()` in `src/overlay/cardShape.ts`; pinned by
/// `initial_card_shape_matches_card_shape_ts`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OverlayCardShape {
    /// The resting Minimal pill, `.scard.compact`.
    CompactRest,
    /// The Minimal working pill, `.scard.compact.cworking`, at the same
    /// footprint as Live's own collapsed working pill.
    CompactWorking,
    /// The Live pill before it opens or collapses, `.scard`.
    LivePill,
    /// The Live panel collapsed to its working pill, `.scard.working`.
    LiveWorking,
    /// The Live panel expanded, `.scard.open`.
    LiveOpen,
}

impl OverlayCardShape {
    /// Declaration order, mirroring the `#[repr(u8)]` discriminants. This is
    /// the table [`Self::from_u8`] indexes into.
    const ALL: [OverlayCardShape; 5] = [
        Self::CompactRest,
        Self::CompactWorking,
        Self::LivePill,
        Self::LiveWorking,
        Self::LiveOpen,
    ];

    /// The shape the card takes on the first frame of `state`. Must agree with
    /// `cardShape()` in `src/overlay/cardShape.ts`; pinned by
    /// `initial_card_shape_matches_card_shape_ts`.
    ///
    /// Verified against the frontend, state by state:
    /// `"recording"` renders `.scard.compact` with `working = false` ->
    /// [`Self::CompactRest`]; `"transcribing"`/`"processing"` render
    /// `.scard.compact.cworking` -> [`Self::CompactWorking`];
    /// `"streaming"` resets its stream state before showing, so it always
    /// starts at `open = false, collapsed = false` -> [`Self::LivePill`].
    pub(crate) fn initial_for(state: &str) -> Self {
        match state {
            "transcribing" | "processing" => Self::CompactWorking,
            "streaming" => Self::LivePill,
            _ => Self::CompactRest, // "recording"
        }
    }

    /// Which card this shape belongs to. The slack and the Flat-material
    /// footprint (the widest card it can reach) both key on it.
    fn card(self) -> Card {
        match self {
            Self::CompactRest | Self::CompactWorking => Card::Pill,
            Self::LivePill | Self::LiveWorking | Self::LiveOpen => Card::Panel,
        }
    }

    /// This shape's own exact footprint at size_scale 1, border included. The
    /// window must equal it under Glass, where the window is the card. Every
    /// number comes from the `--ov-*` block in RecordingOverlay.css.
    fn card_footprint(self, border_width: u16) -> (f64, f64) {
        let border = card_border(border_width);
        let (content_width, content_height) = match self {
            Self::CompactRest => (CARD_COMPACT_REST_CONTENT_W, CARD_COMPACT_CONTENT_H),
            Self::CompactWorking => (CARD_COMPACT_CONTENT_W, CARD_COMPACT_CONTENT_H),
            Self::LivePill => (CARD_LIVE_PILL_CONTENT_W, CARD_COMPACT_CONTENT_H),
            Self::LiveWorking => (CARD_COMPACT_CONTENT_W, CARD_COMPACT_CONTENT_H),
            Self::LiveOpen => (CARD_LIVE_CONTENT_W, CARD_LIVE_CONTENT_H),
        };
        (content_width + border, content_height + border)
    }

    /// The corner-radius factor CSS applies for this shape's `.scard...`
    /// class: the pill and the two compact states are the full radius token,
    /// the Live working pill is 3/4, the open panel is 2/3. Both CSS and the
    /// native `CALayer` radius clamp visually to half the shorter side, so
    /// they agree by construction without either side rounding first. This
    /// value is deliberately left unrounded.
    fn radius_factor(self) -> f64 {
        match self {
            Self::CompactRest | Self::CompactWorking | Self::LivePill => 1.0,
            Self::LiveWorking => 0.75,
            Self::LiveOpen => 2.0 / 3.0,
        }
    }

    /// Recover a shape from the byte the overlay's card-shape atomic stores.
    /// Any value outside the five real discriminants (never produced by this
    /// module) falls back to [`Self::CompactRest`] rather than panicking.
    pub(crate) fn from_u8(value: u8) -> Self {
        Self::ALL
            .get(value as usize)
            .copied()
            .unwrap_or(Self::CompactRest)
    }

    /// How long the glass view fades out for when the overlay hides, matched
    /// to the way this card actually leaves the screen. The pill fades with
    /// its container over `--ov-fade-ms`. The Live card is unmounted outright,
    /// so its blur has to go at once rather than linger over an empty window.
    pub(crate) fn glass_fade_out_ms(self) -> u32 {
        match self.card() {
            Card::Pill => CARD_FADE_MS,
            Card::Panel => 0,
        }
    }
}

/// The three theme tokens the card's rectangle is a function of, clamped once.
///
/// They always travel together. The size scale zooms every length, the border
/// width adds two strokes to every footprint, and the radius rounds the corners
/// the blur has to match. So `from_theme` builds all three once from the
/// resolved theme and callers ask the result for a window size or a corner
/// radius, rather than passing them one at a time to free functions that each
/// have to remember which of them were already clamped.
///
/// The constructor clamps, so no caller has to. The geometry must never trust
/// a number that reached it unclamped, and it uses the same bounds as
/// [`OverlayTheme::size_scale`] and [`OverlayTheme::border_width`], so the
/// window and the card can never disagree about how far a token was allowed to
/// go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CardMetrics {
    scale: f64,
    border_width: u16,
    radius_px: f64,
}

impl CardMetrics {
    /// The resolved `radius` token in px at `size_scale` 1 when the theme sets
    /// none. It is the CSS token's own default (`--ov-radius: 24px`,
    /// `RecordingOverlay.css`).
    const DEFAULT_RADIUS_PX: f64 = 24.0;

    /// The metrics a resolved theme asks for.
    ///
    /// `size_scale` and `border_width` come through the theme's own accessors,
    /// which already clamp. `OverlayTheme` carries no accessor for `radius`,
    /// so this reads the public field and clamps it here.
    pub(crate) fn from_theme(theme: &OverlayTheme) -> Self {
        Self {
            scale: theme.size_scale(),
            border_width: theme.border_width(),
            radius_px: theme
                .radius
                .map(f64::from)
                .unwrap_or(Self::DEFAULT_RADIUS_PX)
                .min(f64::from(crate::overlay_theme::RADIUS_MAX)),
        }
    }

    /// Overlay window size (logical points) for a card shape under a Material.
    ///
    /// Under Glass the window equals the shape's own exact footprint, because
    /// there the window is the card. Under Flat the window covers the widest
    /// card that shape's [`Card`] can reach, because the CSS width/height
    /// morph happens inside it.
    ///
    /// This rounds the scaled card up before adding the slack, so the window
    /// is never a fraction of a point short of the card it hosts and every
    /// result is a whole number of points.
    pub(crate) fn window_size(&self, shape: OverlayCardShape, material: Material) -> (f64, f64) {
        let (card_width, card_height) = match material {
            Material::Glass => shape.card_footprint(self.border_width),
            Material::Flat => shape.card().widest_footprint(self.border_width),
        };
        let (slack_width, slack_height) = shape.card().slack(material);

        (
            (card_width * self.scale).ceil() + slack_width,
            (card_height * self.scale).ceil() + slack_height,
        )
    }

    /// A shape's corner radius in px, mirroring the CSS
    /// `calc(var(--ov-radius) * var(--ov-scale) * factor)`. Unrounded, like
    /// the CSS, so CALayer and CSS agree by construction (both clamp visually
    /// to half the shorter side).
    fn corner_radius(&self, shape: OverlayCardShape) -> f64 {
        self.radius_px * self.scale * shape.radius_factor()
    }
}

/// Everything a theme configures the native overlay window from.
///
/// Its size, the blur's macOS material and the blur's corner radius are
/// functions of these values and of nothing else, so two theme deliveries that
/// agree on all of them would ask the window for exactly what it already is.
/// That is what [`native_update_needed`] tests, and what lets a colour edit
/// skip the main-thread hop, the AppKit material write and the resize
/// entirely.
///
/// Its position is not a function of this state alone. It also follows the
/// `overlay_position` setting and the monitor the card is placed on. Neither
/// is anything a theme can move, and neither reaches the window through here.
/// The position and style commands go through the unconditional
/// `update_overlay_position`, and every show repositions anyway. So skipping
/// a theme delivery can leave the window where the last show or position
/// update put it, which is exactly where it belongs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OverlayWindowState {
    shape: OverlayCardShape,
    material: Material,
    glass: GlassAppearance,
    metrics: CardMetrics,
}

impl OverlayWindowState {
    /// The state `resolved` asks for, at the card shape on screen right now.
    pub(crate) fn new(shape: OverlayCardShape, resolved: &ResolvedOverlayTheme) -> Self {
        Self {
            shape,
            material: resolved.effective_material,
            glass: GlassAppearance::from_theme(&resolved.theme),
            metrics: CardMetrics::from_theme(&resolved.theme),
        }
    }

    /// The window this state wants, in logical points.
    pub(crate) fn window_size(&self) -> (f64, f64) {
        self.metrics.window_size(self.shape, self.material)
    }

    /// The corner radius the native blur has to take to match the card.
    pub(crate) fn corner_radius(&self) -> f64 {
        self.metrics.corner_radius(self.shape)
    }
}

/// Whether the native window has to be touched at all.
///
/// Pure, so "only a change to something the window is actually built from"
/// is a test rather than a reading of the call site. An unknown previous
/// state always needs the update, because the window may have just been
/// created.
pub(crate) fn native_update_needed(
    previous: Option<&OverlayWindowState>,
    next: &OverlayWindowState,
) -> bool {
    previous != Some(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend_source::{css_ms, css_px, css_rule, tsx_const, OVERLAY_CSS, OVERLAY_TSX};
    use crate::overlay_glass::GlassAppearance;
    use crate::overlay_theme::{
        GlassMaterial, GlassStyle, HexColor, BORDER_WIDTH_INHERIT, BORDER_WIDTH_MAX, PADDING_MAX,
        WAVEFORM_GAP_MAX, WAVEFORM_WIDTH_MAX, WAVEFORM_WIDTH_MIN,
    };

    /// The metrics of today's card: nothing set, everything inherited.
    fn inherit_metrics() -> CardMetrics {
        CardMetrics::from_theme(&OverlayTheme::default())
    }

    /// The window `shape` gets at `scale` under `material`, with the inherit
    /// border width, the shorthand the dimension tests below are written in.
    fn window(shape: OverlayCardShape, scale: f64, material: Material) -> (f64, f64) {
        window_at(shape, scale, material, BORDER_WIDTH_INHERIT)
    }

    /// The same, for the tests that vary the border width too. Built through
    /// the constructor's own clamp, so an out-of-range scale or width is
    /// treated here exactly as one arriving from a theme would be.
    fn window_at(
        shape: OverlayCardShape,
        scale: f64,
        material: Material,
        border_width: u16,
    ) -> (f64, f64) {
        metrics_for(scale, border_width).window_size(shape, material)
    }

    fn metrics_for(scale: f64, border_width: u16) -> CardMetrics {
        CardMetrics::from_theme(&OverlayTheme {
            size_scale: Some(scale),
            border_width: Some(border_width),
            ..OverlayTheme::default()
        })
    }

    fn window_state() -> OverlayWindowState {
        OverlayWindowState {
            shape: OverlayCardShape::LivePill,
            material: Material::Flat,
            glass: GlassAppearance {
                glass_material: GlassMaterial::HudWindow,
                glass_style: GlassStyle::Regular,
                surface: None,
                glass_tint: None,
            },
            metrics: inherit_metrics(),
        }
    }

    /// The rule that makes live editing cheap: an accent, a text colour or a
    /// padding cannot move the native window, so a delivery carrying only
    /// those must not hop to the main thread at all.
    #[test]
    fn a_theme_that_cannot_move_the_window_needs_no_native_update() {
        let state = window_state();
        assert!(!native_update_needed(Some(&state), &state.clone()));
    }

    /// ...and everything the window really is built from still gets through,
    /// one field at a time.
    ///
    /// The test destructures the state exhaustively, with no `..`, and every
    /// binding it produces then derives the variant that changes that one
    /// field, under a local `deny(unused_variables)`. So a field added to
    /// `OverlayWindowState`, to `CardMetrics` or to `GlassAppearance` stops
    /// this test compiling until it is named here, and naming it without using
    /// it is an error too.
    ///
    /// What that cannot catch: a value the native window is built from that
    /// never became a field of this state at all. Nothing but reading
    /// `update_overlay_position_on_main` and `overlay_glass` will tell you
    /// that. The state is a claim about those two, and this test only keeps
    /// the claim internally honest.
    #[test]
    #[deny(unused_variables)]
    fn every_token_the_window_is_built_from_forces_a_native_update() {
        let base = window_state();
        let OverlayWindowState {
            shape,
            material,
            glass:
                GlassAppearance {
                    glass_material,
                    glass_style,
                    surface,
                    glass_tint,
                },
            metrics:
                CardMetrics {
                    scale,
                    border_width,
                    radius_px,
                },
        } = base.clone();

        // Each variant differs from `base` in exactly the field it names, and
        // is derived from that field's own value so it cannot accidentally
        // equal it.
        let glass_variant = |glass: GlassAppearance| OverlayWindowState {
            glass,
            ..base.clone()
        };
        let metrics_variant = |metrics: CardMetrics| OverlayWindowState {
            metrics,
            ..base.clone()
        };
        let variants = [
            OverlayWindowState {
                shape: match shape {
                    OverlayCardShape::LiveOpen => OverlayCardShape::LivePill,
                    _ => OverlayCardShape::LiveOpen,
                },
                ..base.clone()
            },
            OverlayWindowState {
                material: match material {
                    Material::Flat => Material::Glass,
                    Material::Glass => Material::Flat,
                },
                ..base.clone()
            },
            metrics_variant(CardMetrics {
                scale: scale + 0.25,
                ..base.metrics
            }),
            metrics_variant(CardMetrics {
                border_width: border_width + 1,
                ..base.metrics
            }),
            metrics_variant(CardMetrics {
                radius_px: radius_px + 12.0,
                ..base.metrics
            }),
            // The four Glass-only tokens. Only the liquid engine reads the
            // style and the tint, but all of them are live property writes on
            // the installed view, so a change to any has to reach it.
            glass_variant(GlassAppearance {
                glass_material: match glass_material {
                    GlassMaterial::Popover => GlassMaterial::HudWindow,
                    _ => GlassMaterial::Popover,
                },
                ..base.glass.clone()
            }),
            glass_variant(GlassAppearance {
                glass_style: match glass_style {
                    GlassStyle::Clear => GlassStyle::Regular,
                    _ => GlassStyle::Clear,
                },
                ..base.glass.clone()
            }),
            glass_variant(GlassAppearance {
                surface: match surface {
                    Some(_) => None,
                    None => HexColor::parse("#101020"),
                },
                ..base.glass.clone()
            }),
            glass_variant(GlassAppearance {
                glass_tint: match glass_tint {
                    Some(tint) => Some(tint / 2.0),
                    None => Some(0.2),
                },
                ..base.glass.clone()
            }),
        ];
        for variant in variants {
            assert!(
                native_update_needed(Some(&base), &variant),
                "{variant:?} must reach the native window"
            );
        }
    }

    /// A window nothing has configured yet, freshly created or re-created,
    /// always needs the update, whatever the theme says.
    #[test]
    fn an_unconfigured_window_always_needs_the_native_update() {
        assert!(native_update_needed(None, &window_state()));
    }

    /// A window state answers both native questions itself, from the shape it
    /// carries. That is the whole reason it holds [`CardMetrics`] rather than
    /// three loose numbers.
    #[test]
    fn a_window_state_sizes_and_rounds_its_own_card() {
        let state = window_state();
        assert_eq!(
            state.window_size(),
            window(OverlayCardShape::LivePill, 1.0, Material::Flat)
        );
        assert_eq!(
            state.corner_radius(),
            inherit_metrics().corner_radius(OverlayCardShape::LivePill)
        );
    }

    /// The "defaults reproduce today's overlay exactly" pin: with no size token
    /// set, every state gets the window this overlay has always used. The
    /// literals are the sizes overlay.rs hardcoded before the token existed.
    #[test]
    fn overlay_dimensions_at_scale_one_match_todays_windows() {
        for (state, expected_shape) in [
            ("recording", OverlayCardShape::CompactRest),
            ("transcribing", OverlayCardShape::CompactWorking),
            ("processing", OverlayCardShape::CompactWorking),
        ] {
            let shape = OverlayCardShape::initial_for(state);
            assert_eq!(shape, expected_shape, "{state}");
            assert_eq!(shape.card(), Card::Pill, "{state}");
            assert_eq!(window(shape, 1.0, Material::Flat), (256.0, 46.0), "{state}");
        }
        assert_eq!(
            OverlayCardShape::initial_for("streaming"),
            OverlayCardShape::LivePill
        );
        assert_eq!(OverlayCardShape::LivePill.card(), Card::Panel);
        assert_eq!(
            window(OverlayCardShape::LivePill, 1.0, Material::Flat),
            (400.0, 120.0)
        );

        // The same four numbers as named fixtures, so the Windows bounds tests
        // in `overlay` keep exercising today's sizes.
        assert_eq!((OVERLAY_WIDTH, OVERLAY_HEIGHT), (256.0, 46.0));
        assert_eq!(
            (OVERLAY_STREAM_WIDTH, OVERLAY_STREAM_HEIGHT),
            (400.0, 120.0)
        );
    }

    /// The card scales, the slack does not. Under Flat every shape of a card
    /// produces the same window, so which exact shape is passed is immaterial.
    /// The shapes below are picked for variety.
    #[test]
    fn overlay_dimensions_scale_with_the_token() {
        assert_eq!(
            window(OverlayCardShape::CompactRest, 1.5, Material::Flat),
            (365.0, 67.0)
        );
        assert_eq!(
            window(OverlayCardShape::LivePill, 1.5, Material::Flat),
            (597.0, 179.0)
        );
        assert_eq!(
            window(OverlayCardShape::CompactRest, 0.8, Material::Flat),
            (213.0, 38.0)
        );
        assert_eq!(
            window(OverlayCardShape::LivePill, 0.8, Material::Flat),
            (322.0, 97.0)
        );
    }

    /// A scale that reached the geometry unclamped is treated as the nearest
    /// bound, and a number that is no scale at all falls back to 1.
    #[test]
    fn overlay_dimensions_clamp_out_of_range_and_non_finite() {
        assert_eq!(
            window(OverlayCardShape::LiveOpen, 3.0, Material::Flat),
            (597.0, 179.0)
        );
        assert_eq!(
            window(OverlayCardShape::CompactWorking, 0.1, Material::Flat),
            (213.0, 38.0)
        );
        for broken in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                window(OverlayCardShape::CompactRest, broken, Material::Flat),
                (256.0, 46.0)
            );
        }
    }

    /// The invariant Glass must not break when it sets the slack to zero: a
    /// Flat window covers the card at every scale. The card footprints are
    /// the token contract's own numbers, not a repeat of the arithmetic under
    /// test.
    #[test]
    fn overlay_window_always_covers_the_card() {
        for (shape, scale, card_width, card_height) in [
            (OverlayCardShape::CompactWorking, 0.80, 175.0, 34.0),
            (OverlayCardShape::LiveOpen, 0.80, 316.0, 95.0),
            (OverlayCardShape::CompactWorking, 1.00, 218.0, 42.0),
            (OverlayCardShape::LiveOpen, 1.00, 394.0, 118.0),
            (OverlayCardShape::CompactWorking, 1.50, 327.0, 63.0),
            (OverlayCardShape::LiveOpen, 1.50, 591.0, 177.0),
        ] {
            let (width, height) = window(shape, scale, Material::Flat);
            assert!(
                width >= card_width,
                "{shape:?} at {scale}: window {width} is narrower than the card's {card_width}"
            );
            assert!(
                height >= card_height,
                "{shape:?} at {scale}: window {height} is shorter than the card's {card_height}"
            );
        }
    }

    /// Under Glass the window equals the card exactly, with zero slack, at
    /// every shape and every 0.05 step of scale from 0.80 to 1.50. The
    /// footprints are spelled out rather than read from `card_footprint()`,
    /// so a constant that drifted cannot agree with itself, and no
    /// expectation here ever adds a slack term.
    #[test]
    fn glass_window_equals_card_at_every_scale() {
        // Each card's footprint at size_scale 1, as literals: content plus
        // the 1px hairline on each side.
        const CARDS: [(OverlayCardShape, f64, f64); 5] = [
            (OverlayCardShape::CompactRest, 174.0, 42.0),
            (OverlayCardShape::CompactWorking, 218.0, 42.0),
            (OverlayCardShape::LivePill, 186.0, 42.0),
            (OverlayCardShape::LiveWorking, 218.0, 42.0),
            (OverlayCardShape::LiveOpen, 394.0, 118.0),
        ];
        // The same five cards at the token's maximum, also as literals: the
        // one scale where every width lands on a whole point without a
        // rounding step to hide a mistake.
        const CARDS_AT_1_5: [(OverlayCardShape, f64, f64); 5] = [
            (OverlayCardShape::CompactRest, 261.0, 63.0),
            (OverlayCardShape::CompactWorking, 327.0, 63.0),
            (OverlayCardShape::LivePill, 279.0, 63.0),
            (OverlayCardShape::LiveWorking, 327.0, 63.0),
            (OverlayCardShape::LiveOpen, 591.0, 177.0),
        ];

        for (shape, width, height) in CARDS {
            assert_eq!(
                window(shape, 1.0, Material::Glass),
                (width, height),
                "{shape:?} at 1.0"
            );
        }
        for (shape, width, height) in CARDS_AT_1_5 {
            assert_eq!(
                window(shape, 1.5, Material::Glass),
                (width, height),
                "{shape:?} at 1.5"
            );
        }

        // Every 0.05 step in between, built from integers so the last step is
        // exactly the 1.50 the geometry clamps to.
        for step in 0..=14 {
            let scale = f64::from(80 + step * 5) / 100.0;
            for (shape, width, height) in CARDS {
                assert_eq!(
                    window(shape, scale, Material::Glass),
                    ((width * scale).ceil(), (height * scale).ceil()),
                    "{shape:?} at {scale}"
                );
            }
        }
    }

    /// `border_width` is the second token that moves the native window, so the
    /// window has to grow with it. The card is content-box, so each extra px
    /// of stroke costs two px of footprint before the scale multiplies. The
    /// expectations are the token table's arithmetic written out, never
    /// `card_footprint()` called again.
    #[test]
    fn overlay_dimensions_follow_the_border_width() {
        // Under Glass the window is the card, so the footprint is visible
        // directly. LiveOpen's content is 392 x 116.
        for (width, expected) in [
            (0, (392.0, 116.0)),
            (1, (394.0, 118.0)),
            (2, (396.0, 120.0)),
            (4, (400.0, 124.0)),
        ] {
            assert_eq!(
                window_at(OverlayCardShape::LiveOpen, 1.0, Material::Glass, width),
                expected,
                "border_width {width}"
            );
        }

        // Flat keeps its slack, so the same growth lands on today's window:
        // the pill is 216 x 40 of content plus 38 x 4 of slack.
        for (width, expected) in [(0, (254.0, 44.0)), (1, (256.0, 46.0)), (4, (262.0, 52.0))] {
            assert_eq!(
                window_at(OverlayCardShape::CompactRest, 1.0, Material::Flat, width),
                expected,
                "border_width {width}"
            );
        }

        // The stroke scales with the card, so at 1.5x a 4px border is 12
        // points of footprint: 392 + 8 = 400 content-plus-border, x 1.5.
        assert_eq!(
            window_at(OverlayCardShape::LiveOpen, 1.5, Material::Glass, 4),
            (600.0, 186.0)
        );

        // Whatever the width, Glass's window equals its card and Flat's
        // window covers it, the invariant zero slack rests on.
        for width in 0..=BORDER_WIDTH_MAX {
            for step in 0..=14 {
                let scale = f64::from(80 + step * 5) / 100.0;
                for shape in OverlayCardShape::ALL {
                    let (glass_w, glass_h) = window_at(shape, scale, Material::Glass, width);
                    let (card_w, card_h) = shape.card_footprint(width);
                    assert_eq!(
                        (glass_w, glass_h),
                        ((card_w * scale).ceil(), (card_h * scale).ceil()),
                        "{shape:?} at {scale}, border {width}"
                    );

                    let (flat_w, flat_h) = window_at(shape, scale, Material::Flat, width);
                    assert!(
                        flat_w >= glass_w && flat_h >= glass_h,
                        "{shape:?} at {scale}, border {width}: Flat window \
                         {flat_w}x{flat_h} does not cover the card {glass_w}x{glass_h}"
                    );
                }
            }
        }

        // A width that reached the geometry unclamped is treated as the
        // bound, exactly as an out-of-range scale is.
        assert_eq!(
            window_at(OverlayCardShape::LiveOpen, 1.0, Material::Glass, 99),
            window_at(
                OverlayCardShape::LiveOpen,
                1.0,
                Material::Glass,
                BORDER_WIDTH_MAX
            )
        );
    }

    /// The corner radius follows the CSS `calc()` exactly, including the
    /// per-shape factor, and a radius that arrived out of range is clamped
    /// like every other token.
    #[test]
    fn the_corner_radius_mirrors_the_css_calc() {
        let metrics = inherit_metrics();
        assert_eq!(metrics.corner_radius(OverlayCardShape::CompactRest), 24.0);
        assert_eq!(metrics.corner_radius(OverlayCardShape::LiveWorking), 18.0);
        assert_eq!(metrics.corner_radius(OverlayCardShape::LiveOpen), 16.0);

        let scaled = CardMetrics::from_theme(&OverlayTheme {
            size_scale: Some(1.5),
            radius: Some(10),
            ..OverlayTheme::default()
        });
        assert_eq!(scaled.corner_radius(OverlayCardShape::LivePill), 15.0);

        let over = CardMetrics::from_theme(&OverlayTheme {
            radius: Some(u16::MAX),
            ..OverlayTheme::default()
        });
        assert_eq!(
            over.corner_radius(OverlayCardShape::CompactRest),
            f64::from(crate::overlay_theme::RADIUS_MAX)
        );
    }

    /// Why `waveform_width` stops at 6 and `padding` at 20: at every token's
    /// maximum the control row's content still fits the working pill, so no
    /// combination of the spacing tokens can force the native window to grow.
    /// Only `size_scale` and `border_width` do that.
    #[test]
    fn the_waveform_never_outgrows_the_working_pill() {
        // The three inputs the frontend owns, read from the frontend: the bar
        // count from the component that renders them, and `.swave`'s right
        // padding and `.sbase`'s two side columns (which hold the recording
        // dot and the cancel button) from the stylesheet that draws them.
        let bars = tsx_const(OVERLAY_TSX, "const WAVE_BARS = ");
        let wave_padding_right = css_px(css_rule(OVERLAY_CSS, ".swave {"), "padding-right");
        let side_column = css_px(css_rule(OVERLAY_CSS, ".sbase {"), "grid-template-columns");

        let widest_row = bars * f64::from(WAVEFORM_WIDTH_MAX)
            + (bars - 1.0) * f64::from(WAVEFORM_GAP_MAX)
            + wave_padding_right
            + 2.0 * f64::from(PADDING_MAX)
            + 2.0 * side_column;

        assert!(
            widest_row <= CARD_COMPACT_CONTENT_W,
            "the row at every maximum is {widest_row}, wider than the {CARD_COMPACT_CONTENT_W} working pill"
        );
        // And the narrowest bar still draws: 2px at the smallest scale is
        // 1.6px, which WebKit still paints.
        assert!(f64::from(WAVEFORM_WIDTH_MIN) * crate::overlay_theme::SIZE_SCALE_MIN >= 1.0);
    }

    /// The card shape survives the round trip through the byte the atomic
    /// stores, for every variant, and a byte nothing in this module can
    /// produce falls back instead of panicking.
    #[test]
    fn card_shape_round_trips_through_its_byte() {
        for (index, shape) in OverlayCardShape::ALL.into_iter().enumerate() {
            assert_eq!(OverlayCardShape::from_u8(shape as u8), shape, "{shape:?}");
            // ALL is the table from_u8 indexes into, so it has to stay in
            // discriminant order.
            assert_eq!(shape as usize, index, "{shape:?}");
        }
        for unknown in [OverlayCardShape::ALL.len() as u8, u8::MAX] {
            assert_eq!(
                OverlayCardShape::from_u8(unknown),
                OverlayCardShape::CompactRest,
                "{unknown}"
            );
        }
    }

    /// The blur leaves the screen the way the card does: with the pill's
    /// fade, and at once for the Live card, which is unmounted.
    #[test]
    fn glass_fades_out_with_the_card_it_sits_under() {
        assert_eq!(
            OverlayCardShape::CompactRest.glass_fade_out_ms(),
            CARD_FADE_MS
        );
        assert_eq!(
            OverlayCardShape::CompactWorking.glass_fade_out_ms(),
            CARD_FADE_MS
        );
        assert_eq!(OverlayCardShape::LivePill.glass_fade_out_ms(), 0);
        assert_eq!(OverlayCardShape::LiveWorking.glass_fade_out_ms(), 0);
        assert_eq!(OverlayCardShape::LiveOpen.glass_fade_out_ms(), 0);
    }

    /// Must agree with `cardShape()` in `src/overlay/cardShape.ts` for every
    /// state string the show path can carry.
    #[test]
    fn initial_card_shape_matches_card_shape_ts() {
        assert_eq!(
            OverlayCardShape::initial_for("recording"),
            OverlayCardShape::CompactRest
        );
        assert_eq!(
            OverlayCardShape::initial_for("transcribing"),
            OverlayCardShape::CompactWorking
        );
        assert_eq!(
            OverlayCardShape::initial_for("processing"),
            OverlayCardShape::CompactWorking
        );
        assert_eq!(
            OverlayCardShape::initial_for("streaming"),
            OverlayCardShape::LivePill
        );
    }

    /// The card constants above and the `--ov-*` block in RecordingOverlay.css
    /// are two copies of the same geometry. This test is what keeps them one
    /// number. It reads the CSS the overlay actually ships with and fails
    /// naming the variable that drifted, instead of shipping a clipped card.
    #[test]
    fn overlay_window_constants_match_overlay_css() {
        // The card's content lengths, which the footprints above are built
        // from before the border is added.
        assert_eq!(css_px(OVERLAY_CSS, "--ov-work-w"), CARD_COMPACT_CONTENT_W);
        assert_eq!(css_px(OVERLAY_CSS, "--ov-base-h"), CARD_COMPACT_CONTENT_H);
        assert_eq!(css_px(OVERLAY_CSS, "--ov-open-w"), CARD_LIVE_CONTENT_W);
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-base-h")
                + css_px(OVERLAY_CSS, "--ov-cap-max-h")
                + css_px(OVERLAY_CSS, "--ov-cap-pad-y"),
            CARD_LIVE_CONTENT_H
        );
        // The two shapes only Glass sizes to exactly: the resting compact
        // pill and the Live pill before it opens or collapses.
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-rest-w"),
            CARD_COMPACT_REST_CONTENT_W
        );
        assert_eq!(css_px(OVERLAY_CSS, "--ov-pill-w"), CARD_LIVE_PILL_CONTENT_W);

        // The stroke `.scard` draws on each edge. The CSS declares the
        // inherit width, and this side doubles it. The card is content-box,
        // so the footprint carries one on each edge.
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-border-w"),
            f64::from(BORDER_WIDTH_INHERIT)
        );
        assert_eq!(card_border(BORDER_WIDTH_INHERIT), CARD_BORDER_INHERIT);
        // The waveform bar's own width. No footprint uses it, but it follows
        // the same "the CSS declares the inherit value" rule, so the tab's
        // slider and the stylesheet cannot drift.
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-wave-w"),
            f64::from(crate::overlay_theme::WAVEFORM_WIDTH_INHERIT)
        );
        // The radius an unset token falls back to, and the number
        // `corner_radius` scales into the blur's own radius.
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-radius"),
            CardMetrics::DEFAULT_RADIUS_PX
        );

        // The card's own timings. The CSS transitions read these two
        // properties, the overlay webview reads --ov-morph-ms at runtime to
        // tell the backend how long to animate the window frame, and the
        // native reveal and fade-out read the two constants pinned just
        // below, so all three have to be one number.
        assert_eq!(
            css_ms(OVERLAY_CSS, "--ov-morph-ms"),
            f64::from(CARD_MORPH_MS)
        );
        assert_eq!(css_ms(OVERLAY_CSS, "--ov-fade-ms"), f64::from(CARD_FADE_MS));

        // Both morphs are a grow, so the widest card per shape family is the
        // one the window is sized from above.
        assert!(css_px(OVERLAY_CSS, "--ov-rest-w") <= css_px(OVERLAY_CSS, "--ov-work-w"));
        assert!(css_px(OVERLAY_CSS, "--ov-pill-w") <= css_px(OVERLAY_CSS, "--ov-open-w"));
    }
}
