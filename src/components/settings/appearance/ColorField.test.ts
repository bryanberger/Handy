import { describe, expect, test } from "bun:test";
import { normalizeHexInput } from "./ColorField";

/**
 * Unit tests for the hex field's parsing rule. The token contract's colors
 * are `#RRGGBB` only — no alpha, no shorthand, no named colors (that leniency
 * is the theme file's, not the settings window's own text field).
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
