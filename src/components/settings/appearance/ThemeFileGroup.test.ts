import { describe, expect, test } from "bun:test";
import type { OverlayTheme } from "@/bindings";
import { INHERIT_ALL } from "@/lib/overlayTheme";
import {
  diagnosticI18nKey,
  lockedTokenCounts,
  moreDiagnosticsCount,
  themeAsJsonDocument,
} from "./ThemeFileGroup";
import { OVERLAY_TOKEN_FIELDS } from "./overlayTokenFields";

describe("moreDiagnosticsCount", () => {
  test("is 0 when nothing was capped", () => {
    expect(moreDiagnosticsCount(3, 3)).toBe(0);
  });

  test("is the difference when the payload was capped", () => {
    // A broken file can produce one diagnostic per token; the payload caps at 5.
    expect(moreDiagnosticsCount(9, 5)).toBe(4);
  });

  test("never goes negative", () => {
    expect(moreDiagnosticsCount(2, 5)).toBe(0);
  });
});

describe("lockedTokenCounts", () => {
  test("the total is the tokens with a row, not every token there is", () => {
    const { total } = lockedTokenCounts([]);
    expect(total).toBe(OVERLAY_TOKEN_FIELDS.length);
    // One token short of the contract's fifteen: `glass_material` drives the
    // pre-macOS-26 fallback engine and has no row to be shown locked in.
    expect(total).toBe(Object.keys(INHERIT_ALL).length - 1);
  });

  test("counts the owned tokens the tab can show as locked", () => {
    expect(lockedTokenCounts(["accent", "radius", "material"]).count).toBe(3);
  });

  test("a tab-less token the file owns is not counted", () => {
    expect(lockedTokenCounts(["glass_material"]).count).toBe(0);
    expect(lockedTokenCounts(["accent", "glass_material"]).count).toBe(1);
  });
});

describe("diagnosticI18nKey", () => {
  test("maps every code to a distinct settings.appearance.themeFile key", () => {
    const codes: Array<Parameters<typeof diagnosticI18nKey>[0]> = [
      "malformed_document",
      "unsupported_version",
      "unknown_key",
      "wrong_type",
      "invalid_color",
      "out_of_bounds",
      "unreadable",
    ];
    const keys = codes.map(diagnosticI18nKey);
    expect(new Set(keys).size).toBe(codes.length);
    keys.forEach((key) =>
      expect(key.startsWith("settings.appearance.themeFile.")).toBe(true),
    );
  });

  test("unsupported_version reuses the dedicated newerVersion copy", () => {
    expect(diagnosticI18nKey("unsupported_version")).toBe(
      "settings.appearance.themeFile.newerVersion",
    );
  });
});

describe("themeAsJsonDocument", () => {
  test("a fully-inherited theme copies as just the version", () => {
    expect(JSON.parse(themeAsJsonDocument(INHERIT_ALL))).toEqual({
      version: 1,
    });
  });

  test("only set tokens are emitted", () => {
    const theme: OverlayTheme = {
      ...INHERIT_ALL,
      accent: "#7aa2f7",
      radius: 12,
    };
    expect(JSON.parse(themeAsJsonDocument(theme))).toEqual({
      version: 1,
      accent: "#7aa2f7",
      radius: 12,
    });
  });

  test("is valid JSON", () => {
    const theme: OverlayTheme = { ...INHERIT_ALL, material: "glass" };
    expect(() => JSON.parse(themeAsJsonDocument(theme))).not.toThrow();
  });

  test("serializes exactly like the contract's examples", () => {
    // Two-space indent, `version` first, tokens after it in the contract's
    // own table order — not whatever order the runtime object happens to
    // carry. This is the document a theming tool is handed, so its shape is
    // part of the contract.
    expect(themeAsJsonDocument(INHERIT_ALL)).toBe('{\n  "version": 1\n}');

    const theme: OverlayTheme = {
      ...INHERIT_ALL,
      radius: 12,
      accent: "#7aa2f7",
      material: "glass",
    };
    expect(themeAsJsonDocument(theme)).toBe(
      [
        "{",
        '  "version": 1,',
        '  "accent": "#7aa2f7",',
        '  "material": "glass",',
        '  "radius": 12',
        "}",
      ].join("\n"),
    );
  });
});
