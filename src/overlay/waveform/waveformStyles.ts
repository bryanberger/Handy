import type { WaveformStyle } from "@/bindings";
import { WAVEFORM_STYLES } from "@/lib/overlayTheme";

/**
 * Which of the two waveform lengths each style actually reads.
 *
 * Data only, no renderer imported, so the Appearance tab can ask which rows a
 * style has without pulling the canvas code into the settings bundle. The tab
 * hides the rows a style ignores; the lane is sized from both lengths whatever
 * the style, so a hidden row still shapes the slot and never the footprint.
 *
 * `the_waveform_styles_match_the_frontends` in `src-tauri/src/overlay_theme.rs`
 * pins this table against `WaveformStyle::ALL`, so a value added in Rust with
 * no entry here fails a test rather than rendering nothing.
 */
export interface WaveformStyleTokens {
  /** Reads `waveform_width`. */
  usesWidth: boolean;
  /** Reads `waveform_gap`. */
  usesGap: boolean;
}

export const WAVEFORM_STYLE_TOKENS = {
  // Nine capsules: the width is the bar and the gap is between bars.
  bars: { usesWidth: true, usesGap: true },
  // The width is the ribbon's thickness at silence; it has nothing to gap.
  ribbon: { usesWidth: true, usesGap: false },
  // Sized by the lane, so neither length reaches it.
  bloom: { usesWidth: false, usesGap: false },
  // The width is a mote's diameter; the field is scattered, not spaced.
  motes: { usesWidth: true, usesGap: false },
  // The width caps a dot and the gap spaces the panel on both axes.
  matrix: { usesWidth: true, usesGap: true },
  // The width is a step; contiguous by design, so no gap.
  steps: { usesWidth: true, usesGap: false },
} satisfies Record<WaveformStyle, WaveformStyleTokens>;

/** The styles that read `waveform_width`, and those that read `waveform_gap`.
 *  Derived, so the descriptor table in the Appearance tab and the renderers
 *  cannot disagree about which rows a style has. */
export const STYLES_USING_WAVEFORM_WIDTH: readonly WaveformStyle[] =
  WAVEFORM_STYLES.filter((style) => WAVEFORM_STYLE_TOKENS[style].usesWidth);
export const STYLES_USING_WAVEFORM_GAP: readonly WaveformStyle[] =
  WAVEFORM_STYLES.filter((style) => WAVEFORM_STYLE_TOKENS[style].usesGap);

/** Every style but today's bars: the five drawn on a canvas. */
export type CanvasWaveformStyle = Exclude<WaveformStyle, "bars">;

/** Whether this style is drawn on a canvas rather than as the DOM capsules.
 *  The bars stay the DOM path, so an unset token costs nothing new. */
export function isCanvasWaveformStyle(
  style: WaveformStyle,
): style is CanvasWaveformStyle {
  return style !== "bars";
}

/**
 * Which style is actually drawn, given whether the canvas could be had.
 *
 * A browser that returns no 2D context drops every style back to the bars.
 * They are DOM elements fed from React state, and that state is skipped while
 * a canvas is drawing, so the overlay has to be told rather than the card
 * deciding on its own: otherwise the fallback bars sit at zero for the rest of
 * the session.
 */
export function drawnWaveformStyle(
  style: WaveformStyle,
  canvasUnavailable: boolean,
): WaveformStyle {
  return canvasUnavailable ? "bars" : style;
}
