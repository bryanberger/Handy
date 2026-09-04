import {
  animationSeconds,
  frameSeconds,
  newFrameClock,
  paintColor,
  sampleLevel,
  type WaveformDraw,
} from "./waveformLane";

/**
 * Matrix: a dot-matrix VU. Each column is a stack of square dots lit from the
 * centre row outward in quantised steps, under a peak dot that hangs above it
 * and falls back in. Only lit dots are drawn: the unlit panel an LED meter
 * shows was tried and taken out, to keep the card clean.
 *
 * The grid comes from the lane, not the tokens: the row pitch is the lane over
 * an odd row count, so one row is the centre and the panel symmetric about it.
 * `waveform_width` caps a dot and `waveform_gap` closes the space between them,
 * both to whole device pixels: a half-pixel dot is a smear, not an LED.
 *
 * Two fills a frame whatever the grid measures: the glow, then the lit dots.
 */

/** Rows in the panel: seven when the lane is tall enough for a dot to read as
 *  one, five otherwise. Odd either way, so one row is the centre and the lit
 *  steps are symmetric about it. */
const ROWS_TALL = 7;
const ROWS_SHORT = 5;

/** The shortest row pitch a seven-row panel is allowed, in device pixels. Below
 *  it the dots read as a texture rather than lit lamps, so the lane takes five
 *  taller rows instead. */
const TALL_MIN_PITCH = 9;

/** The smallest dot that still reads as one. */
const MIN_DOT = 2;

/** How much of a row's pitch the gap may take. The lane is short, so the gap
 *  token is capped rather than obeyed to the point of erasing the dot. */
const GAP_SHARE = 0.32;

/** The widest panel worth drawing, and the length of the two per-column
 *  stores. */
const MAX_COLUMNS = 96;

/** The glow under a lit dot: one extra pass, this many device pixels wider on
 *  every side, at this alpha. */
const GLOW_SPREAD = 1;
const GLOW_ALPHA = 0.3;

/** How long a column's peak takes to fall from full scale to nothing. */
const PEAK_FALL_SECONDS = 0.6;

/** The arming idle: one dot walking the centre row, this many columns a second,
 *  and how lit it is.
 *
 *  Not the lane's shared `ARMING_ALPHA`: a single small dot on a bare lane
 *  disappears at 0.35, so it is drawn brighter to read as a lamp. */
const WALK_COLUMNS_PER_SECOND = 14;
const ARMING_LIT = 0.75;

/** Per column: how many steps are lit, where the held peak sits, and the clock
 *  that peak falls over. Allocated once, so a frame allocates nothing. */
const litSteps = new Int8Array(MAX_COLUMNS);
const peakStep = new Int8Array(MAX_COLUMNS);
const peak = new Float32Array(MAX_COLUMNS);
const clock = newFrameClock();

/** The frame's own grid, allocated once and refilled per frame, so the two
 *  tracing helpers take what they draw rather than eight measurements each, and
 *  a frame still builds no closure. */
const panel = {
  firstX: 0,
  centreY: 0,
  cell: 0,
  dot: 0,
  steps: 0,
  columns: 0,
  width: 0,
  height: 0,
};

/** One dot, grown by the glow's spread and kept inside the lane, so the
 *  outermost row's halo is never the thing that clips. */
function traceDot(
  ctx: CanvasRenderingContext2D,
  column: number,
  offset: number,
  grow: number,
): void {
  const x = panel.firstX + column * panel.cell;
  const y = panel.centreY + offset * panel.cell;
  const left = Math.max(0, x - grow);
  const top = Math.max(0, y - grow);
  ctx.rect(
    left,
    top,
    Math.min(panel.width, x + panel.dot + grow) - left,
    Math.min(panel.height, y + panel.dot + grow) - top,
  );
}

/** What is lit: each column's stack out from the centre row, mirrored above and
 *  below, plus the held peak where it hangs clear of the stack. Traced twice a
 *  frame, once grown for the glow and once for the dots. */
function traceLit(ctx: CanvasRenderingContext2D, grow: number): void {
  ctx.beginPath();
  for (let column = 0; column < panel.columns; column += 1) {
    const top = litSteps[column];
    if (top < 0) continue;
    for (let offset = 0; offset <= top; offset += 1) {
      traceDot(ctx, column, -offset, grow);
      if (offset > 0) traceDot(ctx, column, offset, grow);
    }
    if (peakStep[column] > top) {
      traceDot(ctx, column, -peakStep[column], grow);
      traceDot(ctx, column, peakStep[column], grow);
    }
  }
}

export const drawMatrix: WaveformDraw = (
  ctx,
  levels,
  elapsedMs,
  geom,
  colors,
  flags,
) => {
  const { width, height, unit, gap } = geom;
  const rows = Math.floor(height / ROWS_TALL) >= TALL_MIN_PITCH ? ROWS_TALL : ROWS_SHORT; // prettier-ignore
  const pitch = Math.max(MIN_DOT + 1, Math.floor(height / rows));
  const spacing = Math.max(
    1,
    Math.min(Math.round(Math.min(gap, pitch * GAP_SHARE)), pitch - MIN_DOT),
  );
  const dot = Math.max(MIN_DOT, Math.min(pitch - spacing, Math.round(unit)));
  const cell = dot + spacing;
  const columns = Math.max(
    4,
    Math.min(MAX_COLUMNS, Math.floor((width + spacing) / cell)),
  );
  // Whole device pixels throughout, and the centre row placed first, so every
  // row above the centre has an exact mirror below it.
  panel.firstX = Math.round((width - (columns * cell - spacing)) / 2);
  panel.centreY = Math.round((height - dot) / 2);
  panel.cell = cell;
  panel.dot = dot;
  panel.steps = (rows - 1) / 2;
  panel.columns = columns;
  panel.width = width;
  panel.height = height;
  const steps = panel.steps;
  const span = columns - 1;

  const seconds = animationSeconds(elapsedMs, flags);
  const step = frameSeconds(clock, elapsedMs, flags);
  const travel = (seconds * WALK_COLUMNS_PER_SECOND) / span;
  const phase = travel % 2;
  const walked = Math.round((phase < 1 ? phase : 2 - phase) * span);

  for (let column = 0; column < columns; column += 1) {
    if (!flags.ready) {
      litSteps[column] = column === walked ? 0 : -1;
      peakStep[column] = -1;
      continue;
    }
    // Evenly across the lane, first column on the first bucket and last on the
    // last, so a quiet end of the spectrum cannot bunch the meter to one side.
    const level = Math.min(
      1,
      Math.max(0, sampleLevel(levels, (column / span) * (levels.length - 1))),
    );
    // A frozen clock holds no peak, so a reduced-motion frame is a pure
    // function of the levels.
    peak[column] =
      step > 0
        ? Math.max(level, peak[column] - step / PEAK_FALL_SECONDS)
        : level;
    litSteps[column] = Math.round(level * steps);
    peakStep[column] = Math.round(peak[column] * steps);
  }

  // What is lit, traced twice: a spread pass for the glow, then the dots
  // themselves over it.
  const base = flags.ready ? 1 : ARMING_LIT;
  ctx.fillStyle = paintColor(colors, flags);
  traceLit(ctx, GLOW_SPREAD);
  ctx.globalAlpha = base * GLOW_ALPHA;
  ctx.fill();
  traceLit(ctx, 0);
  ctx.globalAlpha = base;
  ctx.fill();
};
