import {
  animationSeconds,
  paintFor,
  sampleLevel,
  type WaveformDraw,
} from "./waveformLane";

/**
 * Ribbon: one continuous filled band mirrored about the lane's centre line.
 *
 * Its thickness follows the levels and a slow drift carries it sideways, so
 * quiet speech still flows rather than sitting still. `waveform_width` is the
 * thickness at silence, which is what the band collapses to.
 */

/** Points along the lane. More than the sixteen buckets, so the outline reads
 *  as a curve rather than as a bar chart with the gaps taken out. */
const SAMPLES = 20;

/** How many buckets the ribbon travels per second. Slow enough to read as
 *  flow, not as scrolling. */
const DRIFT_BUCKETS_PER_SECOND = 1.6;

/** The level curve. Below 1 the quiet end of the range gets more of the
 *  thickness, which is where speech spends most of its time. */
const LEVEL_CURVE = 0.7;

/** The idle band's thickness as a fraction of the lane, and how far the
 *  undulation moves it. Arming has no levels to follow, so it draws its own
 *  gentle wave at the minimum thickness. */
const IDLE_LEVEL = 0.16;
const IDLE_SWING = 0.1;

/** The frame's own numbers, allocated once and refilled per frame, so the
 *  thickness below is one argument rather than six and a frame still allocates
 *  nothing. */
const band = {
  levels: new Float32Array(0),
  seconds: 0,
  drift: 0,
  thinnest: 0,
  range: 0,
  ready: false,
};

/**
 * Half the band's thickness at one sample, in device pixels.
 *
 * Written as a function of the sample index so both edges walk the same curve,
 * the top left to right and the bottom back again. At module scope rather than
 * inside the draw, so no closure is built thirty times a second.
 */
function halfThicknessAt(sample: number): number {
  const across = sample / SAMPLES;
  const level = band.ready
    ? sampleLevel(band.levels, across * band.levels.length + band.drift)
    : IDLE_LEVEL +
      IDLE_SWING * Math.sin(across * Math.PI * 2 + band.seconds * 1.4);
  const curved = Math.pow(Math.max(0, Math.min(1, level)), LEVEL_CURVE);
  return (band.thinnest + band.range * curved) / 2;
}

export const drawRibbon: WaveformDraw = (
  ctx,
  levels,
  elapsedMs,
  geom,
  colors,
  flags,
) => {
  const { width, height, unit } = geom;
  const middle = height / 2;
  const seconds = animationSeconds(elapsedMs, flags);
  const thinnest = Math.min(unit, height);

  band.levels = levels;
  band.seconds = seconds;
  band.drift = seconds * DRIFT_BUCKETS_PER_SECOND;
  band.thinnest = thinnest;
  band.range = height - thinnest;
  band.ready = flags.ready;

  ctx.beginPath();
  for (let sample = 0; sample <= SAMPLES; sample += 1) {
    const x = (sample / SAMPLES) * width;
    const y = middle - halfThicknessAt(sample);
    if (sample === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  for (let sample = SAMPLES; sample >= 0; sample -= 1) {
    ctx.lineTo((sample / SAMPLES) * width, middle + halfThicknessAt(sample));
  }
  ctx.closePath();

  paintFor(ctx, colors, flags);
  ctx.fill();
};
