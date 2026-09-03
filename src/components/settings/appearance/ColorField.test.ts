import { describe, expect, test } from "bun:test";
import {
  normalizeHexInput,
  subscribeColorCommit,
  type ColorCommitTarget,
} from "./ColorField";

/**
 * Unit tests for the hex field's parsing rule. Per ticket 02 §1, colors are
 * `#RRGGBB` only — no alpha, no shorthand, no named colors (that leniency is
 * the theme file's, not the settings window's own text field).
 */

describe("normalizeHexInput", () => {
  test("accepts a canonical 6-digit hex with #", () => {
    expect(normalizeHexInput("#7aa2f7")).toBe("#7aa2f7");
  });

  test("accepts a missing #", () => {
    expect(normalizeHexInput("7aa2f7")).toBe("#7aa2f7");
  });

  test("normalizes case to lowercase", () => {
    expect(normalizeHexInput("#7AA2F7")).toBe("#7aa2f7");
    expect(normalizeHexInput("ABCDEF")).toBe("#abcdef");
  });

  test("trims surrounding whitespace", () => {
    expect(normalizeHexInput("  #123456  ")).toBe("#123456");
  });

  test("rejects a 3-digit shorthand — no CSS-style expansion in this field", () => {
    expect(normalizeHexInput("#abc")).toBeNull();
  });

  test("rejects 8-digit (alpha) hex — surface_opacity is its own token", () => {
    expect(normalizeHexInput("#7aa2f7ff")).toBeNull();
  });

  test("rejects a named CSS color", () => {
    expect(normalizeHexInput("red")).toBeNull();
  });

  test("rejects partial input while typing, e.g. a lone '#'", () => {
    expect(normalizeHexInput("#f")).toBeNull();
  });

  test("rejects non-hex characters", () => {
    expect(normalizeHexInput("#gggggg")).toBeNull();
  });

  test("rejects an empty string", () => {
    expect(normalizeHexInput("")).toBeNull();
  });
});

/** A stand-in for the native `<input type="color">`: records what was
 *  subscribed and lets a test fire either event by name. */
function fakePicker(value = "#000000") {
  const listeners = new Map<string, Set<() => void>>();
  const target: ColorCommitTarget = {
    value,
    addEventListener(type, listener) {
      const set = listeners.get(type) ?? new Set();
      set.add(listener);
      listeners.set(type, set);
    },
    removeEventListener(type, listener) {
      listeners.get(type)?.delete(listener);
    },
  };
  const fire = (type: string) =>
    listeners.get(type)?.forEach((listener) => listener());
  const count = (type: string) => listeners.get(type)?.size ?? 0;
  return { target, fire, count };
}

/**
 * The commit rule for the swatch. React maps `onChange` for a color input onto
 * the native `input` event, which the OS picker fires on every frame of a
 * drag; committing there sent one `change_overlay_theme_setting` per frame.
 * Only the native `change` event — fired once, when the picker closes —
 * commits.
 */
describe("subscribeColorCommit", () => {
  test("a drag inside the picker (input events) never commits", () => {
    const picker = fakePicker();
    const committed: string[] = [];
    subscribeColorCommit(picker.target, (value) => committed.push(value));

    for (let i = 0; i < 20; i++) picker.fire("input");

    expect(committed).toEqual([]);
  });

  test("closing the picker (one change event) commits once, with the value", () => {
    const picker = fakePicker("#7aa2f7");
    const committed: string[] = [];
    subscribeColorCommit(picker.target, (value) => committed.push(value));

    picker.fire("input");
    picker.fire("change");

    expect(committed).toEqual(["#7aa2f7"]);
  });

  test("unsubscribing removes the listener, so a later change is ignored", () => {
    const picker = fakePicker("#7aa2f7");
    const committed: string[] = [];
    const unsubscribe = subscribeColorCommit(picker.target, (value) =>
      committed.push(value),
    );
    expect(picker.count("change")).toBe(1);

    unsubscribe();
    picker.fire("change");

    expect(picker.count("change")).toBe(0);
    expect(committed).toEqual([]);
  });
});
