import { describe, expect, test } from "bun:test";
import { DraftDebouncer } from "./useDraftSetting";

/**
 * Unit tests for `DraftDebouncer`, the pure engine behind `useDraftSetting`.
 * Uses real (short) timers rather than a fake-timer library, so these assert
 * the actual debounce behaviour rather than a simulation of it.
 */

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
      30, // a short debounce so the test stays fast
    );

    debouncer.schedule("radius", 10);
    debouncer.schedule("radius", 20);
    debouncer.schedule("radius", 30);

    // Nothing commits synchronously, and nothing before the window elapses.
    expect(commits).toEqual([]);
    expect(debouncer.isPending("radius")).toBe(true);

    await sleep(60);

    expect(commits).toEqual([["radius", 30]]);
    expect(settled).toEqual(["radius"]);
    expect(debouncer.isPending("radius")).toBe(false);
  });

  test("flush() commits immediately without waiting for the debounce", async () => {
    const commits: Array<[string, number]> = [];
    const debouncer = new DraftDebouncer<string, number>(
      (key, value) => {
        commits.push([key, value]);
      },
      () => {},
      10_000, // long enough that only an explicit flush could fire in time
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
    // The first commit for "accent" takes a while to resolve (e.g. a slow
    // IPC round trip); a second edit is scheduled and flushed before it
    // finishes. onSettled must fire once per generation, and in an order
    // that never marks the *newer* edit settled on behalf of the old one.
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
    await sleep(20); // the first debounce fires and starts its slow commit

    debouncer.schedule("accent", "#222222");
    await debouncer.flush("accent"); // the second commit resolves immediately

    expect(settled).toEqual(["accent"]); // only the second generation settled

    resolveFirst?.();
    await sleep(0);

    // The stale first commit resolving afterwards must not add a second,
    // misleading "settled" for a generation that is no longer current.
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
