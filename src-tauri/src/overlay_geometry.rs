//! The card's footprint, and the native window built around it.
//!
//! Everything the overlay window's size and its blur's corner radius are a
//! function of lives here: the card's shapes, the transparent slack around
//! them, the room a Flat card's drop shadow needs, and the tokens that size it.
//! It is pure code, with no `AppHandle`, no platform `cfg` and no AppKit, so
//! the arithmetic deciding whether a card fits its window is tested on its
//! own, not through the window creation, monitor queries and show/hide
//! sequencing in [`crate::overlay`], its only caller.
//!
//! The card constants mirror the `--ov-*` block in `RecordingOverlay.css`, and
//! `overlay_window_constants_match_overlay_css` fails if either side drifts.

use crate::overlay_glass::GlassAppearance;
use crate::overlay_theme::{Material, OverlayTheme, ResolvedOverlayTheme};
use serde::{Deserialize, Serialize};

/// The card's border at size_scale 1, both sides. `.scard` is content-box and
/// strokes `border_width` px per edge, so the footprint is
/// `(content + card_border(width)) × scale`.
///
/// A function, not a constant, because `border_width` is a token (0-4 px,
/// inherit 1). With `size_scale`, `padding` and `element_gap` it is one of
/// the four tokens sizing the card on either Material. At the inherit width
/// it is 2.0, today's hairline on both edges.
const fn card_border(border_width: u16) -> f64 {
    2.0 * border_width as f64
}

/// Widest compact card content (Minimal / transcribing / processing) at
/// size_scale 1: `--ov-work-w`, the working pill. It animates from
/// `--ov-rest-w` 172 outward from its centre, so the window must fit it.
const CARD_COMPACT_CONTENT_W: f64 = 216.0;
/// The control row at size_scale 1 before its padding: `--ov-row-core-h` 20.
/// [`CardMetrics::row_height`] adds one `padding` above and below, so at the
/// inherit padding of 10 it is the 40 px row this overlay has always drawn.
const CARD_ROW_CORE_H: f64 = 20.0;
/// Resting compact card content (Minimal at rest) at size_scale 1:
/// `--ov-rest-w` 172, used only by [`OverlayCardShape::CompactRest`]. Under
/// Flat the window covers [`CARD_COMPACT_CONTENT_W`], its [`Card`]'s widest.
const CARD_COMPACT_REST_CONTENT_W: f64 = 172.0;
/// Widest Live card content at size_scale 1: `--ov-open-w` 392, the open
/// panel. Live opens from `--ov-pill-w` 184, so again the widest state.
const CARD_LIVE_CONTENT_W: f64 = 392.0;
/// Live pill content before it opens or collapses, at size_scale 1:
/// `--ov-pill-w` 184, used only by [`OverlayCardShape::LivePill`]. Under Flat
/// the window still covers [`CARD_LIVE_CONTENT_W`], the panel's widest.
const CARD_LIVE_PILL_CONTENT_W: f64 = 184.0;
/// The live-text region's height at size_scale 1: `--ov-cap-max-h` 64. The
/// Live card is the control row, this, and the inset above it.
const CARD_CAP_MAX_H: f64 = 64.0;
/// The inset above the live text, as a multiple of `padding`:
/// `--ov-cap-pad-f`. A factor rather than a length so the inset follows the
/// padding token; at the inherit padding of 10 it is today's 12 px.
const CARD_CAP_PAD_FACTOR: f64 = 1.2;
/// The control row's side-column floor at size_scale 1 while the cancel button
/// is on the row: `--ov-side-min` 22. It holds that button, so it drops to 0
/// with it and the row shrinks to what is left. `.sbase` scales it like every
/// other length here, so the floors and the card shrink together.
const CARD_SIDE_MIN_W: f64 = 22.0;
/// The left column's own content at size_scale 1 while the waveform is on the
/// row: `--ov-dot-col-w` 12, the recording dot plus `.sbase-l`'s 5 px inset.
/// Wider than 0 and narrower than the side floor, so it decides the left
/// column only once the cancel button is gone.
const CARD_DOT_COL_W: f64 = 12.0;
/// The recording dot itself at size_scale 1: `--ov-dot-w` 7. A row with no
/// waveform drops the inset above, so the dot sits one padding from the card's
/// left edge as the cancel button does from the right, and is the left column
/// there.
const CARD_DOT_W: f64 = 7.0;
/// The number of waveform bars: `WAVE_BARS` in `RecordingOverlay.tsx`, and the
/// `9` in `--ov-wave-slot-w`.
const CARD_WAVE_BARS: f64 = 9.0;
/// `.swave`'s right padding at size_scale 1: `--ov-wave-pad-r` 8. The waveform
/// lane plus this is the centre column.
const CARD_WAVE_PAD_R: f64 = 8.0;
/// The blur radius of Flat's drop shadow at size_scale 1: `--ov-shadow-blur`
/// 20. Derived, not a token: `shadow_strength` and `shadow_offset_y` are the
/// two controls, and a third for the blur would make the shadow a project.
/// With the offset it is how far the shadow reaches, the window's shadow slack.
const CARD_SHADOW_BLUR: f64 = 20.0;

/// The card's morph between two shapes, in milliseconds: `--ov-morph-ms`,
/// `.scard`'s width and border-radius transitions. The overlay webview reads
/// the same property and sends it with every card-shape report, so a native
/// window-frame animation under Glass runs as long as the CSS morph under
/// Flat. That animation is opt-in (`HANDY_GLASS_MORPH=1`) because the window
/// leads WebKit's repaint by a frame or two and the blur shows through.
pub(crate) const CARD_MORPH_MS: u32 = 460;
/// The card's fade, in milliseconds: `--ov-fade-ms`, `.ov-fade`'s opacity
/// transition. Under Glass the blur fades over the same span, in and out, or
/// it reads as a separate object.
pub(crate) const CARD_FADE_MS: u32 = 200;
/// The largest morph duration a card-shape report may ask for, in ms, roughly
/// four times [`CARD_MORPH_MS`]. Anything beyond is a bug or a hostile call,
/// and would pin a native window animation on screen after the card settled.
pub(crate) const MAX_CARD_MORPH_MS: u32 = 2000;

/// Window slack for the pill, in logical points: 218 + 38 = 256 wide,
/// 42 + 4 = 46 tall, exactly the window this overlay has always used.
const COMPACT_SLACK: (f64, f64) = (38.0, 4.0);
/// Window slack for the Live panel: 394 + 6 = 400 wide, 118 + 2 = 120 tall,
/// again today's window.
const LIVE_SLACK: (f64, f64) = (6.0, 2.0);

// The four windows this overlay has always used, kept as named scale-1
// fixtures for the tests below and `overlay`'s Windows placement tests.
// `#[cfg(test)]` because no production path reads a fixed size any more.
// Every window is computed from the card and the resolved size scale.
/// The border on both sides at the inherit `border_width`, for the fixtures
/// below. Today's card, hairline included.
#[cfg(test)]
const CARD_BORDER_INHERIT: f64 = card_border(crate::overlay_theme::BORDER_WIDTH_INHERIT);
/// The padding an unset token inherits, as a float for the fixtures below.
#[cfg(test)]
const CARD_PADDING_INHERIT: f64 = crate::overlay_theme::PADDING_INHERIT as f64;
/// The control row at the inherit padding: today's 40 px.
#[cfg(test)]
const CARD_ROW_H_INHERIT: f64 = CARD_ROW_CORE_H + 2.0 * CARD_PADDING_INHERIT;
/// The Live card's content at the inherit padding: today's 116 px.
#[cfg(test)]
const CARD_LIVE_CONTENT_H_INHERIT: f64 =
    CARD_ROW_H_INHERIT + CARD_CAP_MAX_H + CARD_CAP_PAD_FACTOR * CARD_PADDING_INHERIT;
/// Compact window width at size_scale 1.
#[cfg(test)]
pub(crate) const OVERLAY_WIDTH: f64 =
    CARD_COMPACT_CONTENT_W + CARD_BORDER_INHERIT + COMPACT_SLACK.0;
/// Compact window height at size_scale 1.
#[cfg(test)]
pub(crate) const OVERLAY_HEIGHT: f64 = CARD_ROW_H_INHERIT + CARD_BORDER_INHERIT + COMPACT_SLACK.1;
/// Live window width at size_scale 1.
#[cfg(test)]
pub(crate) const OVERLAY_STREAM_WIDTH: f64 =
    CARD_LIVE_CONTENT_W + CARD_BORDER_INHERIT + LIVE_SLACK.0;
/// Live window height at size_scale 1.
#[cfg(test)]
pub(crate) const OVERLAY_STREAM_HEIGHT: f64 =
    CARD_LIVE_CONTENT_H_INHERIT + CARD_BORDER_INHERIT + LIVE_SLACK.1;

/// Which card an [`OverlayCardShape`] is a shape of: the pill, or the panel
/// that opens out of one.
///
/// The only distinction the native geometry makes. Every other UI-state
/// difference happens inside the card, at a size the window already covers.
/// The pill never opens, so under Flat its window covers the widest pill; the
/// panel opens, so its window covers the open panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Card {
    /// The pill: Minimal in every state, and the compact working card either
    /// overlay style shows while it finalizes.
    Pill,
    /// The Live panel, and the pill it opens from and collapses back to.
    Panel,
}

impl Card {
    /// The widest footprint this card can reach at size_scale 1, border and
    /// padding included. What the window covers under Flat, where the CSS
    /// morph happens inside a window sized for the widest card. Under Glass
    /// the window equals the exact current [`OverlayCardShape`] instead.
    ///
    /// Takes the whole [`CardMetrics`] because the height follows the padding
    /// token and the width the element gap, as well as the border. The caller
    /// applies the scale, so every number returned is at size_scale 1.
    ///
    /// The widest card is always a working or open shape, so the resting
    /// shapes' `max()` against the row ([`CardMetrics::resting_content_width`])
    /// cannot reach here: the row fits the working pill at every combination of
    /// tokens, which `the_waveform_never_outgrows_the_working_pill` pins.
    fn widest_footprint(self, metrics: &CardMetrics) -> (f64, f64) {
        let border = card_border(metrics.border_width);
        let gaps = metrics.gap_width();
        match self {
            Self::Pill => (
                CARD_COMPACT_CONTENT_W + gaps + border,
                metrics.row_height() + border,
            ),
            Self::Panel => (
                CARD_LIVE_CONTENT_W + gaps + border,
                metrics.live_content_height() + border,
            ),
        }
    }

    /// The transparent slack between the card's footprint and the edge of the
    /// native overlay window, in logical points.
    ///
    /// It keeps a card mid-morph from being clipped by the overlay page's
    /// `overflow: hidden`. Zero under Glass, where the window rectangle is the
    /// card and the glass view fills it, so any slack would paint blur outside
    /// it. No `#[cfg(target_os = "macos")]` needed, because off macOS the
    /// effective Material is never Glass. Only a `support()` with
    /// `available: true` produces it, so anywhere but a Glass-capable Mac this
    /// is today's per-card slack.
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
/// overlay style can reach and the CSS morph happens inside it. Under Glass
/// the window is sized from it, because the slack is zero and the Live panel's
/// open/collapsed morph is a webview decision (streamed text and phase) that
/// Rust cannot otherwise see.
///
/// One shape per distinct `.scard` class combination in
/// `RecordingOverlay.tsx`. The footprints mirror the `--ov-*` block in
/// `RecordingOverlay.css`, pinned by
/// `overlay_window_constants_match_overlay_css`, and must agree with
/// `cardShape()` in `src/overlay/cardShape.ts`, pinned by
/// `initial_card_shape_matches_card_shape_ts`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OverlayCardShape {
    /// The resting Minimal pill, `.scard.compact`.
    CompactRest,
    /// The Minimal working pill, `.scard.compact.cworking`, at the same
    /// footprint as Live's collapsed working pill.
    CompactWorking,
    /// The Live pill before it opens or collapses, `.scard`.
    LivePill,
    /// The Live panel collapsed to its working pill, `.scard.working`.
    LiveWorking,
    /// The Live panel expanded, `.scard.open`.
    LiveOpen,
}

impl OverlayCardShape {
    /// Declaration order, mirroring the `#[repr(u8)]` discriminants. The table
    /// [`Self::from_u8`] indexes into.
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
    /// From the frontend: `"recording"` renders `.scard.compact` with
    /// `working = false` -> [`Self::CompactRest`]; `"transcribing"` and
    /// `"processing"` render `.scard.compact.cworking` ->
    /// [`Self::CompactWorking`]; `"streaming"` resets its stream state, so it
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

    /// This shape's exact footprint at size_scale 1, border and padding
    /// included; the window equals it under Glass. Every number comes from the
    /// `--ov-*` block in RecordingOverlay.css. The four pill shapes are the
    /// control row, the open panel adds the live-text region and its inset.
    fn card_footprint(self, metrics: &CardMetrics) -> (f64, f64) {
        let border = card_border(metrics.border_width);
        let row = metrics.row_height();
        let gaps = metrics.gap_width();
        let (content_width, content_height) = match self {
            // Only the two resting shapes are sized to their contents: they
            // shrink when the waveform is hidden and grow when the row outgrows
            // their tuned width. The other three stay put, tuned to translated
            // labels and to the transcript.
            Self::CompactRest => (
                metrics.resting_content_width(CARD_COMPACT_REST_CONTENT_W),
                row,
            ),
            Self::CompactWorking => (CARD_COMPACT_CONTENT_W + gaps, row),
            Self::LivePill => (metrics.resting_content_width(CARD_LIVE_PILL_CONTENT_W), row),
            Self::LiveWorking => (CARD_COMPACT_CONTENT_W + gaps, row),
            Self::LiveOpen => (CARD_LIVE_CONTENT_W + gaps, metrics.live_content_height()),
        };
        (content_width + border, content_height + border)
    }

    /// The corner-radius factor CSS applies for this shape's `.scard...`
    /// class: full radius token for the pill and the two compact states, 3/4
    /// for the Live working pill, 2/3 for the open panel. CSS and the native
    /// `CALayer` radius both clamp visually to half the shorter side, so they
    /// agree without either rounding first. Deliberately left unrounded.
    fn radius_factor(self) -> f64 {
        match self {
            Self::CompactRest | Self::CompactWorking | Self::LivePill => 1.0,
            Self::LiveWorking => 0.75,
            Self::LiveOpen => 2.0 / 3.0,
        }
    }

    /// Recover a shape from the byte the overlay's card-shape atomic stores.
    /// A value outside the five discriminants (never produced here) falls back
    /// to [`Self::CompactRest`] rather than panicking.
    pub(crate) fn from_u8(value: u8) -> Self {
        Self::ALL
            .get(value as usize)
            .copied()
            .unwrap_or(Self::CompactRest)
    }

    /// How long the glass view fades out when the overlay hides, matched to
    /// how this card leaves the screen. The pill fades with its container over
    /// `--ov-fade-ms`; the Live card is unmounted, so its blur goes at once
    /// rather than linger over an empty window.
    pub(crate) fn glass_fade_out_ms(self) -> u32 {
        match self.card() {
            Card::Pill => CARD_FADE_MS,
            Card::Panel => 0,
        }
    }
}

/// The theme tokens the card's rectangle is a function of, clamped once.
///
/// They always travel together. The size scale zooms every length, the border
/// width adds two strokes, the padding insets the control row on every edge
/// and carries the Live card's breathing room, the element gap widens every
/// card by two gaps, the two switches and the waveform's own two lengths
/// decide how wide the row's contents make a resting pill, and the radius
/// rounds the corners the blur matches. So `from_theme` builds them once and
/// callers ask it for a window size or a corner radius, rather than passing
/// them one at a time to functions that must remember the clamps.
///
/// The constructor clamps, so no caller has to and the geometry never trusts
/// an unclamped number. Same bounds as the accessors on [`OverlayTheme`], so
/// window and card cannot disagree about a token's limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CardMetrics {
    scale: f64,
    border_width: u16,
    padding: u16,
    element_gap: u16,
    waveform_gap: u16,
    waveform_width: u16,
    show_waveform: bool,
    show_cancel: bool,
    radius_px: f64,
}

/// The room a Flat card's drop shadow needs around it, clamped once.
///
/// Beside [`CardMetrics`] rather than inside it, because these two describe
/// the space around the card's rectangle, not the rectangle. The window grows
/// into that space on all four sides, by the full slack everywhere except the
/// anchored screen edge, where it takes only the room the card already had. So
/// the card does not move, and the window still stops short of the usable edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShadowMetrics {
    /// 0.00 to 1.00, resolved against the Material's own inherit. Under Flat
    /// it shapes the CSS shadow; under Glass it only switches macOS's.
    strength: f64,
    /// How far the shadow falls below the card, px at size_scale 1. Flat only.
    offset_y: u16,
}

impl ShadowMetrics {
    /// The shadow a resolved theme asks for on the Material being painted.
    ///
    /// The Material is a parameter because `shadow_strength`'s inherit depends
    /// on it: none under Flat, macOS's own under Glass.
    pub(crate) fn from_theme(theme: &OverlayTheme, material: Material) -> Self {
        Self {
            strength: theme.shadow_strength(material),
            offset_y: theme.shadow_offset_y(),
        }
    }

    /// Whether the card casts a shadow at all. Zero strength is the Flat
    /// inherit, and the whole point of the token under Glass.
    fn casts(&self) -> bool {
        self.strength > 0.0
    }

    /// The shadow's reach from the card's edge at size_scale 1: the fixed blur
    /// radius plus the offset, the worst case on the side it falls towards.
    fn reach(&self) -> f64 {
        CARD_SHADOW_BLUR + f64::from(self.offset_y)
    }

    /// The strength the native window shadow is switched from. Only
    /// `overlay_glass` reads it, and only under Glass.
    pub(crate) fn strength(&self) -> f64 {
        self.strength
    }
}

impl CardMetrics {
    /// The resolved `radius` token in px at `size_scale` 1 when the theme sets
    /// none, the CSS token's default (`--ov-radius: 24px`, `RecordingOverlay.css`).
    const DEFAULT_RADIUS_PX: f64 = 24.0;

    /// The metrics a resolved theme asks for.
    ///
    /// `size_scale`, `border_width` and `padding` come through the theme's
    /// accessors, which clamp. `OverlayTheme` has no accessor for `radius`, so
    /// this reads the public field and clamps it here.
    pub(crate) fn from_theme(theme: &OverlayTheme) -> Self {
        Self {
            scale: theme.size_scale(),
            border_width: theme.border_width(),
            padding: theme.padding(),
            element_gap: theme.element_gap(),
            waveform_gap: theme.waveform_gap(),
            waveform_width: theme.waveform_width(),
            show_waveform: theme.show_waveform(),
            show_cancel: theme.show_cancel(),
            radius_px: theme
                .radius
                .map(f64::from)
                .unwrap_or(Self::DEFAULT_RADIUS_PX)
                .min(f64::from(crate::overlay_theme::RADIUS_MAX)),
        }
    }

    /// Overlay window size (logical points) for a card shape under a Material.
    ///
    /// Under Glass the window equals the shape's exact footprint, since there
    /// the window is the card. Under Flat it covers the widest card that
    /// shape's [`Card`] can reach, since the CSS morph happens inside it.
    ///
    /// Rounds the scaled card up before adding the slack, so the window is
    /// never a fraction short of its card and every result is a whole point.
    ///
    /// The shadow's own slack goes on last. It is the full slack on the two
    /// horizontal sides and on the side away from the screen edge the overlay
    /// is anchored to; on the anchored side it is `edge_slack`, only the room
    /// already between the card and the usable edge. So the card keeps the
    /// screen position it has with no shadow, the window never reaches past the
    /// usable edge, and the shadow's faint tail is clipped there rather than
    /// covering the Dock or the menu bar. At the Flat inherit strength of 0
    /// both are 0 and every window is today's.
    pub(crate) fn window_size(
        &self,
        shape: OverlayCardShape,
        material: Material,
        shadow: ShadowMetrics,
        edge_slack: f64,
    ) -> (f64, f64) {
        let (card_width, card_height) = match material {
            Material::Glass => shape.card_footprint(self),
            Material::Flat => shape.card().widest_footprint(self),
        };
        let (slack_width, slack_height) = shape.card().slack(material);
        let shadow_slack = self.shadow_slack(material, shadow);
        // Clamped rather than trusted. The edge slack is resolved against the
        // Material actually rendered, and `OverlayWindowState::initial` sizes a
        // Glass theme's first hidden window as Flat, so the two can disagree
        // for exactly that one window.
        let edge_slack = edge_slack.clamp(0.0, shadow_slack);

        (
            (card_width * self.scale).ceil() + slack_width + 2.0 * shadow_slack,
            (card_height * self.scale).ceil() + slack_height + shadow_slack + edge_slack,
        )
    }

    /// The transparent margin the card's own shadow needs, per window side, in
    /// whole logical points.
    ///
    /// Zero under Glass, where the window is the card and macOS draws the
    /// shadow outside it, and zero at strength 0, so a theme with no shadow
    /// gets today's window byte for byte. Scaled and ceiled here because CSS
    /// cannot round: `--ov-shadow-slack` is one of the two custom properties
    /// the apply layer writes scaled, so `.ov-stage`'s padding and this number
    /// are the same integer, never a fraction of a point.
    ///
    /// The three sides away from the anchored screen edge take this; the
    /// anchored side takes [`shadow_edge_slack`] instead.
    pub(crate) fn shadow_slack(&self, material: Material, shadow: ShadowMetrics) -> f64 {
        if material == Material::Glass || !shadow.casts() {
            return 0.0;
        }
        (shadow.reach() * self.scale).ceil()
    }

    /// The control row's height at size_scale 1: the core the stylesheet
    /// declares plus one `padding` above and below, what `.sbase`'s `height`
    /// and four-sided `padding` add up to.
    fn row_height(&self) -> f64 {
        CARD_ROW_CORE_H + 2.0 * f64::from(self.padding)
    }

    /// The Live card's content height at size_scale 1: the control row, the
    /// live-text region, and the inset above the text, a multiple of `padding`
    /// rather than a length of its own.
    fn live_content_height(&self) -> f64 {
        self.row_height() + CARD_CAP_MAX_H + CARD_CAP_PAD_FACTOR * f64::from(self.padding)
    }

    /// What the row's two element gaps add to a card's own width at
    /// size_scale 1. Both sides of every `max()` below carry it, so the gap
    /// widens a card without changing which won.
    ///
    /// Every card tuned to its contents pays for two, because its row keeps all
    /// three columns. [`Self::row_gap_width`] is what the row itself measures,
    /// which differs once a column is gone.
    fn gap_width(&self) -> f64 {
        2.0 * f64::from(self.element_gap)
    }

    /// What the element gaps add to the control row with a waveform on it at
    /// size_scale 1: one on each side of the waveform, or one alone once the
    /// cancel button has taken the right column away. `--ov-row-gaps` in the
    /// stylesheet, which the apply layer drops to 1 with that button.
    fn row_gap_width(&self) -> f64 {
        let gaps = if self.show_cancel { 2.0 } else { 1.0 };
        gaps * f64::from(self.element_gap)
    }

    /// The row's side-column floor at size_scale 1 (`--ov-side-min`): 22 px
    /// while the cancel button is on the row, 0 without it, since holding that
    /// button is the floor's only reason.
    fn side_min(&self) -> f64 {
        if self.show_cancel {
            CARD_SIDE_MIN_W
        } else {
            0.0
        }
    }

    /// The control row's own width at size_scale 1 with an empty centre column:
    /// the padding, the two element gaps, the left column (the bare dot, or the
    /// side floor if wider) and the right column. This is `--ov-bare-w`, what a
    /// resting pill shrinks to with no waveform, 64 at every inherit value.
    ///
    /// The dot is bare here, not [`CARD_DOT_COL_W`]: with no waveform to pad
    /// the row, `.scard.nowave .sbase-l` drops the dot's inset so the dot and
    /// the cancel button sit one padding from their own card edges. With the
    /// button gone too the pill is a square, as wide as the row is tall, with
    /// the dot centred by `.scard.nowave.nocancel .sbase`; the element gaps
    /// have nothing to fall between and do not widen it.
    fn bare_row_width(&self) -> f64 {
        if !self.show_cancel {
            return self.row_height();
        }
        let side = self.side_min();
        2.0 * f64::from(self.padding) + self.gap_width() + side.max(CARD_DOT_W) + side
    }

    /// The centre column at size_scale 1: the waveform's nine bars and eight
    /// gaps (`--ov-wave-slot-w`) plus `.swave`'s right padding, and the same
    /// padding on its left once the cancel button is gone (`--ov-wave-pad-l`):
    /// the exact row would otherwise put the lane flush against the dot.
    fn wave_column_width(&self) -> f64 {
        let left_pad = if self.show_cancel {
            0.0
        } else {
            CARD_WAVE_PAD_R
        };
        CARD_WAVE_BARS * f64::from(self.waveform_width)
            + (CARD_WAVE_BARS - 1.0) * f64::from(self.waveform_gap)
            + left_pad
            + CARD_WAVE_PAD_R
    }

    /// The whole control row's width at size_scale 1 with the waveform on it:
    /// `--ov-row-w`. Only [`Self::resting_content_width`] asks, and only while
    /// the waveform is shown.
    ///
    /// Written out rather than added to [`Self::bare_row_width`], because a row
    /// with a waveform on it keeps the dot's inset: the two sums differ in
    /// their left and centre columns and in how many element gaps they count.
    ///
    /// Without the cancel button the sum collapses to the padding, the dot
    /// column, one gap and the waveform lane, since the side floor is 0 there
    /// and the right column is gone with it.
    fn row_width(&self) -> f64 {
        let side = self.side_min();
        2.0 * f64::from(self.padding)
            + self.row_gap_width()
            + side.max(CARD_DOT_COL_W)
            + side
            + self.wave_column_width()
    }

    /// A resting shape's content width at size_scale 1: its tuned width plus
    /// the element gaps, but never narrower than the row it has to hold, and
    /// the row alone once either element is hidden.
    ///
    /// The tuned width is the room the pill was designed for, worth holding
    /// only while every element is on the row. Without the cancel button it
    /// would keep an empty column where the button was, so the pill becomes
    /// exactly the row; without the waveform too, the bare row.
    ///
    /// The `max()` fixes a clipping this overlay always had. At the maximum
    /// padding, waveform width and waveform gap the row measures 186, past the
    /// 172 the resting pill was tuned to, and `overflow: hidden` cut the
    /// waveform off. `.scard.compact`'s own `max()` mirrors this exactly.
    fn resting_content_width(&self, tuned: f64) -> f64 {
        if !self.show_waveform {
            return self.bare_row_width();
        }
        if !self.show_cancel {
            return self.row_width();
        }
        (tuned + self.gap_width()).max(self.row_width())
    }

    /// A shape's corner radius in px, mirroring the CSS
    /// `calc(var(--ov-radius) * var(--ov-scale) * factor)`. Unrounded like the
    /// CSS, so CALayer and CSS agree (both clamp visually to half the shorter
    /// side).
    fn corner_radius(&self, shape: OverlayCardShape) -> f64 {
        self.radius_px * self.scale * shape.radius_factor()
    }
}

/// How far the overlay window may grow past the card on the screen edge it is
/// anchored to, in whole logical points.
///
/// The placement rule: the window takes the full [`CardMetrics::shadow_slack`]
/// on the three sides away from that edge, and here only `room`, the gap the
/// card already has to the usable edge (the bottom offset above the Dock, or
/// the top offset below the menu bar). So the card keeps the screen position it
/// has with no shadow, the window stops at the usable edge, and the shadow's
/// faint tail is clipped by the window boundary there.
///
/// Its result travels to the overlay page on the resolved theme, so `.ov-stage`
/// insets the card by the same number and the two cannot disagree about where
/// the card lands. `room` is the one thing the geometry cannot know: it comes
/// from `overlay::anchored_edge_room`, which owns the per-platform offsets.
pub(crate) fn shadow_edge_slack(theme: &OverlayTheme, material: Material, room: f64) -> f64 {
    CardMetrics::from_theme(theme)
        .shadow_slack(material, ShadowMetrics::from_theme(theme, material))
        .min(room.max(0.0))
}

/// Everything a theme configures the native overlay window from.
///
/// Its size, the blur's macOS material and the blur's corner radius are
/// functions of these values and nothing else, so two theme deliveries that
/// agree on all of them ask the window for what it already is. That is what
/// [`native_update_needed`] tests, and what lets a colour edit skip the
/// main-thread hop, the AppKit material write and the resize.
///
/// Its position is not. It also follows the `overlay_position` setting and the
/// monitor the card is placed on, neither of which a theme can move and
/// neither of which reaches the window through here. Position and style
/// commands go through the unconditional `update_overlay_position`, and every
/// show repositions anyway, so skipping a theme delivery leaves the window
/// where the last show or position update put it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OverlayWindowState {
    shape: OverlayCardShape,
    material: Material,
    glass: GlassAppearance,
    metrics: CardMetrics,
    shadow: ShadowMetrics,
    /// The anchored screen edge's share of the shadow slack, resolved once by
    /// [`shadow_edge_slack`] and carried so the window and the overlay page
    /// inset the card by the same number. A field rather than recomputed here,
    /// because the room it is capped by belongs to the screen, not the theme.
    edge_slack: f64,
}

impl OverlayWindowState {
    /// The state `resolved` asks for, at the card shape on screen right now.
    pub(crate) fn new(shape: OverlayCardShape, resolved: &ResolvedOverlayTheme) -> Self {
        Self::for_material(shape, resolved, resolved.effective_material)
    }

    /// The state a window being created starts in: the resting compact pill
    /// under Flat, whatever the theme asks for.
    ///
    /// Glass cannot be in effect yet. `overlay_glass::install` needs the window
    /// this is sizing, so it has not run, and the first show resolves again and
    /// resizes once Glass is installed. The resolved edge slack is Glass's 0
    /// for the same reason, which `window_size` clamps against rather than
    /// reads, so this window is at worst one shadow-slack short on its anchored
    /// side until that first show.
    pub(crate) fn initial(resolved: &ResolvedOverlayTheme) -> Self {
        Self::for_material(OverlayCardShape::CompactRest, resolved, Material::Flat)
    }

    fn for_material(
        shape: OverlayCardShape,
        resolved: &ResolvedOverlayTheme,
        material: Material,
    ) -> Self {
        Self {
            shape,
            material,
            glass: GlassAppearance::from_theme(&resolved.theme),
            metrics: CardMetrics::from_theme(&resolved.theme),
            shadow: ShadowMetrics::from_theme(&resolved.theme, material),
            edge_slack: resolved.shadow_edge_slack,
        }
    }

    /// The window this state wants, in logical points.
    pub(crate) fn window_size(&self) -> (f64, f64) {
        self.metrics
            .window_size(self.shape, self.material, self.shadow, self.edge_slack)
    }

    /// This window's reach past the card towards the anchored screen edge. The
    /// placement subtracts it, so the card lands where it would with no shadow.
    pub(crate) fn edge_slack(&self) -> f64 {
        self.edge_slack
            .clamp(0.0, self.metrics.shadow_slack(self.material, self.shadow))
    }

    /// The `shadow_strength` the native window shadow is switched from.
    pub(crate) fn shadow_strength(&self) -> f64 {
        self.shadow.strength()
    }

    /// The corner radius the native blur has to take to match the card.
    pub(crate) fn corner_radius(&self) -> f64 {
        self.metrics.corner_radius(self.shape)
    }
}

/// Whether the native window has to be touched at all.
///
/// Pure, so "only a change to something the window is built from" is a test
/// rather than a reading of the call site. An unknown previous state always
/// needs the update, because the window may have just been created.
pub(crate) fn native_update_needed(
    previous: Option<&OverlayWindowState>,
    next: &OverlayWindowState,
) -> bool {
    previous != Some(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend_source::{
        css_declaration, css_ms, css_number, css_px, css_rule, tsx_const, APPLY_LAYER_TS,
        OVERLAY_CSS, OVERLAY_TSX,
    };
    use crate::overlay_glass::GlassAppearance;
    use crate::overlay_theme::{
        GlassMaterial, GlassStyle, HexColor, BORDER_WIDTH_INHERIT, BORDER_WIDTH_MAX,
        ELEMENT_GAP_INHERIT, ELEMENT_GAP_MAX, PADDING_INHERIT, PADDING_MAX, SHADOW_OFFSET_Y_MAX,
        SIZE_SCALE_MIN, WAVEFORM_GAP_MAX, WAVEFORM_WIDTH_MAX, WAVEFORM_WIDTH_MIN,
    };

    /// The metrics of today's card: nothing set, everything inherited.
    fn inherit_metrics() -> CardMetrics {
        CardMetrics::from_theme(&OverlayTheme::default())
    }

    /// A CSS value with its whitespace collapsed and its brackets closed up, so
    /// a pinned declaration survives Prettier moving a line break inside a
    /// `calc()` while still failing on a changed term.
    fn collapsed(value: &str) -> String {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace("( ", "(")
            .replace(" )", ")")
    }

    /// The shadow an unset theme asks for on `material`: none under Flat, and
    /// macOS's own (which needs no slack) under Glass.
    fn inherit_shadow(material: Material) -> ShadowMetrics {
        ShadowMetrics::from_theme(&OverlayTheme::default(), material)
    }

    /// The window `shape` gets at `scale` under `material`, with the inherit
    /// border width and no shadow. The shorthand the dimension tests below use.
    fn window(shape: OverlayCardShape, scale: f64, material: Material) -> (f64, f64) {
        window_at(shape, scale, material, BORDER_WIDTH_INHERIT)
    }

    /// The metrics a theme built from `tokens` asks for; the rest inherited.
    fn metrics_of(tokens: OverlayTheme) -> CardMetrics {
        CardMetrics::from_theme(&tokens)
    }

    /// The same, for tests that vary the border width, through the constructor's
    /// clamp, so an out-of-range scale or width acts as one from a theme.
    fn window_at(
        shape: OverlayCardShape,
        scale: f64,
        material: Material,
        border_width: u16,
    ) -> (f64, f64) {
        metrics_for(scale, border_width).window_size(shape, material, inherit_shadow(material), 0.0)
    }

    fn metrics_for(scale: f64, border_width: u16) -> CardMetrics {
        CardMetrics::from_theme(&OverlayTheme {
            size_scale: Some(scale),
            border_width: Some(border_width),
            ..OverlayTheme::default()
        })
    }

    /// The window `shape` gets at `scale` under `material` with `padding` set, the
    /// third token. The border keeps its inherit width, so only padding moves it.
    fn window_padded(
        shape: OverlayCardShape,
        scale: f64,
        material: Material,
        padding: u16,
    ) -> (f64, f64) {
        CardMetrics::from_theme(&OverlayTheme {
            size_scale: Some(scale),
            padding: Some(padding),
            ..OverlayTheme::default()
        })
        .window_size(shape, material, inherit_shadow(material), 0.0)
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
            shadow: inherit_shadow(Material::Flat),
            edge_slack: 0.0,
        }
    }

    /// What makes live editing cheap: an accent, a text colour or a waveform
    /// gap cannot move the native window, so a delivery carrying only those
    /// must not hop to the main thread.
    #[test]
    fn a_theme_that_cannot_move_the_window_needs_no_native_update() {
        let state = window_state();
        assert!(!native_update_needed(Some(&state), &state.clone()));
    }

    /// ...and everything the window is built from still gets through, one
    /// field at a time.
    ///
    /// The test destructures the state exhaustively, with no `..`, and each
    /// binding derives the variant that changes that one field, under a local
    /// `deny(unused_variables)`. A field added to `OverlayWindowState`,
    /// `CardMetrics` or `GlassAppearance` stops this test compiling until it is
    /// named here, and naming it without using it is an error too.
    ///
    /// What it cannot catch: a value the window is built from that never became
    /// a field of this state. Only `update_overlay_position_on_main` and
    /// `overlay_glass` tell you that; this test keeps the claim internally honest.
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
                    padding,
                    element_gap,
                    waveform_gap,
                    waveform_width,
                    show_waveform,
                    show_cancel,
                    radius_px,
                },
            shadow: ShadowMetrics { strength, offset_y },
            edge_slack,
        } = base.clone();

        // Each variant differs from `base` in exactly the field it names, and
        // is derived from that field's value so it cannot equal it.
        let glass_variant = |glass: GlassAppearance| OverlayWindowState {
            glass,
            ..base.clone()
        };
        let metrics_variant = |metrics: CardMetrics| OverlayWindowState {
            metrics,
            ..base.clone()
        };
        let shadow_variant = |shadow: ShadowMetrics| OverlayWindowState {
            shadow,
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
                padding: padding + 2,
                ..base.metrics
            }),
            metrics_variant(CardMetrics {
                radius_px: radius_px + 12.0,
                ..base.metrics
            }),
            // The element gap widens every card, and the four below decide how
            // wide the row's contents make a resting pill, so all five reach
            // the window under Glass.
            metrics_variant(CardMetrics {
                element_gap: element_gap + 8,
                ..base.metrics
            }),
            metrics_variant(CardMetrics {
                waveform_gap: waveform_gap + 1,
                ..base.metrics
            }),
            metrics_variant(CardMetrics {
                waveform_width: waveform_width + 1,
                ..base.metrics
            }),
            metrics_variant(CardMetrics {
                show_waveform: !show_waveform,
                ..base.metrics
            }),
            metrics_variant(CardMetrics {
                show_cancel: !show_cancel,
                ..base.metrics
            }),
            // The shadow changes the window, not the card: on both axes and in
            // its placement, so both halves must reach the native side.
            shadow_variant(ShadowMetrics {
                strength: strength + 0.5,
                ..base.shadow
            }),
            shadow_variant(ShadowMetrics {
                offset_y: offset_y + 4,
                ..base.shadow
            }),
            // The anchored edge's share of that slack is not a token at all:
            // it follows the overlay position and the platform, and it changes
            // the window's height as well as where it is placed.
            OverlayWindowState {
                edge_slack: edge_slack + 8.0,
                ..base.clone()
            },
            // The four Glass-only tokens. Only the liquid engine reads the
            // style and tint, but all are live property writes on the
            // installed view, so a change to any has to reach it.
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

    /// A freshly created or re-created window always needs the update, whatever
    /// the theme says.
    #[test]
    fn an_unconfigured_window_always_needs_the_native_update() {
        assert!(native_update_needed(None, &window_state()));
    }

    /// A window state answers every native question itself, from its shape, so
    /// it holds [`CardMetrics`] and [`ShadowMetrics`] not loose numbers.
    #[test]
    fn a_window_state_sizes_and_rounds_its_own_card() {
        let state = window_state();
        assert_eq!(
            state.window_size(),
            window(OverlayCardShape::LivePill, 1.0, Material::Flat)
        );
        assert_eq!(state.shadow_strength(), 0.0);
        assert_eq!(state.edge_slack(), 0.0);
        assert_eq!(
            state.corner_radius(),
            inherit_metrics().corner_radius(OverlayCardShape::LivePill)
        );
    }

    /// The "defaults reproduce today's overlay exactly" pin. With no size
    /// token set, every state gets the window this overlay has always used;
    /// the literals are the sizes overlay.rs hardcoded before the token.
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
    /// gives the same window, so the shapes below are picked for variety.
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
    /// Flat window covers the card at every scale. The footprints are the
    /// token contract's own numbers, not a repeat of the arithmetic tested.
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

    /// Under Glass the window equals the card exactly, zero slack, at every
    /// shape and every 0.05 step of scale from 0.80 to 1.50. The footprints
    /// are spelled out rather than read from `card_footprint()`, so a drifted
    /// constant cannot agree with itself, and no expectation adds a slack term.
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
        // The same five cards at the token's maximum, also as literals. The
        // one scale where every width lands on a whole point, with no
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
    /// window grows with it. The card is content-box, so each extra px of
    /// stroke costs two px of footprint before the scale. The expectations are
    /// the token table's arithmetic, never `card_footprint()` again.
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
                    let (card_w, card_h) = shape.card_footprint(&metrics_for(scale, width));
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

    /// `padding` is the third token that moves the native window, and the only
    /// one that moves a single axis: the card grows taller, never wider. The
    /// expectations are the spacing model written out. The control row is a
    /// 20 px core plus one padding above and below, the Live card adds the
    /// 64 px text region and an inset of 1.2 paddings, both carry the inherit
    /// hairline on each edge, and the Flat slack goes on after the scale.
    #[test]
    fn overlay_dimensions_follow_the_padding() {
        for (padding, scale, compact, live) in [
            (0, 0.80, (213.0, 22.0), (322.0, 71.0)),
            (0, 1.00, (256.0, 26.0), (400.0, 88.0)),
            (0, 1.50, (365.0, 37.0), (597.0, 131.0)),
            (10, 0.80, (213.0, 38.0), (322.0, 97.0)),
            (10, 1.00, (256.0, 46.0), (400.0, 120.0)),
            (10, 1.50, (365.0, 67.0), (597.0, 179.0)),
            (20, 0.80, (213.0, 54.0), (322.0, 122.0)),
            (20, 1.00, (256.0, 66.0), (400.0, 152.0)),
            (20, 1.50, (365.0, 97.0), (597.0, 227.0)),
        ] {
            assert_eq!(
                window_padded(
                    OverlayCardShape::CompactRest,
                    scale,
                    Material::Flat,
                    padding
                ),
                compact,
                "compact at padding {padding}, scale {scale}"
            );
            assert_eq!(
                window_padded(OverlayCardShape::LiveOpen, scale, Material::Flat, padding),
                live,
                "Live at padding {padding}, scale {scale}"
            );
        }

        // Under Glass the window is the card, so the same growth shows without
        // the slack. The Live panel's content is 40 + 64 + 12 at the inherit
        // padding, 20 + 64 at zero and 60 + 64 + 24 at the bound.
        for (padding, expected) in [
            (0, (394.0, 86.0)),
            (10, (394.0, 118.0)),
            (20, (394.0, 150.0)),
        ] {
            assert_eq!(
                window_padded(OverlayCardShape::LiveOpen, 1.0, Material::Glass, padding),
                expected,
                "Glass at padding {padding}"
            );
        }

        // Setting the padding to its inherit value by hand is the window an
        // unset padding already gets: today's card, exactly.
        for shape in OverlayCardShape::ALL {
            assert_eq!(
                window_padded(shape, 1.0, Material::Flat, PADDING_INHERIT),
                window(shape, 1.0, Material::Flat),
                "{shape:?}"
            );
        }

        // A padding that reached the geometry unclamped is treated as the
        // bound, exactly as an out-of-range scale or border width is.
        assert_eq!(
            window_padded(OverlayCardShape::LiveOpen, 1.0, Material::Glass, 99),
            window_padded(
                OverlayCardShape::LiveOpen,
                1.0,
                Material::Glass,
                PADDING_MAX
            )
        );

        // And whatever the padding, Glass's window equals its card and Flat's
        // covers it, the invariant zero slack rests on.
        for padding in [0, PADDING_INHERIT, PADDING_MAX] {
            for shape in OverlayCardShape::ALL {
                let (glass_w, glass_h) = window_padded(shape, 1.0, Material::Glass, padding);
                let (flat_w, flat_h) = window_padded(shape, 1.0, Material::Flat, padding);
                assert!(
                    flat_w >= glass_w && flat_h >= glass_h,
                    "{shape:?} at padding {padding}: Flat window \
                     {flat_w}x{flat_h} does not cover the card {glass_w}x{glass_h}"
                );
            }
        }
    }

    /// The corner radius follows the CSS `calc()` exactly, per-shape factor
    /// included, and an out-of-range radius is clamped like every other token.
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

    /// A show seeds the card shape from its state alone, and every Live state
    /// seeds the pill; the webview opens or collapses the panel afterwards, at
    /// two other radii. So nothing that rounds the blur after a show may carry
    /// the seeded radius — it has to re-read the shape on screen, which is why
    /// `overlay::schedule_glass_fallback_reveal` derives its own. Carrying it
    /// left the open panel's border tracing a tighter arc than the glass under
    /// it, the pill's radius on the panel's window.
    #[test]
    fn the_shape_a_live_show_seeds_does_not_carry_the_morphed_card_s_radius() {
        let metrics = inherit_metrics();
        let seeded = OverlayCardShape::initial_for("streaming");
        assert_eq!(seeded, OverlayCardShape::LivePill);
        for morphed in [OverlayCardShape::LiveOpen, OverlayCardShape::LiveWorking] {
            assert_ne!(
                metrics.corner_radius(seeded),
                metrics.corner_radius(morphed),
                "{morphed:?} would be rounded for the pill the show seeded"
            );
        }
    }

    /// Why `waveform_width` stops at 6 and `padding` at 20: at every token's
    /// maximum the control row still fits the *working* pill, so no spacing
    /// token can force that window wider whatever else changes.
    ///
    /// It never fitted the *resting* pill: at those three maxima the row
    /// measures 186 against the 172 it was tuned to, and `overflow: hidden`
    /// clipped the waveform. The resting pill now takes the row when the row is
    /// wider, so the second assertion is that the metrics produce exactly the
    /// two numbers this test computes, not that the row fits.
    ///
    /// Both halves are gap-invariant: the element gap adds two gaps to the row
    /// and to every card, so it cancels.
    #[test]
    fn the_waveform_never_outgrows_the_working_pill() {
        // The frontend-owned inputs: the bar count from the component that
        // renders them, and the row's own lengths from the CSS the card is
        // painted with.
        let bars = tsx_const(OVERLAY_TSX, "const WAVE_BARS = ");
        let wave_padding_right = css_px(OVERLAY_CSS, "--ov-wave-pad-r");
        let side_column = css_px(OVERLAY_CSS, "--ov-side-min");
        let dot_column = css_px(OVERLAY_CSS, "--ov-dot-col-w");

        // (element gap, the row it measures): the expectation is the literal
        // the arithmetic lands on, so a drifted length shows as a number rather
        // than as two sides agreeing on the wrong one.
        for (gap, row) in [(0, 186.0), (ELEMENT_GAP_MAX, 266.0)] {
            let widest = metrics_of(OverlayTheme {
                padding: Some(PADDING_MAX),
                waveform_gap: Some(WAVEFORM_GAP_MAX),
                waveform_width: Some(WAVEFORM_WIDTH_MAX),
                element_gap: Some(gap),
                ..OverlayTheme::default()
            });
            // The same width built from the frontend's own numbers, so a length
            // changed in the CSS or in the component fails here too.
            let from_the_frontend = bars * f64::from(WAVEFORM_WIDTH_MAX)
                + (bars - 1.0) * f64::from(WAVEFORM_GAP_MAX)
                + wave_padding_right
                + 2.0 * f64::from(PADDING_MAX)
                + 2.0 * f64::from(gap)
                + side_column.max(dot_column)
                + side_column;

            assert_eq!(from_the_frontend, row, "gap {gap}");
            assert_eq!(widest.row_width(), row, "gap {gap}");
            assert!(
                row <= CARD_COMPACT_CONTENT_W + 2.0 * f64::from(gap),
                "the row at every maximum is {row}, wider than the working pill"
            );

            // …and the resting pill, which it does not fit, grows to hold it
            // instead of clipping it.
            assert!(row > CARD_COMPACT_REST_CONTENT_W + 2.0 * f64::from(gap));
            assert_eq!(
                widest.resting_content_width(CARD_COMPACT_REST_CONTENT_W),
                row,
                "gap {gap}"
            );
            assert_eq!(
                widest.resting_content_width(CARD_LIVE_PILL_CONTENT_W),
                row,
                "gap {gap}"
            );
        }

        // At the inherit values the row is well inside the resting pill, so
        // nothing about today's card moves: 20 + 22 + 68 + 22.
        let inherit = inherit_metrics();
        assert_eq!(inherit.row_width(), 132.0);
        assert_eq!(
            inherit.resting_content_width(CARD_COMPACT_REST_CONTENT_W),
            CARD_COMPACT_REST_CONTENT_W
        );
        assert_eq!(
            inherit.resting_content_width(CARD_LIVE_PILL_CONTENT_W),
            CARD_LIVE_PILL_CONTENT_W
        );

        // And the narrowest bar still draws: 2px at the smallest scale is
        // 1.6px, which WebKit still paints.
        assert!(f64::from(WAVEFORM_WIDTH_MIN) * SIZE_SCALE_MIN >= 1.0);
    }

    /// The "a new token changes nothing until it is asked for" pin, for all
    /// five at once: an all-inherit theme gets the four windows this overlay
    /// has always used, on both Materials, and no shadow slack anywhere.
    #[test]
    fn a_shadowless_theme_keeps_todays_windows() {
        let metrics = inherit_metrics();
        for material in [Material::Flat, Material::Glass] {
            let shadow = inherit_shadow(material);
            assert_eq!(metrics.shadow_slack(material, shadow), 0.0, "{material:?}");
        }

        // Flat: the two windows overlay.rs used to hardcode.
        for shape in [
            OverlayCardShape::CompactRest,
            OverlayCardShape::CompactWorking,
        ] {
            assert_eq!(
                window(shape, 1.0, Material::Flat),
                (256.0, 46.0),
                "{shape:?}"
            );
        }
        for shape in [
            OverlayCardShape::LivePill,
            OverlayCardShape::LiveWorking,
            OverlayCardShape::LiveOpen,
        ] {
            assert_eq!(
                window(shape, 1.0, Material::Flat),
                (400.0, 120.0),
                "{shape:?}"
            );
        }

        // Glass: the five card footprints, unchanged by the row's new tokens.
        for (shape, expected) in [
            (OverlayCardShape::CompactRest, (174.0, 42.0)),
            (OverlayCardShape::CompactWorking, (218.0, 42.0)),
            (OverlayCardShape::LivePill, (186.0, 42.0)),
            (OverlayCardShape::LiveWorking, (218.0, 42.0)),
            (OverlayCardShape::LiveOpen, (394.0, 118.0)),
        ] {
            assert_eq!(window(shape, 1.0, Material::Glass), expected, "{shape:?}");
        }

        // Setting each new token to its own inherit value by hand is the same
        // window as leaving it unset. `shadow_strength` is the one whose
        // inherit splits per Material, so the by-hand theme carries that
        // Material's value and supplies the shadow, not the unset theme.
        for material in [Material::Flat, Material::Glass] {
            let theme = OverlayTheme {
                shadow_strength: Some(match material {
                    Material::Flat => 0.0,
                    Material::Glass => 1.0,
                }),
                shadow_offset_y: Some(4),
                show_waveform: Some(true),
                show_cancel: Some(true),
                element_gap: Some(ELEMENT_GAP_INHERIT),
                ..OverlayTheme::default()
            };
            let by_hand = CardMetrics::from_theme(&theme);
            let shadow = ShadowMetrics::from_theme(&theme, material);
            // Derived rather than written as 0, so the derivation is pinned to
            // "no shadow, no room taken" at a room big enough to hide a bug.
            let edge_slack = shadow_edge_slack(&theme, material, 40.0);
            assert_eq!(edge_slack, 0.0, "{material:?}");
            for shape in OverlayCardShape::ALL {
                assert_eq!(
                    by_hand.window_size(shape, material, shadow, edge_slack),
                    window(shape, 1.0, material),
                    "{shape:?} {material:?}"
                );
            }
        }
    }

    /// A Flat shadow grows the window by its full reach on the two horizontal
    /// sides and on the side away from the anchored screen edge, and by the
    /// room the card already had on the anchored side. The expectations are
    /// `blur + offset` scaled and rounded up, written out, never
    /// `shadow_slack()` again.
    #[test]
    fn the_flat_shadow_grows_the_window_on_every_side() {
        // (offset, scale, slack per side, the resting pill's window with the
        // anchored edge given no room, and with it given the full slack): the
        // blur is a fixed 20, and the windows without a shadow are 256 x 46 at
        // scale 1, 213 x 38 at 0.8 and 365 x 67 at 1.5. Only the height differs
        // between the two, by exactly the slack.
        for (offset, scale, slack, clipped, roomy) in [
            (0, 1.00, 20.0, (296.0, 66.0), (296.0, 86.0)),
            (4, 1.00, 24.0, (304.0, 70.0), (304.0, 94.0)),
            (16, 1.00, 36.0, (328.0, 82.0), (328.0, 118.0)),
            (4, 0.80, 20.0, (253.0, 58.0), (253.0, 78.0)), // 19.2, rounded up
            (4, 1.50, 36.0, (437.0, 103.0), (437.0, 139.0)),
            (16, 1.50, 54.0, (473.0, 121.0), (473.0, 175.0)),
            (0, 0.80, 16.0, (245.0, 54.0), (245.0, 70.0)),
        ] {
            let metrics = metrics_of(OverlayTheme {
                size_scale: Some(scale),
                ..OverlayTheme::default()
            });
            let shadow = ShadowMetrics::from_theme(
                &OverlayTheme {
                    shadow_strength: Some(0.5),
                    shadow_offset_y: Some(offset),
                    ..OverlayTheme::default()
                },
                Material::Flat,
            );
            assert_eq!(
                metrics.shadow_slack(Material::Flat, shadow),
                slack,
                "offset {offset} at {scale}"
            );

            assert_eq!(
                metrics.window_size(OverlayCardShape::CompactRest, Material::Flat, shadow, 0.0),
                clipped,
                "offset {offset} at {scale}, no room at the anchored edge"
            );
            assert_eq!(
                metrics.window_size(OverlayCardShape::CompactRest, Material::Flat, shadow, slack),
                roomy,
                "offset {offset} at {scale}, room to spare"
            );
            // More room than the shadow reaches is still only the shadow's
            // reach; a window wider than its own slack would sit off centre.
            assert_eq!(
                metrics.window_size(
                    OverlayCardShape::CompactRest,
                    Material::Flat,
                    shadow,
                    slack + 100.0
                ),
                roomy,
                "offset {offset} at {scale}, capped at the slack"
            );
        }

        // The four windows at the inherit offset and a mid strength, as
        // literals: today's plus 24 on each horizontal side, 24 above and the
        // macOS bottom offset's 15 below.
        let shadow = ShadowMetrics::from_theme(
            &OverlayTheme {
                shadow_strength: Some(0.5),
                ..OverlayTheme::default()
            },
            Material::Flat,
        );
        let metrics = inherit_metrics();
        assert_eq!(
            metrics.window_size(OverlayCardShape::CompactRest, Material::Flat, shadow, 15.0),
            (304.0, 85.0)
        );
        assert_eq!(
            metrics.window_size(OverlayCardShape::LiveOpen, Material::Flat, shadow, 15.0),
            (448.0, 159.0)
        );

        // The strength only decides whether there is a shadow at all; the
        // slack is the offset's and the scale's.
        for strength in [0.01, 0.35, 1.00] {
            let any = ShadowMetrics::from_theme(
                &OverlayTheme {
                    shadow_strength: Some(strength),
                    ..OverlayTheme::default()
                },
                Material::Flat,
            );
            assert_eq!(
                metrics.shadow_slack(Material::Flat, any),
                24.0,
                "{strength}"
            );
        }
        // …and at zero there is none, which is what Flat inherits.
        let off = ShadowMetrics::from_theme(
            &OverlayTheme {
                shadow_strength: Some(0.0),
                shadow_offset_y: Some(SHADOW_OFFSET_Y_MAX),
                ..OverlayTheme::default()
            },
            Material::Flat,
        );
        assert_eq!(metrics.shadow_slack(Material::Flat, off), 0.0);
    }

    /// Under Glass the window is the card, so it can have no slack at all: the
    /// shadow is macOS's, drawn outside the window, and `shadow_strength` only
    /// switches it. A Glass window is byte-identical at every strength.
    #[test]
    fn glass_keeps_zero_slack_whatever_the_shadow_says() {
        let metrics = inherit_metrics();
        for strength in [0.0, 0.01, 0.5, 1.0] {
            for offset in [0, 4, SHADOW_OFFSET_Y_MAX] {
                let shadow = ShadowMetrics::from_theme(
                    &OverlayTheme {
                        shadow_strength: Some(strength),
                        shadow_offset_y: Some(offset),
                        ..OverlayTheme::default()
                    },
                    Material::Glass,
                );
                assert_eq!(metrics.shadow_slack(Material::Glass, shadow), 0.0);
                for shape in OverlayCardShape::ALL {
                    assert_eq!(
                        metrics.window_size(shape, Material::Glass, shadow, 0.0),
                        window(shape, 1.0, Material::Glass),
                        "{shape:?} at {strength}/{offset}"
                    );
                }
            }
        }
    }

    /// The anchored screen edge takes only the room the card already had
    /// there, so the card keeps the position it has with no shadow at all.
    #[test]
    fn the_shadow_takes_only_the_room_the_card_has_at_the_anchored_edge() {
        // (scale, offset, the full slack, and the edge slack at each of the
        // three rooms the placement can offer: macOS Bottom's 15, macOS Top's
        // 46 less a 30 point menu bar, and Windows and Linux's flush 4).
        for (scale, offset, slack, at_15, at_16, at_4) in [
            (1.00, 4, 24.0, 15.0, 16.0, 4.0),
            (1.00, 16, 36.0, 15.0, 16.0, 4.0),
            (0.80, 0, 16.0, 15.0, 16.0, 4.0),
            (0.80, 4, 20.0, 15.0, 16.0, 4.0),
            (1.50, 16, 54.0, 15.0, 16.0, 4.0),
        ] {
            let theme = OverlayTheme {
                size_scale: Some(scale),
                shadow_strength: Some(0.5),
                shadow_offset_y: Some(offset),
                ..OverlayTheme::default()
            };
            assert_eq!(
                CardMetrics::from_theme(&theme).shadow_slack(
                    Material::Flat,
                    ShadowMetrics::from_theme(&theme, Material::Flat)
                ),
                slack,
                "{scale} / {offset}"
            );
            for (room, expected) in [(15.0, at_15), (16.0, at_16), (4.0, at_4)] {
                assert_eq!(
                    shadow_edge_slack(&theme, Material::Flat, room),
                    expected,
                    "{scale} / {offset} in {room} points of room"
                );
            }
            // A room the placement could never offer takes nothing extra, and
            // one that has been eaten away entirely takes nothing at all.
            assert_eq!(shadow_edge_slack(&theme, Material::Flat, 400.0), slack);
            assert_eq!(shadow_edge_slack(&theme, Material::Flat, 0.0), 0.0);
            assert_eq!(shadow_edge_slack(&theme, Material::Flat, -20.0), 0.0);
            // Under Glass there is no CSS shadow to make room for, whatever
            // the room is.
            assert_eq!(shadow_edge_slack(&theme, Material::Glass, 40.0), 0.0);
        }

        // And a theme that asks for no shadow takes no room on either Material,
        // which is what keeps today's windows byte-identical.
        for material in [Material::Flat, Material::Glass] {
            assert_eq!(
                shadow_edge_slack(&OverlayTheme::default(), material, 40.0),
                0.0,
                "{material:?}"
            );
        }
    }

    /// The card's screen rectangle is byte-identical with and without a
    /// shadow. The window grows and moves; the card does not.
    ///
    /// Stated here on the size alone, one shape at a time: the window gains the
    /// full slack on the side away from the anchored screen edge and the edge
    /// slack on the anchored side, and `.ov-stage` insets the card by those
    /// numbers, so the card's height and its distance from both window edges
    /// are what they were. `overlay.rs`'s
    /// `the_card_keeps_its_screen_position_when_a_shadow_is_added` completes it
    /// by placing that window on a screen.
    #[test]
    fn a_shadow_moves_the_window_around_the_card_not_the_card() {
        let theme = OverlayTheme {
            shadow_strength: Some(1.0),
            shadow_offset_y: Some(16),
            ..OverlayTheme::default()
        };
        let metrics = CardMetrics::from_theme(&theme);
        let shadow = ShadowMetrics::from_theme(&theme, Material::Flat);
        let slack = metrics.shadow_slack(Material::Flat, shadow);
        assert_eq!(slack, 36.0);

        for shape in OverlayCardShape::ALL {
            let bare = window(shape, 1.0, Material::Flat);
            for room in [0.0, 4.0, 15.0, 40.0, 100.0] {
                let edge = shadow_edge_slack(&theme, Material::Flat, room);
                let shadowed = metrics.window_size(shape, Material::Flat, shadow, edge);
                // The card sits `slack` from three window edges and `edge` from
                // the anchored one, so what is left is exactly the bare window.
                assert_eq!(
                    (shadowed.0 - 2.0 * slack, shadowed.1 - slack - edge),
                    bare,
                    "{shape:?} in {room} points of room"
                );
            }
        }
    }

    /// The slack is computed in two languages, and CSS cannot round, so the
    /// apply layer writes `--ov-shadow-slack` already scaled and ceiled and
    /// `--ov-shadow-edge-slack` from the number Rust derived. Both sides must
    /// therefore start from the same blur radius and the same expressions;
    /// this reads the frontend for all of it.
    #[test]
    fn the_shadow_slack_is_the_apply_layers() {
        assert_eq!(css_px(OVERLAY_CSS, "--ov-shadow-blur"), CARD_SHADOW_BLUR);
        assert_eq!(
            tsx_const(APPLY_LAYER_TS, "export const SHADOW_BLUR_PX = "),
            CARD_SHADOW_BLUR
        );
        assert!(
            APPLY_LAYER_TS.contains("Math.ceil((SHADOW_BLUR_PX + offsetY) * (scale ?? 1))"),
            "the apply layer no longer ceils blur + offset, scaled"
        );
        // The anchored side is Rust's number, taken off the resolved theme and
        // never re-derived from a platform table over there.
        assert!(
            APPLY_LAYER_TS.contains("const carried = resolved.shadow_edge_slack;"),
            "the apply layer no longer reads the edge slack off the resolved theme"
        );
        assert!(
            APPLY_LAYER_TS
                .contains("vars[\"--ov-shadow-edge-slack\"] = `${edgeSlack(resolved, slack)}px`;"),
            "the apply layer no longer writes the edge slack"
        );
        // Both are taken verbatim, unscaled, because they arrive scaled. A
        // `* var(--ov-scale)` here would square the factor. The stage pads all
        // four sides with the full slack and gives the anchored one back, which
        // is the bottom by default and the top under `.ov-stage.top`.
        let stage = css_rule(OVERLAY_CSS, ".ov-stage {");
        assert_eq!(css_declaration(stage, "padding"), "var(--ov-shadow-slack)");
        assert_eq!(
            css_declaration(stage, "padding-bottom"),
            "var(--ov-shadow-edge-slack)"
        );
        let top = css_rule(OVERLAY_CSS, ".ov-stage.top {");
        assert_eq!(
            css_declaration(top, "padding-top"),
            "var(--ov-shadow-edge-slack)"
        );
        assert_eq!(
            css_declaration(top, "padding-bottom"),
            "var(--ov-shadow-slack)"
        );
    }

    /// `element_gap` widens every card by two gaps, on both Materials, and
    /// nothing else about the card moves.
    #[test]
    fn element_gap_widens_every_pill_by_twice_the_gap() {
        // (gap, the five Glass windows' widths, the two Flat ones): every
        // width written out, the footprints at gap 0 and the same five two
        // gaps wider. `ELEMENT_GAP_MAX` is 40, so its row adds 80.
        for (gap, glass, flat) in [
            (0, [174.0, 218.0, 186.0, 218.0, 394.0], [256.0, 400.0]),
            (8, [190.0, 234.0, 202.0, 234.0, 410.0], [272.0, 416.0]),
            (20, [214.0, 258.0, 226.0, 258.0, 434.0], [296.0, 440.0]),
            (
                ELEMENT_GAP_MAX,
                [254.0, 298.0, 266.0, 298.0, 474.0],
                [336.0, 480.0],
            ),
        ] {
            let metrics = metrics_of(OverlayTheme {
                element_gap: Some(gap),
                ..OverlayTheme::default()
            });

            // Glass sizes the window to the exact card, so the growth shows per
            // shape.
            for (shape, width, height) in [
                (OverlayCardShape::CompactRest, glass[0], 42.0),
                (OverlayCardShape::CompactWorking, glass[1], 42.0),
                (OverlayCardShape::LivePill, glass[2], 42.0),
                (OverlayCardShape::LiveWorking, glass[3], 42.0),
                (OverlayCardShape::LiveOpen, glass[4], 118.0),
            ] {
                assert_eq!(
                    metrics.window_size(
                        shape,
                        Material::Glass,
                        inherit_shadow(Material::Glass),
                        0.0
                    ),
                    (width, height),
                    "{shape:?} at gap {gap}"
                );
            }

            // Flat's window covers the widest card of the family, which grew
            // by the same two gaps.
            for (shape, width, height) in [
                (OverlayCardShape::CompactRest, flat[0], 46.0),
                (OverlayCardShape::LiveOpen, flat[1], 120.0),
            ] {
                assert_eq!(
                    metrics.window_size(shape, Material::Flat, inherit_shadow(Material::Flat), 0.0),
                    (width, height),
                    "{shape:?} at gap {gap}"
                );
            }
        }

        // A gap that reached the geometry unclamped is treated as the bound.
        assert_eq!(
            metrics_of(OverlayTheme {
                element_gap: Some(999),
                ..OverlayTheme::default()
            }),
            metrics_of(OverlayTheme {
                element_gap: Some(ELEMENT_GAP_MAX),
                ..OverlayTheme::default()
            })
        );
    }

    /// Hiding the waveform shrinks the two resting shapes to the row that is
    /// left. The working pill and the open panel are tuned to translated labels
    /// and the transcript, so their widths hold and every morph stays a grow.
    #[test]
    fn hiding_the_waveform_shrinks_only_the_resting_shapes() {
        let hidden = metrics_of(OverlayTheme {
            show_waveform: Some(false),
            ..OverlayTheme::default()
        });
        let glass_shadow = inherit_shadow(Material::Glass);

        // 2 x 10 padding + 22 + 22 side columns, plus the hairline per edge.
        // The dot is inside the left floor, its inset dropped with the
        // waveform, so it costs the row nothing here.
        for shape in [OverlayCardShape::CompactRest, OverlayCardShape::LivePill] {
            assert_eq!(
                hidden.window_size(shape, Material::Glass, glass_shadow, 0.0),
                (66.0, 42.0),
                "{shape:?}"
            );
        }
        for (shape, expected) in [
            (OverlayCardShape::CompactWorking, (218.0, 42.0)),
            (OverlayCardShape::LiveWorking, (218.0, 42.0)),
            (OverlayCardShape::LiveOpen, (394.0, 118.0)),
        ] {
            assert_eq!(
                hidden.window_size(shape, Material::Glass, glass_shadow, 0.0),
                expected,
                "{shape:?}"
            );
        }

        // Losing the cancel button drops the row's 22 px side floor and the
        // right column it held, so a resting pill is the row that is left
        // rather than its tuned width with a gap where the button was:
        // 2 x 10 padding + the 12 px dot column + the 68 px waveform lane,
        // plus the hairline per edge. The Live pill lands on the same row.
        let no_cancel = metrics_of(OverlayTheme {
            show_cancel: Some(false),
            ..OverlayTheme::default()
        });
        for shape in [OverlayCardShape::CompactRest, OverlayCardShape::LivePill] {
            assert_eq!(
                no_cancel.window_size(shape, Material::Glass, glass_shadow, 0.0),
                (110.0, 42.0),
                "{shape:?}"
            );
        }
        // With the waveform gone too, the pill is a square as wide as the row
        // is tall, 20 + 2 x 10, with the dot centred in it.
        let bare = metrics_of(OverlayTheme {
            show_waveform: Some(false),
            show_cancel: Some(false),
            ..OverlayTheme::default()
        });
        for shape in [OverlayCardShape::CompactRest, OverlayCardShape::LivePill] {
            assert_eq!(
                bare.window_size(shape, Material::Glass, glass_shadow, 0.0),
                (42.0, 42.0),
                "{shape:?}"
            );
        }

        // Every morph out of a shrunken resting shape is still a grow, which
        // the card's width transition and the native frame morph both assume.
        for metrics in [hidden, bare] {
            for (from, to) in [
                (
                    OverlayCardShape::CompactRest,
                    OverlayCardShape::CompactWorking,
                ),
                (OverlayCardShape::LivePill, OverlayCardShape::LiveWorking),
                (OverlayCardShape::LivePill, OverlayCardShape::LiveOpen),
            ] {
                let (narrow, _) = metrics.window_size(from, Material::Glass, glass_shadow, 0.0);
                let (wide, _) = metrics.window_size(to, Material::Glass, glass_shadow, 0.0);
                assert!(narrow <= wide, "{from:?} -> {to:?} is not a grow");
            }
        }
    }

    /// Every combination of the two switches, at both ends of the size scale,
    /// written out. Under Glass the window is the card, so these are the
    /// numbers the native frame is built from, and the stylesheet reaches the
    /// same ones by scaling the same sums.
    ///
    /// The scale is what the row's own floors used to ignore. A card at 0.8 is
    /// its size-scale-1 sum times 0.8, but `.sbase`'s two `minmax()` floors
    /// stayed at 22 px, so with the waveform hidden the row needed 44 px of
    /// floors inside a content box of 35.2 and the cancel button was pushed
    /// through the card's right edge, where `overflow: hidden` cut it. The
    /// last block is that arithmetic.
    #[test]
    fn the_resting_pill_measures_the_row_it_is_left_with() {
        // (waveform, cancel, scale, the resting Minimal pill's window, the Live
        // pill's), at size scale 1 and at 0.80, the smallest a theme can ask
        // for. 172 and 184 hold their tuned widths only while every element is
        // on the row; hide either and the pill is the row itself, which is 100
        // with the waveform alone, 64 with the button alone and a 40 square
        // with the dot alone; every one plus the hairline per edge.
        for (waveform, cancel, scale, compact, live) in [
            (true, true, 1.0, (174.0, 42.0), (186.0, 42.0)),
            (true, true, SIZE_SCALE_MIN, (140.0, 34.0), (149.0, 34.0)),
            (true, false, 1.0, (110.0, 42.0), (110.0, 42.0)),
            (true, false, SIZE_SCALE_MIN, (88.0, 34.0), (88.0, 34.0)),
            (false, true, 1.0, (66.0, 42.0), (66.0, 42.0)),
            (false, true, SIZE_SCALE_MIN, (53.0, 34.0), (53.0, 34.0)),
            (false, false, 1.0, (42.0, 42.0), (42.0, 42.0)),
            (false, false, SIZE_SCALE_MIN, (34.0, 34.0), (34.0, 34.0)),
        ] {
            let metrics = metrics_of(OverlayTheme {
                size_scale: Some(scale),
                show_waveform: Some(waveform),
                show_cancel: Some(cancel),
                ..OverlayTheme::default()
            });
            for (shape, expected) in [
                (OverlayCardShape::CompactRest, compact),
                (OverlayCardShape::LivePill, live),
            ] {
                assert_eq!(
                    metrics.window_size(
                        shape,
                        Material::Glass,
                        inherit_shadow(Material::Glass),
                        0.0
                    ),
                    expected,
                    "{shape:?} at scale {scale}, waveform {waveform}, cancel {cancel}"
                );
            }
        }

        // The row the stylesheet lays out inside the narrowest of those
        // windows: the card at 0.8 less one scaled padding per side.
        let row_box = 64.0 * SIZE_SCALE_MIN - 2.0 * f64::from(PADDING_INHERIT) * SIZE_SCALE_MIN;
        assert_eq!(row_box, 35.2);
        assert!(
            2.0 * CARD_SIDE_MIN_W * SIZE_SCALE_MIN <= row_box,
            "the scaled floors no longer fit the row they sit in"
        );
        assert!(
            2.0 * CARD_SIDE_MIN_W > row_box,
            "floors left at size scale 1 used to overflow this row, and this is the pin"
        );

        // And the same question for the row the cancel button leaves behind,
        // which is two tracks with the waveform in the right one. The card is
        // exactly that row, so the space beside the waveform is the dot column
        // and nothing else, which is what the left track floors at. Every term
        // carries --ov-scale, so it holds at 0.80 as it does here.
        let no_cancel = metrics_of(OverlayTheme {
            show_cancel: Some(false),
            ..OverlayTheme::default()
        });
        // 108: the lane carries its 8 px left padding here, inside the column.
        assert_eq!(no_cancel.row_width(), 108.0);
        assert_eq!(
            no_cancel.row_width()
                - 2.0 * f64::from(PADDING_INHERIT)
                - no_cancel.wave_column_width(),
            CARD_DOT_COL_W
        );

        // The right column takes one of the row's two element gaps with it, so
        // a resting pill without the button pays for one: 100 + 40, plus the
        // hairline per edge.
        let one_gap = metrics_of(OverlayTheme {
            element_gap: Some(ELEMENT_GAP_MAX),
            show_cancel: Some(false),
            ..OverlayTheme::default()
        });
        assert_eq!(
            one_gap.window_size(
                OverlayCardShape::CompactRest,
                Material::Glass,
                inherit_shadow(Material::Glass),
                0.0
            ),
            (150.0, 42.0)
        );
    }

    /// Under Flat the window covers the widest card its family can reach, and
    /// neither switch changes that card, so a Flat window never moves.
    #[test]
    fn flat_windows_ignore_the_visibility_tokens() {
        for show_waveform in [true, false] {
            for show_cancel in [true, false] {
                let metrics = metrics_of(OverlayTheme {
                    show_waveform: Some(show_waveform),
                    show_cancel: Some(show_cancel),
                    ..OverlayTheme::default()
                });
                for shape in OverlayCardShape::ALL {
                    assert_eq!(
                        metrics.window_size(
                            shape,
                            Material::Flat,
                            inherit_shadow(Material::Flat),
                            0.0
                        ),
                        window(shape, 1.0, Material::Flat),
                        "{shape:?} with waveform {show_waveform}, cancel {show_cancel}"
                    );
                }
            }
        }
    }

    /// Every variant survives the round trip through the byte the atomic stores,
    /// and a byte this module cannot produce falls back instead of panicking.
    #[test]
    fn card_shape_round_trips_through_its_byte() {
        for (index, shape) in OverlayCardShape::ALL.into_iter().enumerate() {
            assert_eq!(OverlayCardShape::from_u8(shape as u8), shape, "{shape:?}");
            // ALL is the table from_u8 indexes into, so it stays in discriminant
            // order.
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

    /// The row's side floors carry the size scale, as every other length in
    /// the card does.
    ///
    /// [`CardMetrics`] adds the row up at size scale 1 and multiplies the sum
    /// once, so a floor the stylesheet leaves at 22 px is wider than the row
    /// the window was built for at any scale below 1. That is what pushed the
    /// cancel button through the right edge of a waveless pill at 0.80.
    #[test]
    fn the_rows_side_floors_carry_the_size_scale() {
        assert_eq!(
            collapsed(css_declaration(
                css_rule(OVERLAY_CSS, ".sbase {"),
                "grid-template-columns"
            )),
            "minmax(calc(var(--ov-side-min) * var(--ov-scale)), 1fr) auto \
             minmax(calc(var(--ov-side-min) * var(--ov-scale)), 1fr)"
        );

        // Without the cancel button the row is two tracks and one gap, which is
        // the row `row_width` adds up. The left track floors at the dot column,
        // not at `--ov-side-min`, which went to 0 with the button; scaled, like
        // every length around it.
        assert_eq!(
            collapsed(css_declaration(
                css_rule(OVERLAY_CSS, ".scard.nocancel .sbase {"),
                "grid-template-columns"
            )),
            "minmax(calc(var(--ov-dot-col-w) * var(--ov-scale)), 1fr) auto"
        );
        // The column that held the button is gone rather than empty, so the row
        // has one gap to pay for. Only the resting shapes carry `.nocancel`, so
        // the open Live panel keeps the column its timer sits in.
        assert_eq!(
            css_declaration(
                css_rule(OVERLAY_CSS, ".scard.nocancel .sbase-r {"),
                "display"
            ),
            "none"
        );

        // With the dot alone on the row both remaining tracks collapse to their
        // contents and the row centres them in the square `bare_row_width`
        // hands back, so the dot sits the same distance from every edge.
        let dot_only = css_rule(OVERLAY_CSS, ".scard.nowave.nocancel .sbase {");
        assert_eq!(
            collapsed(css_declaration(dot_only, "grid-template-columns")),
            "auto auto"
        );
        assert_eq!(css_declaration(dot_only, "justify-content"), "center");
        assert_eq!(css_declaration(dot_only, "column-gap"), "0");
    }

    /// The card constants above and the `--ov-*` block in RecordingOverlay.css
    /// are two copies of the same geometry. This reads the shipped CSS and
    /// fails naming the variable that drifted, rather than clipping a card.
    #[test]
    fn overlay_window_constants_match_overlay_css() {
        // The card's content lengths, which the footprints are built from
        // before the border is added.
        assert_eq!(css_px(OVERLAY_CSS, "--ov-work-w"), CARD_COMPACT_CONTENT_W);
        assert_eq!(css_px(OVERLAY_CSS, "--ov-open-w"), CARD_LIVE_CONTENT_W);

        // The control row as the stylesheet builds it: `.sbase`'s core plus
        // the padding token on every edge. The CSS declares the inherit
        // padding, and both sides put the same two into the height.
        let css_padding = css_px(OVERLAY_CSS, "--ov-pad");
        assert_eq!(css_padding, f64::from(PADDING_INHERIT));
        assert_eq!(css_px(OVERLAY_CSS, "--ov-row-core-h"), CARD_ROW_CORE_H);
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-row-core-h") + 2.0 * css_padding,
            inherit_metrics().row_height()
        );
        // That sum is the row's real height only if `.sbase` writes it the same
        // way, so the rule is pinned too: border-box, so the height covers the
        // padding rather than sitting inside it, and one padding per edge.
        let sbase = css_rule(OVERLAY_CSS, ".sbase {");
        assert_eq!(css_declaration(sbase, "box-sizing"), "border-box");
        assert_eq!(
            css_declaration(sbase, "height"),
            "calc((var(--ov-row-core-h) + 2 * var(--ov-pad)) * var(--ov-scale))"
        );

        // The Live card on top: the text region, and the inset above it, which
        // the stylesheet also writes as a multiple of the padding, not a length.
        assert_eq!(css_px(OVERLAY_CSS, "--ov-cap-max-h"), CARD_CAP_MAX_H);
        assert_eq!(
            css_number(OVERLAY_CSS, "--ov-cap-pad-f"),
            CARD_CAP_PAD_FACTOR
        );
        // The factor only reaches the card through `--ov-cap-pad-y`, a product
        // rather than a length, so the sum below would still add up if the
        // stylesheet gave the inset a size of its own.
        assert_eq!(
            css_declaration(OVERLAY_CSS, "--ov-cap-pad-y"),
            "calc(var(--ov-pad) * var(--ov-cap-pad-f))"
        );
        assert_eq!(
            inherit_metrics().row_height()
                + css_px(OVERLAY_CSS, "--ov-cap-max-h")
                + css_number(OVERLAY_CSS, "--ov-cap-pad-f") * css_padding,
            inherit_metrics().live_content_height()
        );
        // And the two heights they add up to at the inherit padding are the
        // ones this overlay has always drawn.
        assert_eq!(inherit_metrics().row_height(), 40.0);
        assert_eq!(inherit_metrics().live_content_height(), 116.0);
        // The two shapes only Glass sizes to exactly: the resting compact
        // pill and the Live pill before it opens or collapses.
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-rest-w"),
            CARD_COMPACT_REST_CONTENT_W
        );
        assert_eq!(css_px(OVERLAY_CSS, "--ov-pill-w"), CARD_LIVE_PILL_CONTENT_W);

        // The stroke `.scard` draws on each edge. The CSS declares the inherit
        // width, this side doubles it, and the card is content-box, so the
        // footprint carries one per edge.
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-border-w"),
            f64::from(BORDER_WIDTH_INHERIT)
        );
        assert_eq!(card_border(BORDER_WIDTH_INHERIT), CARD_BORDER_INHERIT);
        // The waveform bar's width. No footprint uses it, but it follows the
        // same "the CSS declares the inherit value" rule, so the tab's slider
        // and the stylesheet cannot drift.
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
        // properties, the overlay webview reads --ov-morph-ms to tell the
        // backend how long to animate the window frame, and the native reveal
        // and fade-out read the constants below, so all three are one number.
        assert_eq!(
            css_ms(OVERLAY_CSS, "--ov-morph-ms"),
            f64::from(CARD_MORPH_MS)
        );
        assert_eq!(css_ms(OVERLAY_CSS, "--ov-fade-ms"), f64::from(CARD_FADE_MS));

        // Both morphs are a grow, so the widest card per shape family is the
        // one sized from above.
        assert!(css_px(OVERLAY_CSS, "--ov-rest-w") <= css_px(OVERLAY_CSS, "--ov-work-w"));
        assert!(css_px(OVERLAY_CSS, "--ov-pill-w") <= css_px(OVERLAY_CSS, "--ov-open-w"));

        // The row's own lengths, which decide how wide a resting pill has to
        // be. The stylesheet declares each one and this side reads it, so the
        // `max()` in `.scard.compact` and `resting_content_width` cannot
        // disagree about a number.
        assert_eq!(css_px(OVERLAY_CSS, "--ov-side-min"), CARD_SIDE_MIN_W);
        assert_eq!(css_px(OVERLAY_CSS, "--ov-dot-col-w"), CARD_DOT_COL_W);
        assert_eq!(css_px(OVERLAY_CSS, "--ov-dot-w"), CARD_DOT_W);
        // The left column is the dot plus an inset, and the two sums below take
        // one part each: the row with a waveform on it keeps the inset, the bare
        // row drops it with the rule that hides the waveform. So the
        // stylesheet's own three numbers have to add up, and the dot and the
        // inset have to reach the card through those very properties.
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-dot-w") + css_px(OVERLAY_CSS, "--ov-dot-inset"),
            CARD_DOT_COL_W
        );
        assert_eq!(
            css_declaration(css_rule(OVERLAY_CSS, ".sdot {"), "width"),
            "calc(var(--ov-dot-w) * var(--ov-scale))"
        );
        assert_eq!(
            css_declaration(css_rule(OVERLAY_CSS, ".sbase-l {"), "padding-left"),
            "calc(var(--ov-dot-inset) * var(--ov-scale))"
        );
        assert_eq!(
            css_declaration(
                css_rule(OVERLAY_CSS, ".scard.nowave .sbase-l {"),
                "padding-left"
            ),
            "0"
        );
        assert_eq!(css_px(OVERLAY_CSS, "--ov-wave-pad-r"), CARD_WAVE_PAD_R);
        // The lane's left padding is the stylesheet's 0 until the apply layer
        // hands it the right padding with the cancel button gone.
        assert_eq!(css_px(OVERLAY_CSS, "--ov-wave-pad-l"), 0.0);
        assert_eq!(tsx_const(OVERLAY_TSX, "const WAVE_BARS = "), CARD_WAVE_BARS);
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-elem-gap"),
            f64::from(ELEMENT_GAP_INHERIT)
        );
        // How many element gaps the row with a waveform counts; `row_gap_width`
        // mirrors it and the apply layer drops it to 1 with the cancel button.
        assert_eq!(css_number(OVERLAY_CSS, "--ov-row-gaps"), 2.0);
        assert_eq!(inherit_metrics().row_gap_width(), 0.0);
        assert_eq!(
            metrics_of(OverlayTheme {
                element_gap: Some(6),
                ..OverlayTheme::default()
            })
            .row_gap_width(),
            12.0
        );
        assert_eq!(
            metrics_of(OverlayTheme {
                element_gap: Some(6),
                show_cancel: Some(false),
                ..OverlayTheme::default()
            })
            .row_gap_width(),
            6.0
        );
        // The three derived widths, as text, since none of them is a number:
        // the bar count and the two side columns are the same sums Rust adds
        // up. Whitespace is collapsed, because Prettier owns the line breaks.
        for (name, expected) in [
            (
                "--ov-wave-slot-w",
                "calc(9 * var(--ov-wave-w) + 8 * var(--ov-wave-gap))",
            ),
            (
                "--ov-bare-w",
                "calc( 2 * var(--ov-pad) + 2 * var(--ov-elem-gap) +                  max(var(--ov-side-min), var(--ov-dot-w)) + var(--ov-side-min) )",
            ),
            (
                "--ov-row-w",
                "calc( 2 * var(--ov-pad) + var(--ov-row-gaps) * var(--ov-elem-gap) +                  max(var(--ov-side-min), var(--ov-dot-col-w)) + var(--ov-side-min) +                  var(--ov-wave-pad-l) + var(--ov-wave-slot-w) + var(--ov-wave-pad-r) )",
            ),
        ] {
            assert_eq!(
                collapsed(css_declaration(OVERLAY_CSS, name)),
                collapsed(expected),
                "{name}"
            );
        }
        // …and the widths the card is actually drawn at, each carrying the two
        // element gaps and, for the two resting shapes, the `max()` against the
        // row that stops the waveform being clipped.
        for (selector, expected) in [
            (
                "\n.scard {",
                "calc(max(var(--ov-pill-w) + 2 * var(--ov-elem-gap), var(--ov-row-w)) * var(--ov-scale))",
            ),
            (
                ".scard.compact {",
                "calc(max(var(--ov-rest-w) + 2 * var(--ov-elem-gap), var(--ov-row-w)) * var(--ov-scale))",
            ),
            (
                ".scard.nowave {",
                "calc(var(--ov-bare-w) * var(--ov-scale))",
            ),
            (
                ".scard.nocancel {",
                "calc(var(--ov-row-w) * var(--ov-scale))",
            ),
            (
                ".scard.nowave.nocancel {",
                "calc((var(--ov-row-core-h) + 2 * var(--ov-pad)) * var(--ov-scale))",
            ),
            (
                ".scard.open {",
                "calc((var(--ov-open-w) + 2 * var(--ov-elem-gap)) * var(--ov-scale))",
            ),
            (
                ".scard.working {",
                "calc((var(--ov-work-w) + 2 * var(--ov-elem-gap)) * var(--ov-scale))",
            ),
            (
                ".scard.compact.cworking {",
                "calc((var(--ov-work-w) + 2 * var(--ov-elem-gap)) * var(--ov-scale))",
            ),
        ] {
            assert_eq!(
                collapsed(css_declaration(css_rule(OVERLAY_CSS, selector), "width")),
                collapsed(expected),
                "{selector}"
            );
        }
        // …and the last of those is a square: with both elements hidden the
        // pill is written as exactly what `.sbase` is tall, which is the row
        // height `bare_row_width` hands back.
        assert_eq!(
            collapsed(css_declaration(
                css_rule(OVERLAY_CSS, ".scard.nowave.nocancel {"),
                "width"
            )),
            collapsed(css_declaration(css_rule(OVERLAY_CSS, ".sbase {"), "height"))
        );
        // The row's gap is the token itself, once per column boundary, which is
        // what the widths above pay for: twice while the row keeps its three
        // columns, once for the pill the cancel button left as two.
        assert_eq!(
            collapsed(css_declaration(
                css_rule(OVERLAY_CSS, ".sbase {"),
                "column-gap"
            )),
            "calc(var(--ov-elem-gap) * var(--ov-scale))"
        );

        // The shadow's four `:root` declarations. The strength and the offset
        // are the tokens' own inherits, the slack is written by the apply layer
        // and the blur is derived, living only here and in `CARD_SHADOW_BLUR`.
        assert_eq!(css_number(OVERLAY_CSS, "--ov-shadow-strength"), 0.0);
        assert_eq!(
            css_px(OVERLAY_CSS, "--ov-shadow-y"),
            f64::from(crate::overlay_theme::SHADOW_OFFSET_Y_INHERIT)
        );
        assert_eq!(css_px(OVERLAY_CSS, "--ov-shadow-blur"), CARD_SHADOW_BLUR);
        assert_eq!(css_px(OVERLAY_CSS, "--ov-shadow-slack"), 0.0);
        assert_eq!(css_px(OVERLAY_CSS, "--ov-shadow-edge-slack"), 0.0);
        // The card's own shadow: two layers, both scaled, both at an alpha the
        // strength multiplies, so strength 0 is fully transparent and Flat is
        // pixel-identical to what it was. Only the alphas follow the strength;
        // the blur is the constant above: half the window's shadow slack.
        assert_eq!(
            collapsed(css_declaration(css_rule(OVERLAY_CSS, "\n.scard {"), "box-shadow")),
            collapsed(
                "0 calc(var(--ov-shadow-y) * var(--ov-scale))                  calc(var(--ov-shadow-blur) * var(--ov-scale))                  rgb(0 0 0 / calc(var(--ov-shadow-strength) * 0.45)),                  0 calc(1px * var(--ov-scale)) calc(2px * var(--ov-scale))                  rgb(0 0 0 / calc(var(--ov-shadow-strength) * 0.25))"
            )
        );
        // Under Glass the shadow is macOS's own, outside the window, so the
        // card draws none.
        assert_eq!(
            css_declaration(
                css_rule(OVERLAY_CSS, ":root[data-material=\"glass\"] .scard {"),
                "box-shadow"
            ),
            "none"
        );
        // The apply layer's own copy of the blur, so the two languages compute
        // the same slack.
        assert_eq!(
            tsx_const(APPLY_LAYER_TS, "export const SHADOW_BLUR_PX = "),
            CARD_SHADOW_BLUR
        );
    }
}
