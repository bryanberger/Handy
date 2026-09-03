/**
 * The preview scales the card to fit its stage rather than tightening the
 * token contract's bounds: at `size_scale` 1.50 a Live card is 591×177 px,
 * comfortably past the stage's ~456×148 px box at the settings window's
 * minimum width. Both numbers here are computed, never hardcoded, from
 * `getComputedStyle` of the stage (`--ov-open-w`, `--ov-work-w`,
 * `--ov-base-h`, `--ov-cap-max-h`) so they can never drift from the CSS.
 */

import type { OverlayPreviewStyle } from "./useOverlayPreviewDriver";

export interface CardBaseMetrics {
  /** `--ov-open-w`: the Live panel's width at scale 1. */
  openW: number;
  /** `--ov-work-w`: the widest compact-form width at scale 1. */
  workW: number;
  /** `--ov-base-h`: the control row's height at scale 1. */
  baseH: number;
  /** `--ov-cap-max-h`: the live-text region's max height at scale 1. */
  capMaxH: number;
  /** `--ov-cap-pad-y`: the live-text region's top padding at scale 1. */
  capPadY: number;
}

/**
 * The card's footprint at `scale`, content plus its (also-scaled) 1 px
 * border on every side — the same "base × scale + border" rule the native
 * window uses (`overlay.rs`'s `overlay_dimensions`). Live uses the open
 * panel's footprint (the tallest/widest form of that style); Minimal uses the
 * compact form's widest pill, since those are the shapes that must fit.
 */
export function cardFootprint(
  style: OverlayPreviewStyle,
  scale: number,
  base: CardBaseMetrics,
): { width: number; height: number } {
  const border = 2 * scale;
  if (style === "live") {
    return {
      width: base.openW * scale + border,
      height: (base.baseH + base.capMaxH + base.capPadY) * scale + border,
    };
  }
  return {
    width: base.workW * scale + border,
    height: base.baseH * scale + border,
  };
}

/**
 * `min(1, stageWidth / cardWidth, stageHeight / cardHeight)` — never upscales
 * (a small window never zooms the card past its true size), and falls back to
 * `1` for any non-positive input rather than producing `Infinity` or `NaN`.
 */
export function computeFit(
  stageWidth: number,
  stageHeight: number,
  cardWidth: number,
  cardHeight: number,
): number {
  if (
    stageWidth <= 0 ||
    stageHeight <= 0 ||
    cardWidth <= 0 ||
    cardHeight <= 0
  ) {
    return 1;
  }
  return Math.min(1, stageWidth / cardWidth, stageHeight / cardHeight);
}
