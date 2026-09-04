import { animationSeconds, paintFor, type WaveformDraw } from "./waveformLane";

/**
 * Steps: a contiguous stepped histogram, square corners and no gaps, heights
 * quantised to fixed levels. The bars' data read as a blocky silhouette.
 * `waveform_gap` means nothing here, contiguous being the point.
 *
 * A step is twice `waveform_width`, so the silhouette is chunkier than the bars
 * rather than the bars with the gaps taken out.
 */

/** A step's width as a multiple of `waveform_width`. */
const STEP_WIDTH_FACTOR = 2;

/** How many heights a step may take, counted from the centre outward. Five is
 *  coarse enough to read as quantised at eighteen points of lane, and one more
 *  than four, which jumped too far between neighbours. */
const LEVELS = 5;

/** The shortest step, in levels, so a quiet lane is a low plateau rather than
 *  a hairline between two blocks. */
const FLOOR = 1;

/** The arming idle: the plateau every step rests at, the step the travelling
 *  one rises to, and how fast it travels, in steps per second. */
const IDLE_PLATEAU = 1;
const IDLE_PEAK = 2;
const IDLE_STEPS_PER_SECOND = 3;

/**
 * The mean level over a step's whole span of buckets, weighted by overlap.
 *
 * A step is a block, not a probe. Sampling the span's centre threw away every
 * bucket between two centres, half the spectrum at eight steps over sixteen
 * buckets, and read as two heavy ends with nothing between. A span narrower
 * than a bucket takes that bucket's level, as a histogram of a block should.
 */
function spanLevel(levels: Float32Array, from: number, to: number): number {
  let total = 0;
  let covered = 0;
  const last = Math.min(levels.length, Math.ceil(to));
  for (let bucket = Math.max(0, Math.floor(from)); bucket < last; bucket += 1) {
    const overlap = Math.min(to, bucket + 1) - Math.max(from, bucket);
    if (overlap <= 0) continue;
    total += levels[bucket] * overlap;
    covered += overlap;
  }
  return covered > 0 ? total / covered : 0;
}

export const drawSteps: WaveformDraw = (
  ctx,
  levels,
  elapsedMs,
  geom,
  colors,
  flags,
) => {
  const { width, height, unit } = geom;
  const stepWidth = Math.max(2, Math.round(unit * STEP_WIDTH_FACTOR));
  const count = Math.max(3, Math.round(width / stepWidth));
  const middle = height / 2;
  const perLevel = height / 2 / LEVELS;
  const seconds = animationSeconds(elapsedMs, flags);
  const travelling = Math.floor(seconds * IDLE_STEPS_PER_SECOND) % count;

  ctx.beginPath();
  for (let step = 0; step < count; step += 1) {
    let quantised: number;
    if (flags.ready) {
      const level = spanLevel(
        levels,
        (step / count) * levels.length,
        ((step + 1) / count) * levels.length,
      );
      quantised = Math.max(FLOOR, Math.round(Math.min(1, level) * LEVELS));
    } else {
      quantised = step === travelling ? IDLE_PEAK : IDLE_PLATEAU;
    }
    // Whole device pixels on both edges, so contiguous steps share one edge
    // and no seam shows between them.
    const left = Math.round((step * width) / count);
    const right = Math.round(((step + 1) * width) / count);
    const halfHeight = Math.round(quantised * perLevel);
    ctx.rect(left, middle - halfHeight, right - left, halfHeight * 2);
  }

  paintFor(ctx, colors, flags);
  ctx.fill();
};
