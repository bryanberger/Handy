import {
  animationSeconds,
  frameSeconds,
  loudness,
  newFrameClock,
  paintAlpha,
  paintColor,
  sampleLevel,
  type WaveformDraw,
} from "./waveformLane";

/**
 * Bloom: one closed blob whose surface deforms per bucket and breathes with the
 * overall level. A living thing rather than a graph, so it reads at the lane's
 * eighteen points of height.
 *
 * The outline is a smooth closed curve through one control point per bucket,
 * quadratics meeting at their shared midpoints, so no corners at any level.
 * Three passes and no more: a thin outer glow, the silhouette at nearly full
 * accent, and a smaller copy of the outline lifted to the light. A crisp edge
 * makes deformation legible; nested fills, the first attempt, smeared bright at
 * bulges and faint in recesses, so it read lopsided on even buckets.
 *
 * The silhouette re-centres on its middle each frame, so a loud bucket grows
 * the surface where it points instead of dragging the blob across the lane.
 *
 * The only style that reads neither waveform length: it is sized by the lane.
 */

/** Control points around the outline, one per microphone bucket. */
const POINTS = 16;
const TAU = Math.PI * 2;

/** How much of the lane's half-width and half-height the outline may reach.
 *  The glow goes past it, into the room this leaves. */
const BODY_REACH = 0.86;

/** The three passes: how far outside the silhouette the glow reaches, in device
 *  pixels, and what each carries of the accent.
 *
 *  The body is a hair under solid so the highlight shows: the accent at full is
 *  the brightest available, so nothing reads over a body already there. */
const GLOW_PIXELS = 1.5;
const GLOW_ALPHA = 0.22;
const BODY_ALPHA = 0.85;
const HIGHLIGHT_ALPHA = 1;

/** The inner highlight: a copy of the outline this far in, lifted by this share
 *  of the blob's height. The lift keeps it from reading as a pupil in an eye,
 *  which a concentric copy does. */
const HIGHLIGHT_SCALE = 0.55;
const HIGHLIGHT_LIFT = 0.26;

/** How wide the blob may be for its height. The lane is nearly three times as
 *  wide as tall, and a blob filling it would be a lens; this keeps a body with
 *  room around it at every level. */
const ASPECT = 2.55;

/** The blob's share of its reach at silence, how much of the rest the overall
 *  level swells it by, and the gain on it: the mean of sixteen buckets sits far
 *  below how loud speech sounds, which peaks in a bucket or two. */
const RESTING = 0.32;
const BREATH = 0.68;
const LEVEL_GAIN = 2.6;

/** How far a bucket pushes its control point either way, as a share of radius.
 *
 *  Against the frame's quietest and loudest bucket, not an absolute level:
 *  sixteen smoothed buckets span a tenth of the range in conversation, and a
 *  fixed gain over that left the surface an ellipse whatever was said. */
const PUSH_SWING = 0.32;

/** The spread of buckets that earns the whole swing, and the smallest spread
 *  the normalisation divides by. Under the first the surface hands over to its
 *  slow undulation, so a quiet lane is a calm blob rather than one amplifying
 *  the last of the noise. */
const SPREAD_FULL = 0.1;
const SPREAD_FLOOR = 1e-4;

/** How much of the swing that undulation is worth. Under the speech it stands
 *  in for, because a quiet blob should breathe, not twitch. */
const QUIET_LOBE = 0.55;

/** How long a control point takes to reach its bucket. Without it the surface
 *  jitters every frame; much more and a bucket's own movement is averaged away
 *  into a smooth ellipse again. */
const EASE_SECONDS = 0.055;

/** How fast the deformation rolls around the outline, in buckets a second. */
const ROLL_BUCKETS_PER_SECOND = 0.9;

/** The arming idle: the level the blob breathes around, how far it swings,
 *  how long one breath takes, and how many lobes travel round the surface.
 *  The lobes are the quiet undulation too, so there is one shape of calm. */
const IDLE_LEVEL = 0.16;
const IDLE_SWING = 0.08;
const IDLE_PERIOD_SECONDS = 3;
const IDLE_LOBES = 2;
const IDLE_ROLL_RADIANS_PER_SECOND = 1.2;

/** The unit circle the control points sit on and what the blob keeps between
 *  frames: each point's eased push, that push rounded against its neighbours,
 *  its offset from the centre, and the clock the easing integrates over.
 *  Allocated once, so a frame allocates nothing. */
const cosine = new Float32Array(POINTS);
const sine = new Float32Array(POINTS);
for (let point = 0; point < POINTS; point += 1) {
  cosine[point] = Math.cos((point / POINTS) * TAU);
  sine[point] = Math.sin((point / POINTS) * TAU);
}
const push = new Float32Array(POINTS);
const rounded = new Float32Array(POINTS);
const offsetX = new Float32Array(POINTS);
const offsetY = new Float32Array(POINTS);
const clock = newFrameClock();

/**
 * The outline, one closed path of quadratics around a centre, scaled per axis.
 *
 * Each control point is a curve's control and the on-curve points are the
 * midpoints between neighbours, so consecutive quadratics share a tangent: a
 * closed curve with no corners, from as few points as the buckets.
 *
 * The two scales are separate so the glow can stand off the silhouette by the
 * same pixels all the way round a shape wider than it is tall.
 */
function trace(
  ctx: CanvasRenderingContext2D,
  centreX: number,
  centreY: number,
  scaleX: number,
  scaleY: number,
): void {
  const last = POINTS - 1;
  ctx.beginPath();
  ctx.moveTo(
    centreX + ((offsetX[last] + offsetX[0]) / 2) * scaleX,
    centreY + ((offsetY[last] + offsetY[0]) / 2) * scaleY,
  );
  for (let point = 0; point < POINTS; point += 1) {
    const next = (point + 1) % POINTS;
    ctx.quadraticCurveTo(
      centreX + offsetX[point] * scaleX,
      centreY + offsetY[point] * scaleY,
      centreX + ((offsetX[point] + offsetX[next]) / 2) * scaleX,
      centreY + ((offsetY[point] + offsetY[next]) / 2) * scaleY,
    );
  }
  ctx.closePath();
}

export const drawBloom: WaveformDraw = (
  ctx,
  levels,
  elapsedMs,
  geom,
  colors,
  flags,
) => {
  const { width, height } = geom;
  const centreX = width / 2;
  const centreY = height / 2;
  const seconds = animationSeconds(elapsedMs, flags);
  const step = frameSeconds(clock, elapsedMs, flags);
  // A frozen clock snaps the surface to the levels, so a reduced-motion frame
  // is a pure function of them.
  const ease = step > 0 ? 1 - Math.exp(-step / EASE_SECONDS) : 1;

  const breath = 0.5 + 0.5 * Math.sin((seconds / IDLE_PERIOD_SECONDS) * TAU);
  const level = flags.ready
    ? loudness(levels)
    : IDLE_LEVEL + IDLE_SWING * breath;
  const swell = RESTING + BREATH * Math.min(1, level * LEVEL_GAIN);
  const roll = seconds * ROLL_BUCKETS_PER_SECOND;

  let lowest = 1;
  let highest = 0;
  for (let bucket = 0; bucket < levels.length; bucket += 1) {
    if (levels[bucket] < lowest) lowest = levels[bucket];
    if (levels[bucket] > highest) highest = levels[bucket];
  }
  const spread = highest - lowest;
  const spoken = flags.ready ? Math.min(1, spread / SPREAD_FULL) : 0;

  for (let point = 0; point < POINTS; point += 1) {
    // Two lobes travelling round the surface: the whole shape while arming,
    // and what a lane too flat to say anything falls back to.
    const lobes =
      0.5 *
      Math.sin(
        (point / POINTS) * TAU * IDLE_LOBES +
          seconds * IDLE_ROLL_RADIANS_PER_SECOND,
      );
    // The point's own place around the blob picks the bucket that pushes it,
    // so a loud bucket raises one part of the surface rather than all of it.
    const bucket = (point / POINTS) * levels.length;
    const spectral = flags.ready
      ? (sampleLevel(levels, bucket + roll) - lowest) /
          Math.max(SPREAD_FLOOR, spread) -
        0.5
      : 0;
    const shape = flags.ready
      ? spectral * spoken + lobes * (1 - spoken) * QUIET_LOBE
      : lobes;
    push[point] += (1 + 2 * PUSH_SWING * shape - push[point]) * ease;
  }

  // Round the surface off: a 1-4-1 pass around the ring leaves a swell over
  // several buckets almost untouched and takes a third of a bucket-to-bucket
  // spike. Without it one loud bucket pulls a point out and the blob reads as a
  // comma; the 1-2-1 tried first left only the broadest lobe, an ellipse again.
  for (let point = 0; point < POINTS; point += 1) {
    const before = push[(point + POINTS - 1) % POINTS];
    const after = push[(point + 1) % POINTS];
    rounded[point] = (before + 4 * push[point] + after) / 6;
    offsetX[point] = rounded[point] * cosine[point];
    offsetY[point] = rounded[point] * sine[point];
  }

  // Re-centre on the outline's own middle, so the deformation grows the surface
  // rather than sliding the blob across the lane, then measure what the shape
  // reaches: a push past the unit circle only shrinks the blob to fit, keeping
  // every level inside the lane.
  let middleX = 0;
  let middleY = 0;
  for (let point = 0; point < POINTS; point += 1) {
    middleX += offsetX[point];
    middleY += offsetY[point];
  }
  middleX /= POINTS;
  middleY /= POINTS;
  let reachX = 1;
  let reachY = 1;
  for (let point = 0; point < POINTS; point += 1) {
    offsetX[point] -= middleX;
    offsetY[point] -= middleY;
    reachX = Math.max(reachX, Math.abs(offsetX[point]));
    reachY = Math.max(reachY, Math.abs(offsetY[point]));
  }

  // Normalising by the furthest point, not clamping it, puts the surface's
  // extreme exactly on the reach at every level, ellipse or spike alike.
  const fit = 1 / Math.max(reachX, reachY);
  const radiusY = centreY * BODY_REACH * swell * fit;
  const radiusX = Math.min(centreX * BODY_REACH * fit, radiusY * ASPECT);
  for (let point = 0; point < POINTS; point += 1) {
    offsetX[point] *= radiusX;
    offsetY[point] *= radiusY;
  }

  // What the silhouette reaches, and so the room left for the glow to stand off
  // in: a fixed number of pixels all the way round, never past the lane.
  const spanX = reachX * radiusX;
  const spanY = reachY * radiusY;
  const glow = Math.max(
    0,
    Math.min(GLOW_PIXELS, centreY - spanY, centreX - spanX),
  );
  const base = paintAlpha(flags);
  ctx.fillStyle = paintColor(colors, flags);

  ctx.globalAlpha = base * GLOW_ALPHA;
  trace(
    ctx,
    centreX,
    centreY,
    1 + glow / Math.max(1e-3, spanX),
    1 + glow / Math.max(1e-3, spanY),
  );
  ctx.fill();

  ctx.globalAlpha = base * BODY_ALPHA;
  trace(ctx, centreX, centreY, 1, 1);
  ctx.fill();

  ctx.globalAlpha = base * HIGHLIGHT_ALPHA;
  trace(
    ctx,
    centreX,
    centreY - radiusY * HIGHLIGHT_LIFT,
    HIGHLIGHT_SCALE,
    HIGHLIGHT_SCALE,
  );
  ctx.fill();
};
