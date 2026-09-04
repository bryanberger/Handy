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
 * Bloom: one closed blob whose surface deforms per bucket and breathes with
 * the overall level. A living thing rather than a graph, so it reads even at
 * the lane's eighteen points of height.
 *
 * The outline is a smooth closed curve through one control point per bucket,
 * each quadratic meeting the next at their shared midpoint so the surface has
 * no corners at any level. It is painted in three passes and no more: a thin
 * outer glow, the silhouette itself at nearly full accent, and one smaller
 * copy of the same outline lifted towards the light. A crisp edge is what
 * makes the deformation legible; a stack of nested fills, which was the first
 * attempt, accumulates into a smear that is brightest wherever the outline
 * bulges and faint in its recesses, so the blob reads as lopsided even when
 * the buckets are even.
 *
 * The silhouette is re-centred on its own middle every frame, so a loud bucket
 * grows the surface where it points instead of dragging the whole blob off to
 * one side of the lane.
 *
 * The only style that reads neither waveform length: it is sized by the lane.
 */

/** Control points around the outline, one per microphone bucket. */
const POINTS = 16;
const TAU = Math.PI * 2;

/** How much of the lane's half-width and half-height the outline may reach.
 *  The glow goes past it, into the room this leaves. */
const BODY_REACH = 0.86;

/** The three passes: how far outside the silhouette the glow reaches, in
 *  device pixels, and what each pass carries of the accent.
 *
 *  The body is a hair under solid so the highlight has somewhere to go: the
 *  accent at full is the brightest thing available, so an inner highlight can
 *  only exist if the surface around it is not already there. */
const GLOW_PIXELS = 1.5;
const GLOW_ALPHA = 0.22;
const BODY_ALPHA = 0.85;
const HIGHLIGHT_ALPHA = 1;

/** The inner highlight: a copy of the same outline this much of the way in,
 *  lifted by this share of the blob's own height. The lift is what keeps it
 *  from reading as a pupil in an eye, which a concentric copy does. */
const HIGHLIGHT_SCALE = 0.55;
const HIGHLIGHT_LIFT = 0.26;

/** How wide the blob may be for its height. The lane is nearly three times as
 *  wide as it is tall, and a blob that filled it would be a lens; this keeps
 *  the shape a body with room around it at every level. */
const ASPECT = 2.55;

/** The share of its reach the blob takes at silence, how much of the rest the
 *  overall level swells it by, and how hard that level is driven. The gain is
 *  what the mean of sixteen buckets needs: speech peaks in a bucket or two and
 *  averages far below the level it sounds like. */
const RESTING = 0.32;
const BREATH = 0.68;
const LEVEL_GAIN = 2.6;

/** How far a bucket pushes its own control point, either way, as a share of
 *  the blob's radius.
 *
 *  Measured against the quietest and loudest bucket of the frame rather than
 *  against an absolute level: sixteen smoothed buckets span a tenth of the
 *  range at a conversational level, and a fixed gain over that difference left
 *  the surface an ellipse whatever was said. */
const PUSH_SWING = 0.32;

/** The spread of buckets that earns the whole swing, and the smallest spread
 *  the normalisation will divide by. Under the first the surface hands over to
 *  its own slow undulation, so a quiet lane is a calm living blob rather than
 *  a shape amplifying the last of the noise. */
const SPREAD_FULL = 0.1;
const SPREAD_FLOOR = 1e-4;

/** How much of the swing that undulation is worth. Under the speech it stands
 *  in for, because a quiet blob should breathe, not twitch. */
const QUIET_LOBE = 0.55;

/** How long a control point takes to reach its bucket. Without it the surface
 *  jitters with every frame; much more than this and a bucket's own movement
 *  is averaged away and the blob is a smooth ellipse again. */
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

/** The unit circle the control points sit on, and everything the blob keeps
 *  between frames: each point's eased push, that push rounded off against its
 *  neighbours, its offset from the centre, and the clock the easing is
 *  integrated over. Allocated once, so a frame allocates nothing. */
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
 * The outline, as one closed path of quadratics, around a centre and scaled
 * per axis.
 *
 * Each control point is a curve's control, and the on-curve points are the
 * midpoints between neighbours, which is what makes consecutive quadratics
 * share a tangent: a closed curve with no corners, from as few points as the
 * buckets themselves.
 *
 * The two scales are separate so the glow can stand off the silhouette by the
 * same number of pixels all the way round a shape that is wider than it is
 * tall.
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
  // A frozen clock snaps the surface to the levels, which is what makes a
  // reduced-motion frame a pure function of them.
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

  // Round the surface off against itself: a 1-4-1 pass around the ring, which
  // leaves a swell spanning several buckets almost untouched and takes a
  // third of a bucket-to-bucket spike. Without it a single loud bucket pulls
  // a point out of the outline and the blob reads as a comma; with the 1-2-1
  // that was tried first, nothing but the broadest lobe survived and the blob
  // was an ellipse again.
  for (let point = 0; point < POINTS; point += 1) {
    const before = push[(point + POINTS - 1) % POINTS];
    const after = push[(point + 1) % POINTS];
    rounded[point] = (before + 4 * push[point] + after) / 6;
    offsetX[point] = rounded[point] * cosine[point];
    offsetY[point] = rounded[point] * sine[point];
  }

  // Re-centre on the outline's own middle, so the deformation grows the
  // surface rather than sliding the blob across the lane, and then measure
  // what the shape now reaches: a push past the unit circle only shrinks the
  // blob to fit, which is what keeps every level inside the lane.
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

  // Normalising by the furthest point, rather than clamping it, puts the
  // extreme of the surface exactly on the reach at every level: the blob
  // touches the same bound whether it is a smooth ellipse or a spiky one.
  const fit = 1 / Math.max(reachX, reachY);
  const radiusY = centreY * BODY_REACH * swell * fit;
  const radiusX = Math.min(centreX * BODY_REACH * fit, radiusY * ASPECT);
  for (let point = 0; point < POINTS; point += 1) {
    offsetX[point] *= radiusX;
    offsetY[point] *= radiusY;
  }

  // What the silhouette actually reaches, and so how much room is left for
  // the glow to stand off it in: a fixed number of pixels all the way round,
  // and never past the lane.
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
