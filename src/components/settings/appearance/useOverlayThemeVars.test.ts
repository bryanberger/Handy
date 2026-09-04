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
 * Unit tests for `mergeDraft`, the rule a token row is painted by:
 * `draft[key] ?? resolved.theme[key]`. The theme file is the theme, so a draft
 * is the value on its way into that file, and the on-screen preview shows it
 * before it gets there, managed file or not.
 */

describe("mergeDraft", () => {
  test("a draft overrides the persisted value", () => {
    const theme: OverlayTheme = { ...INHERIT_ALL, radius: 24 };
    const merged = mergeDraft(theme, { radius: 8 });
    expect(merged.radius).toBe(8);
  });

  test("keys absent from the draft keep the resolved value", () => {
    const theme: OverlayTheme = {
      ...INHERIT_ALL,
      surface: "#111111",
      text: "#eeeeee",
    };
    const merged = mergeDraft(theme, { radius: 12 });
    expect(merged.surface).toBe("#111111");
    expect(merged.text).toBe("#eeeeee");
    expect(merged.radius).toBe(12);
  });

  test("a draft value of null (an in-progress reset) still overrides", () => {
    const theme: OverlayTheme = { ...INHERIT_ALL, padding: 14 };
    const merged = mergeDraft(theme, { padding: null });
    expect(merged.padding).toBeNull();
  });

  test("a managed file locks the rows, not the preview", () => {
    // Contract point 4: Handy never writes this file, and the preview still
    // paints what the user is dragging. Nothing here knows about ownership.
    const theme: OverlayTheme = {
      ...INHERIT_ALL,
      accent: "#7aa2f7",
      radius: 24,
    };
    const merged = mergeDraft(theme, { accent: "#ff0000", radius: 4 });
    expect(merged.accent).toBe("#ff0000");
    expect(merged.radius).toBe(4);
  });

  test("does not mutate its inputs", () => {
    const theme: OverlayTheme = { ...INHERIT_ALL, radius: 24 };
    const draft = { radius: 8 };
    mergeDraft(theme, draft);
    expect(theme.radius).toBe(24);
    expect(draft.radius).toBe(8);
  });
});

/**
 * `parseComputedColor` reads `getComputedStyle(probe).color` back to a hex
 * string for the "resolved default" display. Regression coverage for two real
 * bugs found by screenshotting the running app; neither shows up in a
 * type-check or a jsdom-free unit test. Current WebKit serializes an opaque
 * color as legacy comma syntax, switches to CSS Color 4 space syntax once alpha
 * is present, and switches again to `color(srgb ...)` for an srgb `color-mix()`
 * with alpha. That last form carries 0-1 fractional channels, not 0-255 integers.
 * `surface`'s derivation (`color-mix(in srgb, <surface> <alpha>%, transparent)`)
 * always has alpha, so it hit the one format a comma-only, then a comma-or-space,
 * regex both missed, silently falling back to a hardcoded "#000000", while
 * `accent`/`text` (opaque) worked either way by accident.
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
    // color-mixed toward transparent), captured from the running app rather
    // than invented, so a future WebKit format change is caught here first.
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
    // The dangerous case. Reading the digits and dropping the "%" turns white
    // into near-black. WebKit has not been seen to emit this for `.color`, but
    // it is legal CSS, and a wrong color here is invisible, showing only as the
    // token's "resolved default".
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
 * `sameStringMap` keeps the colour readback (the probes here and the theme vars
 * they read through) from re-firing and setting state when nothing changed.
 *
 * Regression coverage for a real crash. Dragging a Size & Spacing slider
 * blanked the Appearance tab with React's "Maximum update depth exceeded". Both
 * the memo key and the effect payload were compared by object identity. React
 * may hand back a fresh-but-equal `useState` value (a functional updater
 * re-runs on a re-render, twice per render under `StrictMode`), so `draft` and
 * everything memoized on it got a new reference every render while holding the
 * same tokens. The effect re-ran and set state on every commit, each leaving
 * another sync update pending until React gave up. Identity is not a proxy for
 * equality here, so these tests use two distinct objects.
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
    // A length-only check would call these equal; both maps are ones the apply
    // layer really emits (an accent-only theme vs a scale-only one).
    expect(
      sameStringMap({ "--s-accent": "#fff" }, { "--ov-scale": "#fff" }),
    ).toBe(false);
  });

  test("two empty maps are equal", () => {
    expect(sameStringMap({}, {})).toBe(true);
  });

  test("re-resolving the same tokens yields an equal var map", () => {
    // The invariant the crash violated, at the seam it is enforced on. A draft
    // rebuilt with identical tokens must not look like a change to the preview,
    // however often React re-derives it.
    const withScale = (size_scale: number): ResolvedOverlayTheme => ({
      theme: { ...INHERIT_ALL, accent: "#b18cfe", size_scale },
      effective_material: "flat",
      shadow_edge_slack: 15,
      glass_support: { supported: true, available: true, engine: "liquid" },
      file: EMPTY_FILE_STATE,
      watching: true,
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
