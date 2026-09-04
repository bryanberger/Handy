/**
 * What a waveform style is handed, and the arithmetic more than one of them
 * needs.
 *
 * The lane is the fixed slot the waveform draws into, `.swave` in
 * `RecordingOverlay.css`. Its width is a function of the two waveform lengths
 * and never of the style, so switching styles cannot move the card or the
 * native window.
 *
 * Hard constraint, inherited from the apply layer: this module and every style
 * beside it load in the overlay webview (#1279). No React, no i18next, no
 * store. Pure functions and constants, and nothing allocated per frame.
 */

/** The lane a style draws into, in device pixels (the canvas backing store). */
export interface WaveformGeometry {
  /** The backing store's width. */
  width: number;
  /** The backing store's height. */
  height: number;
  /** `waveform_width` after the size scale, the length a style reads for its
   *  own shape: a bar, a mote's diameter, a matrix dot, a step. */
  unit: number;
  /** `waveform_gap` after the size scale. */
  gap: number;
  /** Device pixels per CSS pixel, capped, so a style can snap to whole
   *  pixels where crispness is the point. */
  dpr: number;
}

/**
 * The two colours a style paints with, already resolved to something
 * `fillStyle` accepts.
 *
 * Resolved off a probe once per theme change, never per frame: the overlay's
 * `--s-*` properties are `color-mix()` values, which `fillStyle` rejects.
 */
export interface WaveformColors {
  /** `--s-accent`, what the waveform is once samples are flowing. */
  accent: string;
  /** `--s-muted`, what every style's arming idle is drawn in. */
  muted: string;
}

/** The two facts that change what a style draws rather than how much. */
export interface WaveformFlags {
  /** False until the first microphone sample: the style draws its own idle
   *  at the muted colour instead of the levels. */
  ready: boolean;
  /** `prefers-reduced-motion: reduce`. No idle animation, no drift, no
   *  travel. The shape still follows the levels, because that is data. */
  reduceMotion: boolean;
}

/**
 * One waveform style's whole job: paint `levels` into the lane.
 *
 * Called on a cleared context, once per animation frame. `levels` is the
 * shared, preallocated read of the 16 smoothed microphone buckets, so a style
 * must not keep it. `elapsedMs` is the time since the canvas mounted and must
 * be ignored entirely while `flags.reduceMotion`, which is what makes two
 * timestamps at the same levels draw the same frame.
 */
export type WaveformDraw = (
  ctx: CanvasRenderingContext2D,
  levels: Float32Array,
  elapsedMs: number,
  geom: WaveformGeometry,
  colors: WaveformColors,
  flags: WaveformFlags,
) => void;

/** How many smoothed microphone buckets a frame carries. The backend emits 16
 *  FFT buckets and the overlay smooths all of them; how many of those a style
 *  reads is its own business. Declared once, so the ref, the frame's copy and
 *  the styles cannot disagree about the length. */
export const LEVEL_BUCKETS = 16;

/**
 * How faint an arming idle drawn as one flat shape is, the muted waveform's
 * own 0.35.
 *
 * Not every style's: `matrix` lights one dot over its own unlit panel and sets
 * its contrast against that panel rather than against the lane, so it carries
 * its own alpha and says so.
 */
export const ARMING_ALPHA = 0.35;

/**
 * One level, sampled between buckets and wrapped, so a style can walk the
 * lane at whatever resolution suits it and can drift along the buckets
 * without falling off either end.
 */
export function sampleLevel(levels: Float32Array, position: number): number {
  const count = levels.length;
  const wrapped = ((position % count) + count) % count;
  const low = Math.floor(wrapped);
  const high = (low + 1) % count;
  const blend = wrapped - low;
  return levels[low] * (1 - blend) + levels[high] * blend;
}

/** The mean of the buckets: how loud the whole waveform is right now. */
export function loudness(levels: Float32Array): number {
  let total = 0;
  for (let bucket = 0; bucket < levels.length; bucket += 1)
    total += levels[bucket];
  return total / levels.length;
}

/**
 * The seconds a style animates by: the clock, or a frozen 0 under reduced
 * motion. One place, so no style forgets the rule.
 */
export function animationSeconds(
  elapsedMs: number,
  flags: WaveformFlags,
): number {
  return flags.reduceMotion ? 0 : elapsedMs / 1000;
}

/** The colour a style paints with: the accent once samples are flowing, the
 *  muted neutral while arming. Split from the alpha below, because a style
 *  that layers passes picks its own alphas and still wants this one colour. */
export function paintColor(
  colors: WaveformColors,
  flags: WaveformFlags,
): string {
  return flags.ready ? colors.accent : colors.muted;
}

/** How faint that colour is: full while capturing, [`ARMING_ALPHA`] before the
 *  first sample. */
export function paintAlpha(flags: WaveformFlags): number {
  return flags.ready ? 1 : ARMING_ALPHA;
}

/**
 * Both at once, for a style that paints one flat shape.
 *
 * The alpha is not put back afterwards: `drawWaveformFrame` resets it once
 * after the renderer returns, so no style carries a restore of its own.
 */
export function paintFor(
  ctx: CanvasRenderingContext2D,
  colors: WaveformColors,
  flags: WaveformFlags,
): void {
  ctx.fillStyle = paintColor(colors, flags);
  ctx.globalAlpha = paintAlpha(flags);
}

/** The longest step a style integrates in one go. A canvas that was starved
 *  for a second must not teleport a particle field across the lane. */
const MAX_STEP_SECONDS = 0.1;

/**
 * One style's own clock, for the styles that integrate rather than compute:
 * an eased radius, a falling peak, a rising particle.
 *
 * Allocated once per module and mutated in place, so a frame allocates
 * nothing.
 */
export interface FrameClock {
  /** The previous frame's `elapsedMs`, or -1 before the first frame. */
  previous: number;
}

export function newFrameClock(): FrameClock {
  return { previous: -1 };
}

/**
 * Seconds since the last frame: 0 on the first frame, after the canvas
 * remounts and its clock restarts, and always under reduced motion.
 *
 * That zero is what freezes every integration into a pure function of the
 * levels, which is both the contract's rule and what makes two timestamps at
 * the same levels draw the same picture.
 */
export function frameSeconds(
  clock: FrameClock,
  elapsedMs: number,
  flags: WaveformFlags,
): number {
  const previous = clock.previous;
  clock.previous = elapsedMs;
  if (flags.reduceMotion || previous < 0 || elapsedMs < previous) return 0;
  return Math.min((elapsedMs - previous) / 1000, MAX_STEP_SECONDS);
}

/**
 * A style's cached soft-edged brush.
 *
 * A canvas gradient belongs to the context that made it, and building one is
 * an allocation, so each style that paints with one keeps a slot and rebuilds
 * only when the context or the colour changes: on a theme repaint, or when
 * arming hands over to the accent.
 */
export interface SoftGradient {
  ctx: CanvasRenderingContext2D | null;
  color: string;
  core: number;
  gradient: CanvasGradient | null;
}

export function newSoftGradient(): SoftGradient {
  return { ctx: null, color: "", core: -1, gradient: null };
}

/**
 * A radial gradient in unit space: solid `color` out to `core`, fading to
 * nothing at radius 1.
 *
 * Unit space because the shape it fills changes size every frame. Paint with
 * it by setting the transform to the shape's own extent
 * (`setTransform(rx, 0, 0, ry, centreX, centreY)`) just before the fill: a
 * path already built is in canvas space and does not move, while the gradient
 * is resolved through the transform in force when it is painted.
 *
 * The outer stop is `transparent` rather than a faded colour because a canvas
 * interpolates its stops with the alpha premultiplied, so the fade keeps the
 * hue instead of running through black, and no colour string has to be picked
 * apart to give it an alpha.
 */
export function softGradient(
  cache: SoftGradient,
  ctx: CanvasRenderingContext2D,
  color: string,
  core: number,
): CanvasGradient {
  if (
    cache.gradient &&
    cache.ctx === ctx &&
    cache.color === color &&
    cache.core === core
  )
    return cache.gradient;
  const gradient = ctx.createRadialGradient(0, 0, 0, 0, 0, 1);
  gradient.addColorStop(0, color);
  gradient.addColorStop(core, color);
  gradient.addColorStop(1, "transparent");
  cache.ctx = ctx;
  cache.color = color;
  cache.core = core;
  cache.gradient = gradient;
  return gradient;
}
