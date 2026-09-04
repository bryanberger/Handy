import { WAVEFORM_RENDERERS } from "./renderers";
import type { CanvasWaveformStyle } from "./waveformStyles";
import {
  LEVEL_BUCKETS,
  type WaveformColors,
  type WaveformFlags,
  type WaveformGeometry,
} from "./waveformLane";

/**
 * One animation frame of the waveform canvas: size the backing store if it
 * moved, copy the levels in, and hand the style a cleared context.
 *
 * Apart from `WaveformCanvas`, which owns the DOM and the loop this runs in, so
 * the decision to draw a frame can be read, and tested, without a browser.
 */

/** The cap on the backing store's density. Past 2 the lane costs pixels
 *  nobody can see: a 60x18 lane at 2 is already under 5 000 of them. */
export const MAX_DPR = 2;

/** How far a bucket must move to be worth a frame. Only consulted under reduced
 *  motion, where a style has no idle motion and would repaint the same
 *  picture. */
export const LEVEL_EPSILON = 0.002;

/** What a frame writes its pixels into. The two fields of a canvas this
 *  module touches, so the rules above are testable without one. */
export interface WaveformBackingStore {
  width: number;
  height: number;
}

/** Everything a frame mutates in place. One object per mounted canvas, so a
 *  frame allocates nothing. */
export interface WaveformCanvasState {
  ctx: CanvasRenderingContext2D | null;
  geom: WaveformGeometry;
  colors: WaveformColors;
  flags: WaveformFlags;
  levels: Float32Array;
  /** The lane's CSS box, what the backing store is derived from. */
  cssWidth: number;
  cssHeight: number;
  /** The two waveform lengths in CSS pixels, after the size scale. */
  unitCss: number;
  gapCss: number;
  /** Whether anything the style draws from moved since the last frame. */
  moved: boolean;
  /** The clock the styles animate from: when this canvas mounted. */
  start: number;
  style: CanvasWaveformStyle;
}

/** A state with nothing measured yet: draws nothing until the lane's box and
 *  the theme's colours have been read. */
export function initialWaveformState(
  style: CanvasWaveformStyle,
): WaveformCanvasState {
  return {
    ctx: null,
    geom: { width: 0, height: 0, unit: 0, gap: 0, dpr: 1 },
    colors: { accent: "#000", muted: "#000" },
    flags: { ready: false, reduceMotion: false },
    levels: new Float32Array(LEVEL_BUCKETS),
    cssWidth: 0,
    cssHeight: 0,
    unitCss: 0,
    gapCss: 0,
    moved: true,
    start: 0,
    style,
  };
}

export function drawWaveformFrame(
  canvas: WaveformBackingStore,
  state: WaveformCanvasState,
  source: readonly number[],
  now: number,
  dpr: number,
): void {
  const { ctx } = state;
  if (!ctx) return;

  const width = Math.round(state.cssWidth * dpr);
  const height = Math.round(state.cssHeight * dpr);
  if (width <= 0 || height <= 0) return;
  // Assigning either field clears the canvas, so it is written only when the
  // lane's box or the display's density actually moved.
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
    state.moved = true;
  }
  // Synced whatever that branch did. A fresh canvas already measures its
  // intrinsic 300x150, so a lane wanting exactly that leaves the assignment
  // untaken and the geometry every style reads would stay at zero.
  state.geom.width = width;
  state.geom.height = height;
  if (state.geom.dpr !== dpr) {
    state.geom.dpr = dpr;
    state.moved = true;
  }
  state.geom.unit = state.unitCss * dpr;
  state.geom.gap = state.gapCss * dpr;

  const levels = state.levels;
  const count = Math.min(levels.length, source.length);
  for (let bucket = 0; bucket < count; bucket += 1) {
    const next = source[bucket] || 0;
    if (Math.abs(next - levels[bucket]) > LEVEL_EPSILON) state.moved = true;
    levels[bucket] = next;
  }

  // Under reduced motion nothing animates on its own, so a frame whose levels
  // did not move has nothing new to paint.
  if (state.flags.reduceMotion && !state.moved) return;
  state.moved = false;

  ctx.clearRect(0, 0, width, height);
  WAVEFORM_RENDERERS[state.style](
    ctx,
    levels,
    now - state.start,
    state.geom,
    state.colors,
    state.flags,
  );
  // The one restore. A style sets `globalAlpha` once per pass, and the next
  // frame starts with a clear rather than a fill, so putting it back here is
  // both cheaper and the only place it can be forgotten.
  ctx.globalAlpha = 1;
}
