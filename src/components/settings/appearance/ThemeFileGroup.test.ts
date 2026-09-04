import { describe, expect, test } from "bun:test";
import type {
  ManagedReason,
  OverlayTheme,
  ResolvedOverlayTheme,
} from "@/bindings";
import { INHERIT_ALL } from "@/lib/overlayTheme";
import {
  diagnosticI18nKey,
  setTokenCounts,
  moreDiagnosticsCount,
  themeAsJsonDocument,
  themeFileStatus,
} from "./ThemeFileGroup";
import { EMPTY_FILE_STATE } from "./useOverlayThemeVars";
import { OVERLAY_TOKEN_FIELDS } from "./overlayTokenFields";

const NO_COUNTS = { count: 0, total: 20 };

function fileState(
  overrides: Partial<ResolvedOverlayTheme["file"]>,
): ResolvedOverlayTheme["file"] {
  return {
    ...EMPTY_FILE_STATE,
    path: "/home/u/.config/handy/overlay_theme.json",
    ...overrides,
  };
}

function managed(
  reason: ManagedReason,
  target: string | null = null,
): ResolvedOverlayTheme["file"] {
  return fileState({
    present: true,
    ownership: { writable: false, reason, target },
  });
}

/**
 * The Theme File group says one of three things, and which one decides whether
 * every token row above it is editable. Pure, so the states are asserted here
 * rather than screenshotted.
 */
describe("themeFileStatus", () => {
  test("a file Handy writes is the theme, and says how much of it is set", () => {
    expect(
      themeFileStatus(fileState({ present: true }), { count: 4, total: 20 }),
    ).toEqual({
      key: "settings.appearance.themeFile.owned",
      params: { count: 4, total: 20 },
    });
  });

  test("no file yet is not a problem, only a path Handy will create", () => {
    expect(themeFileStatus(fileState({}), NO_COUNTS).key).toBe(
      "settings.appearance.themeFile.notFound",
    );
  });

  test("each managed reason has its own line", () => {
    expect(managedKeys()).toEqual([
      "settings.appearance.themeFile.managed.symlink",
      "settings.appearance.themeFile.managed.readOnly",
      "settings.appearance.themeFile.managed.notCreatable",
      "settings.appearance.themeFile.managed.unknown",
    ]);
  });

  test("a symlink names the file really in charge", () => {
    expect(
      themeFileStatus(
        managed("symlink", "/dotfiles/handy/tokyo.json"),
        NO_COUNTS,
      ),
    ).toEqual({
      key: "settings.appearance.themeFile.managed.symlink",
      params: { target: "/dotfiles/handy/tokyo.json" },
    });
  });

  test("a reason with no target of its own falls back to the path", () => {
    const file = managed("read_only");
    expect(themeFileStatus(file, NO_COUNTS).params).toEqual({
      target: file.path,
    });
  });

  test("managed outranks present: a locked file is never the owned line", () => {
    const file = managed("symlink", "/elsewhere.json");
    expect(themeFileStatus({ ...file, present: false }, NO_COUNTS).key).toBe(
      "settings.appearance.themeFile.managed.symlink",
    );
  });
});

function managedKeys(): string[] {
  const reasons: ManagedReason[] = [
    "symlink",
    "read_only",
    "not_creatable",
    "unknown",
  ];
  return reasons.map(
    (reason) => themeFileStatus(managed(reason), NO_COUNTS).key,
  );
}

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

describe("setTokenCounts", () => {
  test("the total is the rows on screen, not every token there is", () => {
    // Twenty-one of the contract's twenty-three under Flat. `glass_material`
    // drives the pre-macOS-26 fallback engine and has no row; the two alphas
    // share one slot, Flat's surface opacity or Glass's tint, never both.
    expect(setTokenCounts([], "flat", true).total).toBe(21);
    expect(Object.keys(INHERIT_ALL).length).toBe(23);

    // One fewer under Glass: macOS places its own window shadow and takes no
    // offset, so that row is not on screen to be counted.
    expect(setTokenCounts([], "glass", true).total).toBe(20);

    // The two shadow rows are one token, so the table has one entry more than
    // the tokens it covers. The two waveform lengths count under every style,
    // so the total does not move as the user picks one.
    expect(OVERLAY_TOKEN_FIELDS.length).toBe(23);
  });

  test("a hidden waveform takes its whole group out of the total", () => {
    // The tab drops the Waveform group with the waveform, so its three rows
    // (the style and the two lengths) are not there to be counted.
    expect(setTokenCounts([], "flat", false).total).toBe(18);
    expect(setTokenCounts([], "glass", false).total).toBe(17);
    // A file owning one of them counts nothing while the group is gone.
    const waveformOwned = ["waveform_style", "accent"];
    expect(setTokenCounts(waveformOwned, "flat", false)).toEqual({
      count: 1,
      total: 18,
    });
    expect(setTokenCounts(waveformOwned, "flat", true)).toEqual({
      count: 2,
      total: 21,
    });
  });

  test("counts the tokens the file sets that have a row on screen", () => {
    expect(
      setTokenCounts(["accent", "radius", "material"], "flat", true).count,
    ).toBe(3);
  });

  test("a token the file sets with no row anywhere is not counted", () => {
    expect(setTokenCounts(["glass_material"], "flat", true).count).toBe(0);
    expect(
      setTokenCounts(["accent", "glass_material"], "flat", true).count,
    ).toBe(1);
  });

  test("nor is the alpha belonging to the other Material", () => {
    // A file pinning both alphas fills exactly one row, whichever Material is
    // painted; the other control is not on screen to be counted.
    const bothAlphas = ["surface_opacity", "glass_tint"];
    expect(setTokenCounts(bothAlphas, "flat", true)).toEqual({
      count: 1,
      total: 21,
    });
    expect(setTokenCounts(bothAlphas, "glass", true)).toEqual({
      count: 1,
      total: 20,
    });
    expect(setTokenCounts(["glass_tint"], "flat", true).count).toBe(0);
    expect(setTokenCounts(["surface_opacity"], "glass", true).count).toBe(0);
  });

  test("the shadow offset counts only where it has a row", () => {
    // The token still applies under Glass, with no row there, so counting it
    // would promise one the user cannot find.
    expect(setTokenCounts(["shadow_offset_y"], "flat", true).count).toBe(1);
    expect(setTokenCounts(["shadow_offset_y"], "glass", true).count).toBe(0);
    // The strength has a row on both, a slider or a switch.
    for (const material of ["flat", "glass"] as const) {
      expect(setTokenCounts(["shadow_strength"], material, true).count).toBe(1);
    }
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
    // table order, not the runtime object's. This is the document a theming
    // tool is handed, so its shape is part of the contract.
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
