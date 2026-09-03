import { describe, expect, test } from "bun:test";
import {
  cardPropsAt,
  LIVE_SEQUENCE,
  MINIMAL_SEQUENCE,
  offsetFor,
  pinnableStatesFor,
  revealedText,
  stepAt,
  syntheticLevels,
  totalDurationMs,
} from "./useOverlayPreviewDriver";

/**
 * Unit tests for the preview driver's pure state machine. Every duration is
 * transcribed from ticket 03 §3a: Minimal totals 8.1s (0.8 + 3.7 + 1.6 + 1.4
 * + 0.6 gap), Live totals 7.7s (0.8 + 4.4 + 1.9 + 0.6 gap).
 */

describe("sequence durations", () => {
  test("Minimal totals 8100ms", () => {
    expect(totalDurationMs(MINIMAL_SEQUENCE)).toBe(8100);
  });
  test("Live totals 7700ms", () => {
    expect(totalDurationMs(LIVE_SEQUENCE)).toBe(7700);
  });
});

describe("stepAt", () => {
  test("walks the Minimal sequence in order", () => {
    expect(stepAt(MINIMAL_SEQUENCE, 0).name).toBe("arming");
    expect(stepAt(MINIMAL_SEQUENCE, 799).name).toBe("arming");
    expect(stepAt(MINIMAL_SEQUENCE, 800).name).toBe("recording");
    expect(stepAt(MINIMAL_SEQUENCE, 800 + 3700 - 1).name).toBe("recording");
    expect(stepAt(MINIMAL_SEQUENCE, 800 + 3700).name).toBe("transcribing");
    expect(stepAt(MINIMAL_SEQUENCE, 800 + 3700 + 1600).name).toBe("processing");
    expect(stepAt(MINIMAL_SEQUENCE, 800 + 3700 + 1600 + 1400).name).toBe("gap");
  });

  test("wraps to the next cycle after the total duration", () => {
    const total = totalDurationMs(MINIMAL_SEQUENCE);
    const first = stepAt(MINIMAL_SEQUENCE, 100);
    const secondCycle = stepAt(MINIMAL_SEQUENCE, total + 100);
    expect(secondCycle.name).toBe(first.name);
    expect(secondCycle.elapsedInStepMs).toBe(first.elapsedInStepMs);
    expect(first.cycle).toBe(0);
    expect(secondCycle.cycle).toBe(1);
  });

  test("elapsedInStepMs resets at each step boundary", () => {
    const active = stepAt(MINIMAL_SEQUENCE, 800 + 100);
    expect(active.name).toBe("recording");
    expect(active.elapsedInStepMs).toBe(100);
  });

  test("negative elapsed time is treated as zero", () => {
    expect(stepAt(MINIMAL_SEQUENCE, -50).name).toBe("arming");
  });
});

describe("pinnableStatesFor", () => {
  test("Minimal offers arming, recording, transcribing, processing", () => {
    expect(pinnableStatesFor("minimal")).toEqual([
      "arming",
      "recording",
      "transcribing",
      "processing",
    ]);
  });
  test("Live offers arming, listening, transcribing — no separate processing state", () => {
    expect(pinnableStatesFor("live")).toEqual([
      "arming",
      "listening",
      "transcribing",
    ]);
  });
});

describe("offsetFor", () => {
  test("is the midpoint of the named step", () => {
    // arming: [0, 800) -> midpoint 400
    expect(offsetFor(MINIMAL_SEQUENCE, "arming")).toBe(400);
    // recording: [800, 4500) -> midpoint 800 + 1850 = 2650
    expect(offsetFor(MINIMAL_SEQUENCE, "recording")).toBe(2650);
  });
});

describe("syntheticLevels", () => {
  test("returns 16 buckets, every one within [0, 1]", () => {
    const levels = syntheticLevels(1234);
    expect(levels.length).toBe(16);
    levels.forEach((v) => {
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(1);
    });
  });

  test("is deterministic — the same instant always produces the same levels", () => {
    expect(syntheticLevels(2500)).toEqual(syntheticLevels(2500));
  });

  test("animates — different instants produce different levels", () => {
    expect(syntheticLevels(0)).not.toEqual(syntheticLevels(500));
  });
});

describe("revealedText", () => {
  const sample = "The quick brown fox jumps";

  test("reveals one word per msPerWord, holding the next as tentative", () => {
    expect(revealedText(sample, 0, 260)).toEqual({
      committed: "",
      tentative: "The",
    });
    expect(revealedText(sample, 260, 260)).toEqual({
      committed: "The",
      tentative: "quick",
    });
    expect(revealedText(sample, 260 * 2, 260)).toEqual({
      committed: "The quick",
      tentative: "brown",
    });
  });

  test("stops growing once every word is revealed", () => {
    const result = revealedText(sample, 260 * 100, 260);
    expect(result).toEqual({ committed: sample, tentative: "" });
  });

  test("empty sample text reveals nothing", () => {
    expect(revealedText("   ", 1000, 260)).toEqual({
      committed: "",
      tentative: "",
    });
  });
});

describe("cardPropsAt", () => {
  test("Minimal: arming is not capture-ready; recording is", () => {
    const arming = cardPropsAt("minimal", 100, "sample", true);
    expect(arming.mounted).toBe(true);
    expect(arming.activeState).toBe("arming");
    expect(arming.props.state).toBe("recording");
    expect(arming.props.captureReady).toBe(false);

    const recording = cardPropsAt("minimal", 900, "sample", true);
    expect(recording.activeState).toBe("recording");
    expect(recording.props.state).toBe("recording");
    expect(recording.props.captureReady).toBe(true);
  });

  test("Minimal: the gap step is unmounted", () => {
    const gapStart = 800 + 3700 + 1600 + 1400;
    const gap = cardPropsAt("minimal", gapStart + 10, "sample", true);
    expect(gap.mounted).toBe(false);
  });

  test("Live: listening streams text; the working step holds it", () => {
    const midListening = 800 + 2000; // 1200ms into the 4400ms listening step
    const listening = cardPropsAt(
      "live",
      midListening,
      "sample text here",
      true,
    );
    expect(listening.activeState).toBe("listening");
    expect(listening.props.state).toBe("streaming");
    expect(listening.props.phase).toBe("listening");
    expect(listening.props.streamText.committed.length).toBeGreaterThan(0);

    const working = cardPropsAt(
      "live",
      800 + 4400 + 100,
      "sample text here",
      true,
    );
    expect(working.activeState).toBe("transcribing");
    expect(working.props.phase).toBe("working");
    // Held at exactly what the full 4400ms listening step revealed, not
    // restarted from the working step's own (100ms) clock.
    expect(working.props.streamText).toEqual(
      revealedText("sample text here", 4400, 260),
    );
  });

  test("animated=false yields a static, flat mid-height waveform", () => {
    const first = cardPropsAt("minimal", 900, "sample", false);
    const second = cardPropsAt("minimal", 1900, "sample", false);
    expect(first.props.levels).toEqual(second.props.levels);
    first.props.levels.forEach((v) => expect(v).toBe(0.5));
  });

  test("session equals the sequence's cycle count", () => {
    const total = totalDurationMs(MINIMAL_SEQUENCE);
    const cycle0 = cardPropsAt("minimal", 900, "sample", true);
    const cycle2 = cardPropsAt("minimal", total * 2 + 900, "sample", true);
    expect(cycle0.session).toBe(0);
    expect(cycle2.session).toBe(2);
  });
});
