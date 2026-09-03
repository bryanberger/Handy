import { describe, expect, test } from "bun:test";
import type { PreviewState } from "@/bindings";
import {
  IDLE_PREVIEW,
  previewBlocker,
  previewChipsFor,
  reducePreview,
  type PreviewMode,
} from "./previewMode";

/**
 * Unit tests for preview mode's state machine — the tab's half of the
 * contract. The backend enforces the same rules for itself (a preview refuses
 * to start while recording, ends when a real recording takes the overlay);
 * these pin what the *tab* does about them.
 */

const running = (state: PreviewState = "cycle"): PreviewMode => ({
  running: true,
  state,
});

describe("previewChipsFor", () => {
  test("cycle is always the first chip", () => {
    expect(previewChipsFor("live")[0]).toBe("cycle");
    expect(previewChipsFor("minimal")[0]).toBe("cycle");
  });

  test("each style offers only the states it actually has", () => {
    // Capture is one state under two names: Live calls it listening (the
    // panel is open, text arriving), Minimal calls it recording.
    expect(previewChipsFor("live")).toEqual([
      "cycle",
      "arming",
      "listening",
      "transcribing",
    ]);
    expect(previewChipsFor("minimal")).toEqual([
      "cycle",
      "arming",
      "recording",
      "transcribing",
      "processing",
    ]);
  });
});

describe("previewBlocker", () => {
  test("nothing blocks an idle Handy with an overlay", () => {
    expect(previewBlocker(false, "live")).toBeNull();
    expect(previewBlocker(false, "minimal")).toBeNull();
  });

  test("a real recording blocks the preview", () => {
    expect(previewBlocker(true, "live")).toBe("recording");
  });

  test("an overlay turned off has nothing to show", () => {
    expect(previewBlocker(false, "none")).toBe("overlayOff");
  });

  test("recording outranks every other reason, as it does in Rust", () => {
    expect(previewBlocker(true, "none")).toBe("recording");
  });
});

describe("reducePreview", () => {
  test("start runs the preview and sends the state it is pinned to", () => {
    const { mode, call } = reducePreview(
      { running: false, state: "transcribing" },
      { kind: "start" },
    );
    expect(mode).toEqual({ running: true, state: "transcribing" });
    expect(call).toBe("start");
  });

  test("starting an already-running preview sends nothing", () => {
    expect(reducePreview(running(), { kind: "start" }).call).toBe("none");
  });

  test("stop ends it and hides the overlay", () => {
    const { mode, call } = reducePreview(running(), { kind: "stop" });
    expect(mode.running).toBe(false);
    expect(call).toBe("stop");
  });

  test("stopping when nothing runs sends nothing", () => {
    expect(reducePreview(IDLE_PREVIEW, { kind: "stop" }).call).toBe("none");
  });

  test("pinning a chip while running switches the overlay on screen", () => {
    const { mode, call } = reducePreview(running(), {
      kind: "pin",
      state: "processing",
    });
    expect(mode).toEqual({ running: true, state: "processing" });
    expect(call).toBe("setState");
  });

  test("pinning while stopped only remembers the choice", () => {
    const { mode, call } = reducePreview(IDLE_PREVIEW, {
      kind: "pin",
      state: "arming",
    });
    expect(mode).toEqual({ running: false, state: "arming" });
    expect(call).toBe("none");
    // ... and the next start carries it.
    expect(reducePreview(mode, { kind: "start" })).toEqual({
      mode: { running: true, state: "arming" },
      call: "start",
    });
  });

  test("re-pinning the chip already showing sends nothing", () => {
    expect(
      reducePreview(running("transcribing"), {
        kind: "pin",
        state: "transcribing",
      }).call,
    ).toBe("none");
  });

  test("a style change that keeps the pinned chip changes nothing", () => {
    expect(
      reducePreview(running("transcribing"), {
        kind: "restyle",
        style: "minimal",
      }),
    ).toEqual({ mode: running("transcribing"), call: "none" });
  });

  test("a style change that drops the pinned chip falls back to the loop", () => {
    // Live's `listening` does not exist under Minimal.
    const { mode, call } = reducePreview(running("listening"), {
      kind: "restyle",
      style: "minimal",
    });
    expect(mode).toEqual({ running: true, state: "cycle" });
    expect(call).toBe("setState");
  });

  test("turning the overlay off stops a running preview", () => {
    const { mode, call } = reducePreview(running("listening"), {
      kind: "restyle",
      style: "none",
    });
    expect(mode).toEqual(IDLE_PREVIEW);
    expect(call).toBe("stop");
  });

  test("turning the overlay off while stopped sends nothing", () => {
    expect(
      reducePreview(IDLE_PREVIEW, { kind: "restyle", style: "none" }).call,
    ).toBe("none");
  });

  test("a real recording ends the preview without a call", () => {
    // The backend already stopped driving, and deliberately left the overlay
    // to the recording that took it — telling it to stop would hide a session
    // the user actually started.
    const { mode, call } = reducePreview(running("listening"), {
      kind: "preempted",
    });
    expect(mode.running).toBe(false);
    expect(call).toBe("none");
  });

  test("leaving the tab stops the preview", () => {
    const { mode, call } = reducePreview(running(), { kind: "leave" });
    expect(mode.running).toBe(false);
    expect(call).toBe("stop");
  });

  test("leaving a tab that never started one sends nothing", () => {
    expect(reducePreview(IDLE_PREVIEW, { kind: "leave" }).call).toBe("none");
  });

  test("start, pin, pin, stop leaves nothing running", () => {
    let mode = IDLE_PREVIEW;
    const calls: string[] = [];
    for (const action of [
      { kind: "start" } as const,
      { kind: "pin", state: "arming" } as const,
      { kind: "pin", state: "transcribing" } as const,
      { kind: "stop" } as const,
    ]) {
      const next = reducePreview(mode, action);
      mode = next.mode;
      calls.push(next.call);
    }
    expect(calls).toEqual(["start", "setState", "setState", "stop"]);
    expect(mode).toEqual({ running: false, state: "transcribing" });
  });
});
