import { describe, expect, test } from "bun:test";
import en from "@/i18n/locales/en/translation.json";
import { WAVEFORM_STYLES } from "@/lib/overlayTheme";
import { WAVEFORM_RENDERERS } from "./renderers";
import {
  drawWaveformFrame,
  initialWaveformState,
  type WaveformBackingStore,
} from "./waveformFrame";
import type {
  WaveformColors,
  WaveformFlags,
  WaveformGeometry,
} from "./waveformLane";
import {
  isCanvasWaveformStyle,
  STYLES_USING_WAVEFORM_GAP,
  STYLES_USING_WAVEFORM_WIDTH,
  WAVEFORM_STYLE_TOKENS,
  type CanvasWaveformStyle,
} from "./waveformStyles";

/**
 * The waveform styles, driven off the token's value list so a style added in
 * Rust cannot slip through with no renderer, label or declared lengths, and
 * drawn against a recording stub so what each asks of a canvas is a fact in a
 * test, not a promise in a comment.
 *
 * No DOM here. The stub is the point: a style may only reach for the handful
 * of operations below, and `drawWaveformFrame` is separate from the component
 * so the "is this frame worth drawing" rules are testable too.
 */

/**
 * What each style may touch on a context, one list per style rather than one
 * union: a style is a shape, and the operations it reaches for are that shape's
 * own vocabulary. A curve in the histogram, or a transform in a style with no
 * cached brush, is a redesign and should read as one here.
 *
 * Anything outside these lists is also a missing method on the stub, so the
 * test fails rather than passing blind.
 */
const WHITELIST: Record<CanvasWaveformStyle, readonly string[]> = {
  // One filled polygon, walked out along the top and back along the bottom.
  ribbon: [
    "beginPath",
    "moveTo",
    "lineTo",
    "closePath",
    "fillStyle",
    "globalAlpha",
    "fill",
  ],
  // One closed spline, traced three times: glow, silhouette, highlight.
  bloom: [
    "beginPath",
    "moveTo",
    "quadraticCurveTo",
    "closePath",
    "fillStyle",
    "globalAlpha",
    "fill",
  ],
  // One cached unit-space gradient, placed per mote by a transform.
  motes: [
    "createRadialGradient",
    "fillStyle",
    "globalAlpha",
    "setTransform",
    "beginPath",
    "arc",
    "fill",
  ],
  // Whole-pixel squares, two passes: the glow, then the lit dots.
  matrix: ["beginPath", "rect", "fillStyle", "globalAlpha", "fill"],
  // Whole-pixel squares, one pass.
  steps: ["beginPath", "rect", "fillStyle", "globalAlpha", "fill"],
};

/** A context that records what was asked of it, and can do nothing else.
 *
 *  `calls` is the operation log, `painted` every colour that reached the
 *  canvas: a soft-edged style hands `fillStyle` a cached gradient, not a colour
 *  string, so its accent is a stop, not a fill, and both have to count. */
function stubContext(): {
  ctx: CanvasRenderingContext2D;
  calls: string[];
  ops: () => string[];
  painted: string[];
} {
  const calls: string[] = [];
  const painted: string[] = [];
  const record =
    (op: string) =>
    (...args: number[]) =>
      calls.push(`${op}(${args.map((n) => n.toFixed(3)).join(",")})`);
  const stub = {
    set fillStyle(value: string | CanvasGradient) {
      if (typeof value === "string") {
        calls.push(`fillStyle=${value}`);
        painted.push(value);
      } else calls.push("fillStyle=gradient");
    },
    set globalAlpha(value: number) {
      calls.push(`globalAlpha=${value.toFixed(3)}`);
    },
    beginPath: record("beginPath"),
    closePath: record("closePath"),
    moveTo: record("moveTo"),
    lineTo: record("lineTo"),
    quadraticCurveTo: record("quadraticCurveTo"),
    rect: record("rect"),
    arc: record("arc"),
    fill: record("fill"),
    setTransform: record("setTransform"),
    clearRect: record("clearRect"),
    createRadialGradient: (...args: number[]) => {
      calls.push(`createRadialGradient(${args.map((n) => n.toFixed(3)).join(",")})`); // prettier-ignore
      return {
        addColorStop: (offset: number, color: string) => painted.push(color),
      };
    },
  };
  return {
    ctx: stub as unknown as CanvasRenderingContext2D,
    calls,
    ops: () => calls.map((call) => call.replace(/[(=].*$/, "")),
    painted,
  };
}

/** The lane at the inherit tokens on a 2x display: 60x18 CSS points. */
const GEOM: WaveformGeometry = {
  width: 120,
  height: 36,
  unit: 8,
  gap: 6,
  dpr: 2,
};
const COLORS: WaveformColors = { accent: "rgb(1,2,3)", muted: "rgb(4,5,6)" };

const speech = (): Float32Array =>
  Float32Array.from(
    Array.from({ length: 16 }, (_, bucket) => 0.2 + 0.5 * ((bucket % 5) / 4)),
  );
const silence = (): Float32Array => new Float32Array(16);

const flags = (over: Partial<WaveformFlags> = {}): WaveformFlags => ({
  ready: true,
  reduceMotion: false,
  ...over,
});

const canvasStyles = WAVEFORM_STYLES.filter(isCanvasWaveformStyle);

describe("the six styles as a table", () => {
  test("every style has a renderer, a label and a declared pair of lengths", () => {
    const options: Record<string, string> =
      en.settings.appearance.waveformStyle.options;
    for (const style of WAVEFORM_STYLES) {
      expect(typeof options[style]).toBe("string");
      expect(typeof WAVEFORM_STYLE_TOKENS[style].usesWidth).toBe("boolean");
      expect(typeof WAVEFORM_STYLE_TOKENS[style].usesGap).toBe("boolean");
      // Bars is the DOM path and the inherit, so it has no renderer at all.
      const drawn = (WAVEFORM_RENDERERS as Record<string, unknown>)[style];
      expect(typeof drawn).toBe(style === "bars" ? "undefined" : "function");
    }
    expect(canvasStyles).toEqual([
      "ribbon",
      "bloom",
      "motes",
      "matrix",
      "steps",
    ]);
  });

  test("the tab's two derived lists are the table's own answer", () => {
    expect(STYLES_USING_WAVEFORM_WIDTH).toEqual([
      "bars",
      "ribbon",
      "motes",
      "matrix",
      "steps",
    ]);
    expect(STYLES_USING_WAVEFORM_GAP).toEqual(["bars", "matrix"]);
  });
});

describe("what a style asks of a canvas", () => {
  test("only its own operations, and each style draws something", () => {
    for (const style of canvasStyles) {
      // Both states: a style's arming idle is a different drawing and must
      // stay inside the same vocabulary.
      for (const ready of [true, false]) {
        const { ctx, calls, ops, painted } = stubContext();
        const levels = ready ? speech() : silence();
        WAVEFORM_RENDERERS[style](
          ctx,
          levels,
          400,
          GEOM,
          COLORS,
          flags({ ready }),
        );
        expect(calls.length).toBeGreaterThan(0);
        for (const op of new Set(ops())) {
          expect(WHITELIST[style]).toContain(op);
        }
        // Every style ends in pixels and paints the accent while audio flows.
        expect(ops()).toContain("fill");
        if (ready) expect(painted).toContain(COLORS.accent);
      }
    }
  });

  test("different levels draw a different picture", () => {
    // What a meter is for. Every rule above would hold for a style that drew
    // one shape whatever it was handed, so the levels must be shown reaching
    // the pixels. Drawn under reduced motion, where a frame is a pure function
    // of the levels, so they are the only difference between the two.
    const frozen = flags({ reduceMotion: true });
    for (const style of canvasStyles) {
      const quiet = stubContext();
      const loud = stubContext();
      WAVEFORM_RENDERERS[style](quiet.ctx, silence(), 0, GEOM, COLORS, frozen);
      WAVEFORM_RENDERERS[style](loud.ctx, speech(), 0, GEOM, COLORS, frozen);
      expect(loud.calls).not.toEqual(quiet.calls);
    }
  });

  test("a style that leaves the canvas transformed puts it back", () => {
    // A style may transform the context to place a cached gradient, but the
    // frame after it is handed the same context and starts at the origin.
    for (const style of canvasStyles) {
      const { ctx, calls } = stubContext();
      WAVEFORM_RENDERERS[style](ctx, speech(), 400, GEOM, COLORS, flags());
      const transforms = calls.filter((call) =>
        call.startsWith("setTransform"),
      );
      if (transforms.length === 0) continue;
      expect(transforms[transforms.length - 1]).toBe(
        "setTransform(1.000,0.000,0.000,1.000,0.000,0.000)",
      );
    }
  });

  test("the arming idle paints the muted colour instead", () => {
    for (const style of canvasStyles) {
      const { ctx, ops, painted } = stubContext();
      WAVEFORM_RENDERERS[style](
        ctx,
        silence(),
        400,
        GEOM,
        COLORS,
        flags({ ready: false }),
      );
      // Silent and not yet capturing: the style still draws its own idle,
      // which says the shortcut was heard.
      expect(ops()).toContain("fill");
      expect(painted).toContain(COLORS.muted);
      expect(painted).not.toContain(COLORS.accent);
    }
  });

  test("reduced motion makes a frame a function of the levels alone", () => {
    for (const style of canvasStyles) {
      for (const ready of [true, false]) {
        const levels = ready ? speech() : silence();
        const early = stubContext();
        const late = stubContext();
        const reduced = flags({ ready, reduceMotion: true });
        WAVEFORM_RENDERERS[style](early.ctx, levels, 0, GEOM, COLORS, reduced);
        WAVEFORM_RENDERERS[style](
          late.ctx,
          levels,
          9_000,
          GEOM,
          COLORS,
          reduced,
        );
        expect(late.calls).toEqual(early.calls);
      }
    }
  });

  test("with motion allowed, every idle moves on its own", () => {
    // The arming idle is the one thing every style animates without levels.
    // While capturing, `matrix` and `steps` are level-driven by design, so a
    // still microphone is a still meter, which is what a VU should do.
    for (const style of canvasStyles) {
      const early = stubContext();
      const late = stubContext();
      const arming = flags({ ready: false });
      WAVEFORM_RENDERERS[style](early.ctx, silence(), 0, GEOM, COLORS, arming);
      WAVEFORM_RENDERERS[style](late.ctx, silence(), 617, GEOM, COLORS, arming);
      expect(late.calls).not.toEqual(early.calls);
    }
  });

  test("nothing is drawn outside the lane's own box", () => {
    // The lane is a slot the card's geometry is built from, so a style whose
    // outline reached past it would be clipped, not resized. The motes are the
    // exception on purpose: they drift up out of the lane and fade, and an
    // `arc` is not an outline this walks.
    const within = (value: number, limit: number) =>
      value >= -0.51 && value <= limit + 0.51;
    for (const style of canvasStyles) {
      const { ctx, calls } = stubContext();
      WAVEFORM_RENDERERS[style](ctx, speech(), 1_200, GEOM, COLORS, flags());
      for (const call of calls) {
        const [op, rest] = call.split("(");
        if (
          op !== "moveTo" &&
          op !== "lineTo" &&
          op !== "quadraticCurveTo" &&
          op !== "rect"
        )
          continue;
        const numbers = rest.replace(")", "").split(",").map(Number);
        expect(within(numbers[0], GEOM.width)).toBe(true);
        // `rect` is x, y, width, height; the vertical extent is the sum. A
        // quadratic is a control point then an end point, both bounding the
        // curve, so both are checked.
        const right = op === "rect" ? numbers[0] + numbers[2] : numbers[2];
        const bottom = op === "rect" ? numbers[1] + numbers[3] : numbers[3];
        expect(within(numbers[1], GEOM.height)).toBe(true);
        if (numbers.length > 2) {
          expect(within(right, GEOM.width)).toBe(true);
          expect(within(bottom, GEOM.height)).toBe(true);
        }
      }
    }
  });
});

describe("the frame", () => {
  const frameState = (over: Partial<WaveformFlags> = {}) => {
    const state = initialWaveformState("steps");
    const { ctx, calls, ops } = stubContext();
    state.ctx = ctx;
    state.cssWidth = 60;
    state.cssHeight = 18;
    state.unitCss = 4;
    state.gapCss = 3;
    state.flags = flags(over);
    return { state, calls, ops };
  };
  const canvas = (): WaveformBackingStore => ({ width: 0, height: 0 });

  test("the backing store is the CSS box at the device ratio, written once", () => {
    const { state } = frameState();
    const store = canvas();
    drawWaveformFrame(store, state, [0.5], 0, 2);
    expect([store.width, store.height]).toEqual([120, 36]);

    // Writing either dimension clears the canvas, so an unchanged box must
    // not touch it. A sentinel proves the frame left it alone.
    store.width = 999;
    drawWaveformFrame(store, state, [0.5], 16, 2);
    expect(store.width).toBe(120);
    store.width = 120;
    drawWaveformFrame(store, state, [0.5], 32, 2);
    expect(store.width).toBe(120);
  });

  test("an unmeasured lane draws nothing at all", () => {
    const { state, calls } = frameState();
    state.cssWidth = 0;
    drawWaveformFrame(canvas(), state, [0.5], 0, 2);
    expect(calls).toEqual([]);
  });

  test("under reduced motion an unchanged frame is skipped", () => {
    const { state, calls } = frameState({ reduceMotion: true });
    const store = canvas();
    drawWaveformFrame(store, state, [0.5], 0, 2);
    const drawn = calls.length;
    expect(drawn).toBeGreaterThan(0);

    drawWaveformFrame(store, state, [0.5], 16, 2);
    expect(calls.length).toBe(drawn);

    // A moved bucket is worth a frame again.
    drawWaveformFrame(store, state, [0.9], 32, 2);
    expect(calls.length).toBeGreaterThan(drawn);
  });

  test("with motion allowed every frame is drawn", () => {
    const { state, calls } = frameState();
    const store = canvas();
    drawWaveformFrame(store, state, [0.5], 0, 2);
    const drawn = calls.length;
    drawWaveformFrame(store, state, [0.5], 16, 2);
    expect(calls.length).toBeGreaterThan(drawn);
  });

  test("a frame clears the lane before the style paints it", () => {
    const { state, ops } = frameState();
    drawWaveformFrame(canvas(), state, [0.5], 0, 2);
    expect(ops()[0]).toBe("clearRect");
  });

  test("a canvas with no context draws nothing", () => {
    const { state, calls } = frameState();
    state.ctx = null;
    drawWaveformFrame(canvas(), state, [0.5], 0, 2);
    expect(calls).toEqual([]);
  });

  test("the style's lengths follow the device ratio", () => {
    const { state } = frameState();
    drawWaveformFrame(canvas(), state, [0.5], 0, 2);
    expect([state.geom.unit, state.geom.gap]).toEqual([8, 6]);
    drawWaveformFrame(canvas(), state, [0.5], 16, 1);
    expect([state.geom.unit, state.geom.gap]).toEqual([4, 3]);
  });
});

/** The five styles' own type is the five, and the type-level guarantee that a
 *  new style cannot be forgotten shows up here as a value. */
test("the canvas styles are every style but the bars", () => {
  const styles: CanvasWaveformStyle[] = [...canvasStyles];
  expect(styles.includes("bars" as CanvasWaveformStyle)).toBe(false);
  expect(styles.length).toBe(WAVEFORM_STYLES.length - 1);
});

describe("bloom", () => {
  /** The alpha each fill actually went down at, in order. */
  const fillAlphas = (calls: string[]): number[] => {
    const alphas: number[] = [];
    let alpha = 1;
    for (const call of calls) {
      if (call.startsWith("globalAlpha")) alpha = Number(call.split("=")[1]);
      if (call.startsWith("fill(")) alphas.push(alpha);
    }
    return alphas;
  };

  /** The control points of each traced outline, one array per pass. */
  const passes = (calls: string[]): number[][] => {
    const traced: number[][] = [];
    for (const call of calls) {
      if (call.startsWith("beginPath")) traced.push([]);
      if (!call.startsWith("quadraticCurveTo") || traced.length === 0) continue;
      const numbers = call.slice(call.indexOf("(") + 1, -1).split(",");
      traced[traced.length - 1].push(Number(numbers[0]), Number(numbers[1]));
    }
    return traced;
  };

  test("a crisp silhouette, an inner highlight and a whisper of glow", () => {
    // Three passes and no more. The first attempt, a stack of nested fills,
    // accumulates into a smear brightest wherever the outline bulges: the blob
    // read as lopsided even on even buckets.
    const { ctx, calls } = stubContext();
    WAVEFORM_RENDERERS.bloom(ctx, speech(), 400, GEOM, COLORS, flags());
    const alphas = fillAlphas(calls);
    expect(alphas.length).toBe(3);
    const [glow, body, highlight] = alphas;
    expect(glow).toBeLessThanOrEqual(0.25);
    expect(body).toBeGreaterThanOrEqual(0.85);
    expect(highlight).toBeGreaterThanOrEqual(body);
  });

  test("the silhouette stays centred whatever one bucket says", () => {
    // A loud bucket must grow the surface where it points rather than drag
    // the whole blob to that side of the lane.
    const oneLoudBucket = Float32Array.from(
      Array.from({ length: 16 }, (_, bucket) => (bucket === 3 ? 0.9 : 0.05)),
    );
    const { ctx, calls } = stubContext();
    WAVEFORM_RENDERERS.bloom(ctx, oneLoudBucket, 400, GEOM, COLORS, flags());
    const silhouette = passes(calls)[1];
    expect(silhouette.length).toBe(32);
    let middleX = 0;
    let middleY = 0;
    for (let point = 0; point < silhouette.length; point += 2) {
      middleX += silhouette[point];
      middleY += silhouette[point + 1];
    }
    const half = silhouette.length / 2;
    expect(Math.abs(middleX / half - GEOM.width / 2)).toBeLessThan(0.5);
    expect(Math.abs(middleY / half - GEOM.height / 2)).toBeLessThan(0.5);
  });
});
