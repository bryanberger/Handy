import {
  frameSeconds,
  loudness,
  newFrameClock,
  newSoftGradient,
  paintAlpha,
  paintColor,
  softGradient,
  type WaveformGeometry,
  type WaveformDraw,
} from "./waveformLane";

/**
 * Motes: a field of soft round particles drifting up out of the lane. Loudness
 * lights more of them and throws them further, so speech reads as sparks.
 * `waveform_width` is a mote's diameter.
 *
 * A bounded pool, filled once and reused: a mote is spawned at a column drawn
 * from the buckets themselves, so the field follows the spectrum without ever
 * bunching into one, rises with a little sideways drift, and shrinks and fades
 * over its own life. Every random number comes from one seeded sequence, so
 * the field is identical on every machine and every run.
 *
 * Under reduced motion the pool is not simulated at all: it is seeded into a
 * still field that follows the levels and nothing else, which is what makes
 * two timestamps at the same levels draw the same picture.
 */

const TAU = Math.PI * 2;

/** How many motes the field may hold at once. */
const POOL = 64;

/** How hard the overall level drives the field: how many motes are spawned a
 *  second at silence and at full voice. */
const LEVEL_GAIN = 1.9;
const SPAWN_MIN = 9;
const SPAWN_SPAN = 95;

/** The share of the pool a still field holds at silence. The rest is the
 *  loudness, so a quiet lane sparkles and a loud one fills. */
const FROZEN_MIN = 0.18;

/** Every bucket's share of the spawns, over and above its own level. Without
 *  it a single loud bucket takes every mote and the field reads as a clump. */
const SPREAD_FLOOR = 0.35;

/** How much of the lane's height a mote is spawned into, measured up from the
 *  baseline. */
const BASE_BAND = 0.14;

/** How fast a mote rises, in lanes a second, at silence and at full voice,
 *  and how far it wanders sideways. Fast enough that a loud field reaches the
 *  top of the lane inside one life, which is what makes loudness read as
 *  throw rather than as count alone. */
const RISE_MIN = 0.62;
const RISE_SPAN = 1.6;
const DRIFT = 0.35;

/** How long a mote lives, in seconds. */
const LIFE_MIN = 0.5;
const LIFE_SPAN = 0.4;

/** A mote's radius as a share of `waveform_width`, and how much of it it
 *  loses over its life. */
const SIZE_MIN = 0.36;
const SIZE_SPAN = 0.3;
const SHRINK = 0.4;

/** How quickly a mote lights, and how quickly it goes out, both as a multiple
 *  of its own life. Between the two it burns at full: a mote that dimmed from
 *  the moment it appeared spent its whole rise as a smudge. */
const ATTACK = 7;
const DECAY = 3;

/** Where a mote's gradient stops being solid colour: a bright centre in a
 *  soft falloff, which is what gives it no edge at all. */
const CORE_STOP = 0.42;

/** The loudness the arming idle pretends to hear, so a few motes drift while
 *  the shortcut is acknowledged. */
const IDLE_LOUDNESS = 0.16;

/** The seeded sequence's start. Any odd constant; this one is arbitrary. */
const SEED = 0x9e3779b9 | 0;

/** The pool, the sequence and the clock, all allocated once, so a frame
 *  allocates nothing. A mote is alive while `age < life`. */
const positionX = new Float32Array(POOL);
const positionY = new Float32Array(POOL);
const velocityX = new Float32Array(POOL);
const velocityY = new Float32Array(POOL);
const age = new Float32Array(POOL);
const life = new Float32Array(POOL);
const size = new Float32Array(POOL);
const clock = newFrameClock();
const brush = newSoftGradient();
let seed = SEED;
let cursor = 0;
let debt = 0;

/** The seeded sequence: xorshift32, folded into `0..1`. */
function random(): number {
  seed ^= seed << 13;
  seed ^= seed >>> 17;
  seed ^= seed << 5;
  return (seed >>> 0) / 4294967296;
}

/** A bucket drawn in proportion to its own level, over a floor that keeps the
 *  field spread across the whole lane. */
function pickBucket(levels: Float32Array): number {
  let total = 0;
  for (let bucket = 0; bucket < levels.length; bucket += 1) {
    total += Math.max(0, levels[bucket]) + SPREAD_FLOOR;
  }
  let drawn = random() * total;
  for (let bucket = 0; bucket < levels.length; bucket += 1) {
    drawn -= Math.max(0, levels[bucket]) + SPREAD_FLOOR;
    if (drawn <= 0) return bucket;
  }
  return levels.length - 1;
}

/**
 * Fill one slot with a new mote, already `travelled` of the way through its
 * own life, so the same function both spawns one at the baseline and seeds a
 * still field of motes caught mid-rise.
 */
function spawn(
  mote: number,
  levels: Float32Array,
  geom: WaveformGeometry,
  travelled: number,
): void {
  const bucket = pickBucket(levels);
  const level = Math.min(1, Math.max(0, levels[bucket]));
  life[mote] = LIFE_MIN + LIFE_SPAN * random();
  size[mote] = geom.unit * (SIZE_MIN + SIZE_SPAN * random());
  age[mote] = travelled * life[mote];
  velocityX[mote] = (random() - 0.5) * DRIFT * geom.height;
  velocityY[mote] = -(RISE_MIN + RISE_SPAN * level) * geom.height;
  positionX[mote] =
    ((bucket + random()) / levels.length) * geom.width +
    velocityX[mote] * age[mote];
  positionY[mote] =
    geom.height * (1 - BASE_BAND * random()) + velocityY[mote] * age[mote];
}

/** The still field a frozen clock draws: the sequence restarted and the pool
 *  refilled from the levels alone, so the same levels always give the same
 *  field however long the overlay has been up. */
function freeze(
  levels: Float32Array,
  geom: WaveformGeometry,
  shaped: number,
): void {
  seed = SEED;
  debt = 0;
  const wanted = Math.round(POOL * (FROZEN_MIN + (1 - FROZEN_MIN) * shaped));
  for (let mote = 0; mote < POOL; mote += 1) {
    if (mote < wanted) spawn(mote, levels, geom, random());
    else life[mote] = 0;
  }
}

export const drawMotes: WaveformDraw = (
  ctx,
  levels,
  elapsedMs,
  geom,
  colors,
  flags,
) => {
  const step = frameSeconds(clock, elapsedMs, flags);
  const level = flags.ready ? loudness(levels) : IDLE_LOUDNESS;
  const shaped = Math.min(1, level * LEVEL_GAIN);

  if (step <= 0) {
    freeze(levels, geom, shaped);
  } else {
    for (let mote = 0; mote < POOL; mote += 1) {
      if (age[mote] >= life[mote]) continue;
      age[mote] += step;
      positionX[mote] += velocityX[mote] * step;
      positionY[mote] += velocityY[mote] * step;
    }
    debt = Math.min(POOL, debt + (SPAWN_MIN + SPAWN_SPAN * shaped) * step);
    while (debt >= 1) {
      debt -= 1;
      let slot = -1;
      for (let tried = 0; tried < POOL; tried += 1) {
        cursor = (cursor + 1) % POOL;
        if (age[cursor] >= life[cursor]) {
          slot = cursor;
          break;
        }
      }
      if (slot < 0) break;
      spawn(slot, levels, geom, 0);
    }
  }

  const base = paintAlpha(flags);
  ctx.fillStyle = softGradient(
    brush,
    ctx,
    paintColor(colors, flags),
    CORE_STOP,
  );
  for (let mote = 0; mote < POOL; mote += 1) {
    if (!(age[mote] < life[mote])) continue;
    const spent = age[mote] / life[mote];
    const radius = size[mote] * (1 - SHRINK * spent);
    const alpha =
      base * Math.min(1, spent * ATTACK) * Math.min(1, (1 - spent) * DECAY);
    if (radius <= 0 || alpha <= 0) continue;
    // The mote is drawn in its own unit space, so one cached gradient is every
    // mote's soft edge whatever it measures.
    ctx.globalAlpha = alpha;
    ctx.setTransform(radius, 0, 0, radius, positionX[mote], positionY[mote]);
    ctx.beginPath();
    ctx.arc(0, 0, 1, 0, TAU);
    ctx.fill();
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);
};
