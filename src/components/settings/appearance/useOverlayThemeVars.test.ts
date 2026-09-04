import { describe, expect, test } from "bun:test";
import type { OverlayTheme, ResolvedOverlayTheme } from "@/bindings";
import { INHERIT_ALL, resolveOverlayThemeVars } from "@/lib/overlayTheme";
import {
  EMPTY_FILE_STATE,
  mergeDraft,
  parseComputedColor,
  sameStringMap,
} from "./useOverlayThemeVars";

/**
 * Unit tests for `mergeDraft`, which implements the file-owned locking rule:
 * `owned_keys.includes(key) ? resolved.theme[key] : (draft[key] ??
 * resolved.theme[key])`. A settings-level draft must never outrank a token
 * the theme file owns.
 */

describe("mergeDraft", () => {
  test("an unowned key takes the draft value", () => {
    const theme: OverlayTheme = { ...INHERIT_ALL, radius: 24 };
    const merged = mergeDraft(theme, { radius: 8 }, []);
    expect(merged.radius).toBe(8);
  });

  test("a file-owned key ignores the draft entirely", () => {
    const theme: OverlayTheme = { ...INHERIT_ALL, accent: "#7aa2f7" };
    const merged = mergeDraft(theme, { accent: "#ff0000" }, ["accent"]);
    expect(merged.accent).toBe("#7aa2f7");
  });

  test("keys absent from the draft keep the resolved value, owned or not", () => {
    const theme: OverlayTheme = {
      ...INHERIT_ALL,
      surface: "#111111",
      text: "#eeeeee",
    };
    const merged = mergeDraft(theme, { radius: 12 }, ["surface"]);
    expect(merged.surface).toBe("#111111");
    expect(merged.text).toBe("#eeeeee");
    expect(merged.radius).toBe(12);
  });

  test("a draft value of null (an in-progress reset) still overrides an unowned key", () => {
    const theme: OverlayTheme = { ...INHERIT_ALL, padding: 14 };
    const merged = mergeDraft(theme, { padding: null }, []);
    expect(merged.padding).toBeNull();
  });

  test("locking is per key: other tokens in the same draft still apply", () => {
    const theme: OverlayTheme = {
      ...INHERIT_ALL,
      accent: "#7aa2f7",
      radius: 24,
    };
    const merged = mergeDraft(theme, { accent: "#ff0000", radius: 4 }, [
      "accent",
    ]);
    expect(merged.accent).toBe("#7aa2f7"); // locked: draft ignored
    expect(merged.radius).toBe(4); // not locked: draft applies
  });

  test("does not mutate its inputs", () => {
    const theme: OverlayTheme = { ...INHERIT_ALL, radius: 24 };
    const draft = { radius: 8 };
    mergeDraft(theme, draft, []);
    expect(theme.radius).toBe(24);
    expect(draft.radius).toBe(8);
  });
});

/**
 * `parseComputedColor` reads `getComputedStyle(probe).color` back to a hex
 * string for the "resolved default" display. Regression coverage for two real
 * bugs found by screenshotting the actual running app (neither is visible
 * from a type-check or a jsdom-free unit test alone): current WebKit
 * serializes an *opaque* color as legacy comma syntax, switches to CSS
 * Color 4 space syntax the moment alpha is present, and switches again to
 * the `color(srgb ...)` function — with 0-1 *fractional* channels instead of
 * 0-255 integers — for a `color-mix()` evaluated in the srgb color space
 * once alpha is involved. `surface`'s derivation
 * (`color-mix(in srgb, <surface> <alpha>%, transparent)`) always has alpha,
 * so it exercised the one format a comma-only, then a comma-or-space, regex
 * both missed — silently falling back to a hardcoded "#000000" — while
 * `accent`/`text` (opaque) worked by accident either way.
 */
describe("parseComputedColor", () => {
  test("parses legacy comma syntax (opaque colors)", () => {
    expect(parseComputedColor("rgb(242, 140, 187)")).toBe("#f28cbb");
  });

  test("parses legacy comma syntax with alpha (rgba)", () => {
    expect(parseComputedColor("rgba(44, 43, 41, 0.98)")).toBe("#2c2b29");
  });

  test("parses CSS Color 4 space syntax with alpha", () => {
    expect(parseComputedColor("rgb(44 43 41 / 0.98)")).toBe("#2c2b29");
  });

  test("parses CSS Color 4 space syntax without alpha", () => {
    expect(parseComputedColor("rgb(251 251 251)")).toBe("#fbfbfb");
  });

  test("rounds fractional channel values (rgb form)", () => {
    expect(parseComputedColor("rgb(44.4 42.6 40.5 / 1)")).toBe("#2c2b29");
  });

  test("parses the color(srgb ...) function with 0-1 fractional channels", () => {
    // The exact string observed from a live `getComputedStyle(probe).color`
    // read for the dark theme's --s-surface default (surface_opacity 0.98
    // color-mixed toward transparent): captured from the running app, not
    // invented, so a future WebKit format change is caught here first.
    expect(parseComputedColor("color(srgb 0.172549 0.168627 0.160784)")).toBe(
      "#2c2b29",
    );
  });

  test("parses color(srgb ...) with an explicit alpha component", () => {
    expect(parseComputedColor("color(srgb 0.94902 0.54902 0.733333 / 1)")).toBe(
      "#f28cbb",
    );
  });

  test("returns null for percentage channels rather than mis-reading them", () => {
    // The dangerous case: reading the digits and dropping the "%" turns
    // white into near-black. Not a format WebKit has been seen to emit for
    // `.color`, but it is legal CSS, and a wrong color here is invisible —
    // it just shows as the token's "resolved default".
    expect(parseComputedColor("rgb(100%, 50%, 25%)")).toBeNull();
    expect(parseComputedColor("rgb(100% 50% 25% / 0.5)")).toBeNull();
    expect(parseComputedColor("color(srgb 100% 50% 25%)")).toBeNull();
  });

  test("returns null for an unparseable value", () => {
    expect(parseComputedColor("transparent")).toBeNull();
    expect(parseComputedColor("")).toBeNull();
    expect(parseComputedColor("lab(52% 40 59)")).toBeNull();
  });
});

/**
 * `sameStringMap` is the guard that keeps the tab's colour readback — the
 * probes here, and the theme vars they are read through — from re-firing (and
 * setting state) on a render where nothing actually changed.
 *
 * Regression coverage for a real crash: dragging a Size & Spacing slider blanked
 * the Appearance tab with React's "Maximum update depth exceeded". The cause was
 * that both the memo key and the effect payload were compared by object
 * *identity*. React is free to hand back a fresh-but-equal `useState` value (it
 * re-runs a functional updater on a re-render, and twice per render under
 * `StrictMode`), so `draft` — and everything memoized on it — got a new
 * reference every render while holding the very same tokens; the effect then
 * re-ran and set state on every commit, and each commit left another sync update
 * pending until React gave up. Identity is not a proxy for equality here, so
 * these tests are all about two *distinct* objects.
 */
describe("sameStringMap", () => {
  test("two distinct objects with the same entries are equal", () => {
    const a = { accent: "#b18cfe", surface: "#000000", text: null };
    const b = { accent: "#b18cfe", surface: "#000000", text: null };
    expect(a).not.toBe(b);
    expect(sameStringMap(a, b)).toBe(true);
  });

  test("a changed value is not equal", () => {
    expect(sameStringMap({ accent: "#b18cfe" }, { accent: "#b18cff" })).toBe(
      false,
    );
  });

  test("null and a string are distinguished in both directions", () => {
    expect(sameStringMap({ text: null }, { text: "#000000" })).toBe(false);
    expect(sameStringMap({ text: "#000000" }, { text: null })).toBe(false);
  });

  test("a key added or removed is not equal", () => {
    expect(sameStringMap({ a: "1" }, { a: "1", b: "2" })).toBe(false);
    expect(sameStringMap({ a: "1", b: "2" }, { a: "1" })).toBe(false);
  });

  test("same key count with different key names is not equal", () => {
    // A length-only check would call these equal; both maps here are ones the
    // apply layer really emits (an accent-only theme vs a scale-only one).
    expect(
      sameStringMap({ "--s-accent": "#fff" }, { "--ov-scale": "#fff" }),
    ).toBe(false);
  });

  test("two empty maps are equal", () => {
    expect(sameStringMap({}, {})).toBe(true);
  });

  test("re-resolving the same tokens yields an equal var map", () => {
    // The invariant the crash violated, at the seam it is enforced on: a draft
    // object rebuilt with identical tokens must not look like a change to the
    // preview, however many times React re-derives it.
    const withScale = (size_scale: number): ResolvedOverlayTheme => ({
      theme: { ...INHERIT_ALL, accent: "#b18cfe", size_scale },
      effective_material: "flat",
      glass_support: { supported: true, available: true, engine: "liquid" },
      file: EMPTY_FILE_STATE,
    });
    const first = resolveOverlayThemeVars(withScale(0.85));
    const second = resolveOverlayThemeVars(withScale(0.85));
    expect(first).not.toBe(second);
    expect(sameStringMap(first, second)).toBe(true);

    // ... and a token that really moved must still read as a change.
    expect(sameStringMap(first, resolveOverlayThemeVars(withScale(0.9)))).toBe(
      false,
    );
  });
});
