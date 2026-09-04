import { describe, expect, test } from "bun:test";
import type { PreviewState } from "@/bindings";
import {
  answerPreviewRequest,
  autoStartFor,
  IDLE_PREVIEW,
  overlayAcceptsDrafts,
  previewBlocker,
  previewChipsFor,
  reducePreview,
  type PreviewAutoStartState,
  type PreviewChangeRequest,
  type PreviewMode,
} from "./previewMode";

/**
 * Unit tests for preview mode's state machine, the tab's half of the
 * contract. The backend enforces the same rules for itself (a preview refuses
 * to start while recording, ends when a real recording takes the overlay);
 * these pin what the tab does about them.
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
    // to the recording that took it. Telling it to stop would hide a session
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

/** Nothing in the way: no preview on screen, an overlay to show it on, no
 *  recording, and a Mac that can actually draw glass right now. */
const IDLE_TAB: PreviewAutoStartState = {
  running: false,
  style: "live",
  isRecording: false,
  glassAvailable: true,
};

describe("autoStartFor", () => {
  const idle = IDLE_TAB;

  test("picking Glass puts the overlay on screen by itself", () => {
    expect(autoStartFor({ kind: "material", to: "glass" }, idle)).toEqual({
      kind: "autoStart",
    });
  });

  test("changing the Glass style does too", () => {
    expect(autoStartFor({ kind: "glassStyle" }, idle)).toEqual({
      kind: "autoStart",
    });
  });

  /** Flat is what the overlay looks like everywhere else already; there is
   *  nothing new to show, and starting a preview for it would be a window
   *  appearing in answer to "make it plain". */
  test("picking Flat shows nothing", () => {
    expect(autoStartFor({ kind: "material", to: "flat" }, idle)).toBeNull();
  });

  test("a preview already on screen is left alone", () => {
    expect(
      autoStartFor(
        { kind: "material", to: "glass" },
        { ...idle, running: true },
      ),
    ).toBeNull();
  });

  /** The same refusals the Start button obeys. What may not be started by
   *  hand may not be started on the user's behalf. */
  test("it refuses for every reason the button refuses", () => {
    expect(
      autoStartFor(
        { kind: "material", to: "glass" },
        { ...idle, isRecording: true },
      ),
    ).toBeNull();
    expect(
      autoStartFor({ kind: "glassStyle" }, { ...idle, style: "none" }),
    ).toBeNull();
  });

  /** Glass the machine supports but cannot draw right now renders Flat, so
   *  starting a preview would answer "show me glass" with the card already
   *  there. macOS Reduce Transparency is the case that prompted this. */
  test("Glass that would not actually render shows nothing", () => {
    expect(
      autoStartFor(
        { kind: "material", to: "glass" },
        {
          running: false,
          style: "live",
          isRecording: false,
          glassAvailable: false,
        },
      ),
    ).toBeNull();
    expect(
      autoStartFor(
        { kind: "glassStyle" },
        {
          running: false,
          style: "minimal",
          isRecording: false,
          glassAvailable: false,
        },
      ),
    ).toBeNull();
  });
});

describe("answerPreviewRequest", () => {
  const glass: PreviewChangeRequest = {
    change: { kind: "material", to: "glass" },
    seq: 1,
  };

  test("no request is nothing to answer", () => {
    expect(answerPreviewRequest(null, 0, IDLE_TAB)).toBeNull();
    expect(answerPreviewRequest(undefined, 0, IDLE_TAB)).toBeNull();
  });

  test("a fresh request is answered, and names the seq to remember", () => {
    expect(answerPreviewRequest(glass, 0, IDLE_TAB)).toEqual({
      seq: 1,
      action: { kind: "autoStart" },
    });
  });

  test("a request already answered is never answered again", () => {
    expect(answerPreviewRequest(glass, glass.seq, IDLE_TAB)).toBeNull();
    // Not even once the state around it moves. That is what keeps a later
    // style, recording or availability change from re-firing an answered pick.
    expect(
      answerPreviewRequest(glass, glass.seq, {
        ...IDLE_TAB,
        isRecording: true,
      }),
    ).toBeNull();
  });

  test("an answer of 'leave the screen alone' still counts as answered", () => {
    // Picking Flat starts nothing, but the pick was dealt with, so the next
    // render must not reconsider it.
    const flat: PreviewChangeRequest = {
      change: { kind: "material", to: "flat" },
      seq: 2,
    };
    expect(answerPreviewRequest(flat, 1, IDLE_TAB)).toEqual({
      seq: 2,
      action: null,
    });
    expect(answerPreviewRequest(flat, 2, IDLE_TAB)).toBeNull();
  });

  test("two identical picks in a row are two answers", () => {
    const first: PreviewChangeRequest = {
      change: { kind: "glassStyle" },
      seq: 1,
    };
    const second: PreviewChangeRequest = {
      change: { kind: "glassStyle" },
      seq: 2,
    };
    expect(answerPreviewRequest(first, 0, IDLE_TAB)).toEqual({
      seq: 1,
      action: { kind: "autoStart" },
    });
    expect(answerPreviewRequest(second, 1, IDLE_TAB)).toEqual({
      seq: 2,
      action: { kind: "autoStart" },
    });
  });
});

describe("the auto-start transition", () => {
  test("it starts the loop, whatever chip was remembered", () => {
    const { mode, call } = reducePreview(
      { running: false, state: "transcribing" },
      { kind: "autoStart" },
    );
    expect(mode).toEqual({ running: true, state: "cycle" });
    expect(call).toBe("start");
  });

  test("it sends nothing to a preview that is already running", () => {
    const { mode, call } = reducePreview(running("listening"), {
      kind: "autoStart",
    });
    expect(mode).toEqual(running("listening"));
    expect(call).toBe("none");
  });

  test("Stop is still the way out of one it started", () => {
    const started = reducePreview(IDLE_PREVIEW, { kind: "autoStart" }).mode;
    expect(reducePreview(started, { kind: "stop" })).toEqual({
      mode: { running: false, state: "cycle" },
      call: "stop",
    });
  });
});

describe("overlayAcceptsDrafts", () => {
  test("only a running preview with nothing recording may be repainted live", () => {
    expect(overlayAcceptsDrafts(running(), false)).toBe(true);
    // Pre-empted: the tab still thinks its preview is running (it learns
    // otherwise from the next poll), and the card belongs to the recording.
    expect(overlayAcceptsDrafts(running(), true)).toBe(false);
    // Stopped: there is nothing of the tab's on screen to paint, so a drag
    // must not send one IPC message per frame for the backend to refuse.
    expect(overlayAcceptsDrafts(IDLE_PREVIEW, false)).toBe(false);
    expect(overlayAcceptsDrafts(IDLE_PREVIEW, true)).toBe(false);
  });

  test("the pinned state has no say in it", () => {
    for (const state of previewChipsFor("live")) {
      expect(overlayAcceptsDrafts(running(state), false)).toBe(true);
    }
  });
});
