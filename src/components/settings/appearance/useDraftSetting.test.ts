import { describe, expect, test } from "bun:test";
import { DraftEngine, DraftDebouncer, FrameCoalescer } from "./useDraftSetting";
import { INHERIT_ALL } from "@/lib/overlayTheme";
import type { OverlayTheme } from "@/bindings";

/** A scheduler whose frames only happen when the test says so. */
function manualFrames() {
  const queue: Array<() => void> = [];
  return {
    schedule: (run: () => void) => {
      queue.push(run);
    },
    /** Run every callback queued so far (a frame that has come round). */
    frame: () => queue.splice(0, queue.length).forEach((run) => run()),
    get queued() {
      return queue.length;
    },
  };
}

/** Unit tests for `DraftDebouncer`, the pure engine behind `useDraftSetting`.
 *  Real (short) timers, not a fake-timer library, so the debounce is real. */

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

describe("DraftDebouncer", () => {
  test("a burst of schedule() calls inside one window commits only the last value, once", async () => {
    const commits: Array<[string, number]> = [];
    const settled: string[] = [];
    const debouncer = new DraftDebouncer<string, number>(
      (key, value) => {
        commits.push([key, value]);
      },
      (key) => settled.push(key),
      30, // short debounce, so the test stays fast
    );

    debouncer.schedule("radius", 10);
    debouncer.schedule("radius", 20);
    debouncer.schedule("radius", 30);

    // Nothing commits synchronously, nor before the window elapses.
    expect(commits).toEqual([]);
    expect(debouncer.isPending("radius")).toBe(true);

    await sleep(60);

    expect(commits).toEqual([["radius", 30]]);
    expect(settled).toEqual(["radius"]);
    expect(debouncer.isPending("radius")).toBe(false);
  });

  test("a whole color-picker drag collapses to one commit", async () => {
    // The color swatch's rule, pinned where it lives. macOS's color panel is
    // continuous and WebKit raises a form-control change event per update, so
    // the swatch feeds the draft ~60 times a second and must never commit from
    // that event. Sixty frames of a one-second drag land as one write.
    const commits: Array<[string, string]> = [];
    const debouncer = new DraftDebouncer<string, string>(
      (key, value) => {
        commits.push([key, value]);
      },
      () => {},
      30,
    );

    for (let frame = 0; frame < 60; frame++) {
      debouncer.schedule(
        "accent",
        `#0000${frame.toString(16).padStart(2, "0")}`,
      );
      await sleep(1);
    }
    await sleep(60);

    expect(commits).toEqual([["accent", "#00003b"]]);
  });

  test("flush() commits immediately without waiting for the debounce", async () => {
    const commits: Array<[string, number]> = [];
    const debouncer = new DraftDebouncer<string, number>(
      (key, value) => {
        commits.push([key, value]);
      },
      () => {},
      10_000, // only an explicit flush can fire in time
    );

    debouncer.schedule("padding", 14);
    await debouncer.flush("padding");

    expect(commits).toEqual([["padding", 14]]);
    expect(debouncer.isPending("padding")).toBe(false);
  });

  test("flush() on a key with nothing pending is a no-op", async () => {
    const commits: unknown[] = [];
    const debouncer = new DraftDebouncer<string, number>(
      (key, value) => {
        commits.push([key, value]);
      },
      () => {},
    );

    await debouncer.flush("waveform_gap");
    expect(commits).toEqual([]);
  });

  test("independent keys debounce independently", async () => {
    const commits: Array<[string, number]> = [];
    const debouncer = new DraftDebouncer<string, number>(
      (key, value) => {
        commits.push([key, value]);
      },
      () => {},
      30,
    );

    debouncer.schedule("radius", 1);
    debouncer.schedule("padding", 2);
    await sleep(60);

    expect(commits.sort()).toEqual([
      ["padding", 2],
      ["radius", 1],
    ]);
  });

  test("a late-resolving commit cannot clear a newer edit's pending state", async () => {
    // The first commit for "accent" resolves slowly (a slow IPC round trip)
    // while a second edit is scheduled and flushed. onSettled must fire once
    // per generation, never marking the newer edit settled for the old one.
    let resolveFirst: (() => void) | undefined;
    const settled: string[] = [];
    let callCount = 0;
    const debouncer = new DraftDebouncer<string, string>(
      (_key, _value) => {
        callCount += 1;
        if (callCount === 1) {
          return new Promise<void>((resolve) => {
            resolveFirst = resolve;
          });
        }
        return Promise.resolve();
      },
      (key) => settled.push(key),
      10,
    );

    debouncer.schedule("accent", "#111111");
    await sleep(20); // the first debounce fires, starting its slow commit

    debouncer.schedule("accent", "#222222");
    await debouncer.flush("accent"); // the second commit resolves at once

    expect(settled).toEqual(["accent"]); // only the second generation settled

    resolveFirst?.();
    await sleep(0);

    // The stale commit must not add a second "settled" for an old generation.
    expect(settled).toEqual(["accent"]);
  });

  test("cancel() drops a pending debounce without committing it", async () => {
    const commits: unknown[] = [];
    const debouncer = new DraftDebouncer<string, number>(
      (key, value) => {
        commits.push([key, value]);
      },
      () => {},
      20,
    );

    debouncer.schedule("size_scale", 1.2);
    debouncer.cancel("size_scale");
    await sleep(40);

    expect(commits).toEqual([]);
    expect(debouncer.isPending("size_scale")).toBe(false);
  });

  test("flushAll() commits every pending key and none that aren't pending", async () => {
    const commits: Array<[string, number]> = [];
    const debouncer = new DraftDebouncer<string, number>(
      (key, value) => {
        commits.push([key, value]);
      },
      () => {},
      10_000,
    );

    debouncer.schedule("radius", 8);
    debouncer.schedule("padding", 6);
    await debouncer.flushAll();

    expect(commits.sort()).toEqual([
      ["padding", 6],
      ["radius", 8],
    ]);
  });
});

/** Unit tests for `FrameCoalescer`, the live-preview half of `useDraftSetting`.
 *  The frame scheduler is injected, so these assert the coalescing rule, not
 *  a browser's timing. */
describe("FrameCoalescer", () => {
  test("the first push after a quiet moment goes out immediately", () => {
    const frames = manualFrames();
    const sent: number[] = [];
    const coalescer = new FrameCoalescer<number>(
      (v) => sent.push(v),
      frames.schedule,
    );

    coalescer.push(1);
    expect(sent).toEqual([1]);
  });

  test("a burst inside one frame collapses to the last value", () => {
    const frames = manualFrames();
    const sent: number[] = [];
    const coalescer = new FrameCoalescer<number>(
      (v) => sent.push(v),
      frames.schedule,
    );

    coalescer.push(1);
    coalescer.push(2);
    coalescer.push(3);
    expect(sent).toEqual([1]);

    frames.frame();
    expect(sent).toEqual([1, 3]);
  });

  test("a frame with nothing new sends nothing and goes quiet", () => {
    const frames = manualFrames();
    const sent: number[] = [];
    const coalescer = new FrameCoalescer<number>(
      (v) => sent.push(v),
      frames.schedule,
    );

    coalescer.push(1);
    frames.frame();
    expect(sent).toEqual([1]);
    // Nothing more queued, so the next push is a leading edge again.
    expect(frames.queued).toBe(0);

    coalescer.push(2);
    expect(sent).toEqual([1, 2]);
  });

  test("a sustained stream delivers exactly once per frame", () => {
    const frames = manualFrames();
    const sent: number[] = [];
    const coalescer = new FrameCoalescer<number>(
      (v) => sent.push(v),
      frames.schedule,
    );

    for (let value = 0; value < 12; value++) {
      coalescer.push(value);
      // Three inputs per frame, the rate a 180 Hz control feeds a 60 Hz screen.
      if (value % 3 === 2) frames.frame();
    }
    expect(sent).toEqual([0, 2, 5, 8, 11]);
  });
});

describe("FrameCoalescer.flush and .cancel", () => {
  test("flush() delivers the held value at once, so the last frame is never dropped", () => {
    const frames = manualFrames();
    const sent: number[] = [];
    const coalescer = new FrameCoalescer<number>(
      (v) => sent.push(v),
      frames.schedule,
    );

    coalescer.push(1);
    coalescer.push(2); // held for the frame still to come round
    expect(sent).toEqual([1]);

    // A commit does this before storing, so the persisted 2 is painted last.
    coalescer.flush();
    expect(sent).toEqual([1, 2]);

    // The frame still arrives and finds nothing left to send.
    frames.frame();
    expect(sent).toEqual([1, 2]);
  });

  test("cancel() drops the held value and reopens the leading edge", () => {
    const frames = manualFrames();
    const sent: number[] = [];
    const coalescer = new FrameCoalescer<number>(
      (v) => sent.push(v),
      frames.schedule,
    );

    coalescer.push(1);
    coalescer.push(2);
    coalescer.cancel();

    // The abandoned value is gone, and the next push is immediate again, not
    // waiting on a frame that is no longer coming.
    coalescer.push(3);
    expect(sent).toEqual([1, 3]);

    // The pre-cancel frame delivers nothing and takes no new push's frame.
    frames.frame();
    expect(sent).toEqual([1, 3]);

    // Coalescing carries on. 4 is a leading edge (the frame above found
    // nothing and went quiet), 5 rides the frame after it.
    coalescer.push(4);
    coalescer.push(5);
    expect(sent).toEqual([1, 3, 4]);
    frames.frame();
    expect(sent).toEqual([1, 3, 4, 5]);
  });

  test("cancel() with nothing held is harmless", () => {
    const frames = manualFrames();
    const sent: number[] = [];
    const coalescer = new FrameCoalescer<number>(
      (v) => sent.push(v),
      frames.schedule,
    );

    coalescer.cancel();
    coalescer.push(1);
    expect(sent).toEqual([1]);
  });
});

/** Unit tests for `DraftEngine`, the ordering rules between the two clocks of
 *  live editing, with React, Tauri and the browser replaced by injected
 *  effects. */
describe("DraftEngine", () => {
  /** A painted theme as the tokens it sets, all these tests care about. */
  function painted(theme: OverlayTheme): Record<string, unknown> {
    return Object.fromEntries(
      Object.entries(theme).filter(([, value]) => value !== null),
    );
  }

  /** A store, an overlay and a display, all fake and observable. The log is
   *  one ordered list because ordering is what these tests are about. */
  function harness(options: { canPaint?: () => boolean } = {}) {
    const frames = manualFrames();
    const log: Array<
      { paint: Record<string, unknown> } | { commit: [string, unknown] }
    > = [];
    let stored: OverlayTheme = { ...INHERIT_ALL };

    const engine = new DraftEngine(
      {
        paint: (theme) => log.push({ paint: painted(theme) }),
        commit: (key, value) => {
          log.push({ commit: [key, value] });
          stored = { ...stored, [key]: value };
        },
        storedTheme: () => stored,
        onDraftChange: () => {},
        canPaint: options.canPaint ?? (() => true),
      },
      frames.schedule,
      10_000, // only an explicit flush fires; the drag is still in progress
    );

    return { engine, frames, log, storedTheme: () => stored };
  }

  test("a draft abandoned by a reset never lands: the overlay ends on the stored theme", () => {
    // The bug this pins. Reset Size Scale mid-drag, debounce still pending,
    // and the commit is `null` over an already-inherit token, so the backend
    // has nothing to store and the card kept whatever the finger last touched.
    const { engine, frames, log, storedTheme } = harness();

    engine.set("size_scale", 1.15); // leading edge, painted at once
    engine.set("size_scale", 1.2); // held for the frame still to come
    engine.reset("size_scale");
    frames.frame(); // the frame the abandoned 1.2 would have ridden

    expect(log).toEqual([
      { paint: { size_scale: 1.15 } },
      // The corrective frame, the theme the commit is about to store, painted
      // without waiting for the round trip. 1.2 never reaches the screen.
      { paint: {} },
      { commit: ["size_scale", null] },
    ]);
    // Nothing painted after the commit, and on screen is the store's inherit.
    expect(storedTheme().size_scale).toBe(null);
  });

  test("the value a commit stores is also the last one painted", () => {
    const { engine, log } = harness();

    engine.set("padding", 8);
    // No frame in between, so this one is only held and is committed next.
    engine.set("size_scale", 1.35);
    void engine.flush("size_scale");

    expect(log).toEqual([
      { paint: { padding: 8 } },
      { paint: { padding: 8, size_scale: 1.35 } },
      { commit: ["size_scale", 1.35] },
    ]);
  });

  test("with no preview of ours on screen nothing is painted, and edits still commit", () => {
    const { engine, log } = harness({ canPaint: () => false });

    engine.set("size_scale", 1.1);
    void engine.flush("size_scale");
    engine.reset("size_scale");

    expect(log).toEqual([
      { commit: ["size_scale", 1.1] },
      { commit: ["size_scale", null] },
    ]);
  });

  test("dispose() commits what is pending and lets no later frame paint", async () => {
    const { engine, frames, log } = harness();

    engine.set("radius", 12);
    engine.set("radius", 16); // held
    await engine.dispose();
    frames.frame();

    expect(log).toEqual([
      { paint: { radius: 12 } },
      { paint: { radius: 16 } }, // flushed from the coalescer by the commit
      { commit: ["radius", 16] },
    ]);
  });
});
