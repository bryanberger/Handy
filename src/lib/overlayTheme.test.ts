import { describe, expect, test } from "bun:test";
import type { Material, OverlayTheme, ResolvedOverlayTheme } from "@/bindings";
import {
  applyOverlayTheme,
  autoForeground,
  getStoredOverlayTheme,
  INHERIT_ALL,
  OVERLAY_THEME_CSS_PROPERTIES,
  OVERLAY_THEME_STORAGE_KEY,
  OVERLAY_TOKEN_BOUNDS,
  resolveOverlayThemeVars,
  storeOverlayTheme,
  SURFACE_OPACITY_INHERIT,
} from "./overlayTheme";

/**
 * Unit tests for the apply layer. Run with `bun test src`.
 *
 * Every expected value is transcribed from the specification — the token
 * contract's derivation rules and worked example, and the Material model's
 * Glass neutrals — never from the implementation.
 */

/** A resolved payload with the given tokens; everything else inherits. */
function resolved(
  theme: Partial<OverlayTheme>,
  effective: Material = "flat",
): ResolvedOverlayTheme {
  return {
    theme: { ...INHERIT_ALL, ...theme },
    effective_material: effective,
    glass_support: { supported: effective === "glass", available: false },
    file: {
      path: "/tmp/overlay_theme.json",
      present: false,
      version: null,
      tokens: INHERIT_ALL,
      owned_keys: [],
      diagnostics: [],
      // Added with the theme file, after this fixture was first written.
      diagnostics_total: 0,
      stale: false,
    },
  };
}

/** A stand-in for `document.documentElement` that records what was written. */
function fakeRoot() {
  const properties = new Map<string, string>();
  const removed: string[] = [];
  const dataset: Record<string, string> = {};
  const element = {
    style: {
      setProperty(name: string, value: string) {
        properties.set(name, value);
      },
      removeProperty(name: string) {
        removed.push(name);
        properties.delete(name);
      },
    },
    dataset,
  } as unknown as HTMLElement;
  return { element, properties, removed, dataset };
}

/** Run `body` against an in-memory localStorage holding `stored`. */
function withStoredMirror(stored: string | null, body: () => void): void {
  const values = new Map<string, string>();
  if (stored !== null) values.set(OVERLAY_THEME_STORAGE_KEY, stored);
  const stub = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
    removeItem: (key: string) => {
      values.delete(key);
    },
  };
  const previous = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "localStorage", {
    value: stub,
    configurable: true,
    writable: true,
  });
  try {
    body();
  } finally {
    if (previous) Object.defineProperty(globalThis, "localStorage", previous);
    else Reflect.deleteProperty(globalThis, "localStorage");
  }
}

describe("inherit", () => {
  test("an all-inherit theme writes no custom property", () => {
    expect(resolveOverlayThemeVars(resolved({}))).toEqual({});
  });

  test("applying it leaves the root clean and flat", () => {
    const root = fakeRoot();
    applyOverlayTheme(root.element, resolved({}));
    expect(root.properties.size).toBe(0);
    expect(root.dataset.material).toBe("flat");
  });

  test("a null payload removes every known property", () => {
    const root = fakeRoot();
    applyOverlayTheme(root.element, null);
    expect(root.removed).toEqual([...OVERLAY_THEME_CSS_PROPERTIES]);
    expect(root.dataset.material).toBe("flat");
  });
});

describe("derivations", () => {
  test("an accent writes the accent and its soft tint at 20%", () => {
    expect(resolveOverlayThemeVars(resolved({ accent: "#7aa2f7" }))).toEqual({
      "--s-accent": "#7aa2f7",
      "--s-accent-soft": "color-mix(in srgb, #7aa2f7 20%, transparent)",
    });
  });

  test("surface_opacity alone keeps the card theme-aware", () => {
    const vars = resolveOverlayThemeVars(resolved({ surface_opacity: 0.5 }));
    expect(vars["--s-surface"]).toBe(
      "color-mix(in srgb, var(--color-background) 50%, transparent)",
    );
    expect(vars["--s-text"]).toBeUndefined();
  });

  test("an unset surface_opacity resolves to 0.98 under Flat", () => {
    expect(SURFACE_OPACITY_INHERIT.flat).toBe(0.98);
    expect(
      resolveOverlayThemeVars(resolved({ surface: "#1a1b26" }))["--s-surface"],
    ).toBe("color-mix(in srgb, #1a1b26 98%, transparent)");
  });

  test("a surface alone picks its foreground by WCAG contrast", () => {
    // theme.css's two inks: #0f0f0f on a light card, #fbfbfb on a dark one.
    expect(autoForeground("#fbfbfb")).toBe("#0f0f0f");
    expect(autoForeground("#1a1b26")).toBe("#fbfbfb");

    expect(
      resolveOverlayThemeVars(resolved({ surface: "#fbfbfb" }))["--s-text"],
    ).toBe("#0f0f0f");
    expect(
      resolveOverlayThemeVars(resolved({ surface: "#1a1b26" }))["--s-text"],
    ).toBe("#fbfbfb");
  });

  test("an explicit text wins over the auto foreground", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({ surface: "#1a1b26", text: "#c0caf5" }),
      )["--s-text"],
    ).toBe("#c0caf5");
  });

  test("setting text alone rewrites the whole neutral group", () => {
    expect(resolveOverlayThemeVars(resolved({ text: "#c0caf5" }))).toEqual({
      "--s-text": "#c0caf5",
      "--s-muted": "color-mix(in srgb, var(--s-text) 60%, transparent)",
      "--s-faint": "color-mix(in srgb, var(--s-text) 38%, transparent)",
      "--s-border": "color-mix(in srgb, var(--s-text) 12%, transparent)",
      "--s-hair": "color-mix(in srgb, var(--s-text) 7%, transparent)",
    });
  });

  test("the numeric bounds are the contract's", () => {
    expect(OVERLAY_TOKEN_BOUNDS).toEqual({
      surface_opacity: { min: 0.3, max: 1.0, step: 0.01 },
      size_scale: { min: 0.8, max: 1.5, step: 0.05 },
      radius: { min: 0, max: 32, step: 1 },
      padding: { min: 0, max: 20, step: 1 },
      waveform_gap: { min: 0, max: 5, step: 1 },
    });
  });

  test("lengths are written raw, for CSS to multiply by the scale", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({ size_scale: 1.1, radius: 12, padding: 14, waveform_gap: 2 }),
      ),
    ).toEqual({
      "--ov-scale": "1.1",
      "--ov-radius": "12px",
      "--ov-pad-x": "14px",
      "--ov-wave-gap": "2px",
    });
  });
});

describe("Glass", () => {
  test("writes the surface and the strengthened neutrals unconditionally", () => {
    expect(SURFACE_OPACITY_INHERIT.glass).toBe(0.7);
    expect(resolveOverlayThemeVars(resolved({}, "glass"))).toEqual({
      "--s-surface":
        "color-mix(in srgb, var(--color-background) 70%, transparent)",
      "--s-muted": "color-mix(in srgb, var(--s-text) 78%, transparent)",
      "--s-faint": "color-mix(in srgb, var(--s-text) 52%, transparent)",
      "--s-border": "color-mix(in srgb, var(--s-text) 20%, transparent)",
      "--s-hair": "color-mix(in srgb, var(--s-text) 12%, transparent)",
    });
  });

  test("a requested Glass downgraded to Flat renders Flat neutrals", () => {
    const vars = resolveOverlayThemeVars(resolved({ material: "glass" }));
    expect(vars["--s-surface"]).toBeUndefined();
    expect(vars["--s-muted"]).toBeUndefined();
  });
});

describe("the worked example", () => {
  // The token contract's fully custom theme file, and the CSS it says that file
  // resolves to. Its `material: "glass"` is listed with the Flat neutrals, so
  // this is the rendering where Glass was requested and downgraded (the
  // percentages in the Glass case are covered above); and the contract's own
  // derivation rules mix the neutrals from `var(--s-text)`, which resolves to
  // the `--s-text` written beside them.
  test("resolves to exactly the twelve properties listed", () => {
    const theme: Partial<OverlayTheme> = {
      accent: "#7aa2f7",
      surface: "#1a1b26",
      surface_opacity: 0.92,
      text: "#c0caf5",
      material: "glass",
      size_scale: 1.1,
      radius: 12,
      padding: 14,
      waveform_gap: 2,
    };
    expect(resolveOverlayThemeVars(resolved(theme))).toEqual({
      "--s-accent": "#7aa2f7",
      "--s-accent-soft": "color-mix(in srgb, #7aa2f7 20%, transparent)",
      "--s-surface": "color-mix(in srgb, #1a1b26 92%, transparent)",
      "--s-text": "#c0caf5",
      "--s-muted": "color-mix(in srgb, var(--s-text) 60%, transparent)",
      "--s-faint": "color-mix(in srgb, var(--s-text) 38%, transparent)",
      "--s-border": "color-mix(in srgb, var(--s-text) 12%, transparent)",
      "--s-hair": "color-mix(in srgb, var(--s-text) 7%, transparent)",
      "--ov-scale": "1.1",
      "--ov-radius": "12px",
      "--ov-pad-x": "14px",
      "--ov-wave-gap": "2px",
    });
  });

  test("every property it writes is registered for removal", () => {
    const theme: Partial<OverlayTheme> = {
      accent: "#7aa2f7",
      surface: "#1a1b26",
      surface_opacity: 0.92,
      text: "#c0caf5",
      size_scale: 1.1,
      radius: 12,
      padding: 14,
      waveform_gap: 2,
    };
    for (const property of Object.keys(
      resolveOverlayThemeVars(resolved(theme, "glass")),
    )) {
      expect(OVERLAY_THEME_CSS_PROPERTIES).toContain(property);
    }
  });
});

describe("the removal rule", () => {
  test("a token going back to inherit takes its properties with it", () => {
    const root = fakeRoot();
    applyOverlayTheme(root.element, resolved({ accent: "#7aa2f7" }));
    expect(root.properties.get("--s-accent")).toBe("#7aa2f7");

    applyOverlayTheme(root.element, resolved({}));
    expect(root.properties.size).toBe(0);
    expect(root.removed).toContain("--s-accent");
    expect(root.removed).toContain("--s-accent-soft");
  });

  test("data-material always reflects what is rendered", () => {
    const root = fakeRoot();
    applyOverlayTheme(root.element, resolved({ material: "glass" }, "glass"));
    expect(root.dataset.material).toBe("glass");
    applyOverlayTheme(root.element, resolved({}));
    expect(root.dataset.material).toBe("flat");
  });
});

describe("the boundary re-validation", () => {
  test("malformed values from a hand-edited mirror are treated as unset", () => {
    // Shaped like a payload, but every value is one Rust would never produce.
    const stale = JSON.parse(`{
      "theme": {
        "accent": "red",
        "surface": "#abc",
        "surface_opacity": 5,
        "text": "#c0caf5ff",
        "material": "glass",
        "size_scale": 9,
        "radius": -4,
        "padding": "14px",
        "waveform_gap": null
      },
      "effective_material": "opaque",
      "glass_support": { "supported": true, "available": true },
      "file": null
    }`) as ResolvedOverlayTheme;

    expect(resolveOverlayThemeVars(stale)).toEqual({});

    const root = fakeRoot();
    applyOverlayTheme(root.element, stale);
    expect(root.dataset.material).toBe("flat");
  });

  test("a mirror that is not a payload is ignored", () => {
    withStoredMirror("not json at all", () => {
      expect(getStoredOverlayTheme()).toBeNull();
    });
    withStoredMirror('{"effective_material":"flat"}', () => {
      expect(getStoredOverlayTheme()).toBeNull();
    });
    withStoredMirror(null, () => {
      expect(getStoredOverlayTheme()).toBeNull();
    });
  });

  test("a stored payload round-trips", () => {
    withStoredMirror(null, () => {
      const payload = resolved({ accent: "#7aa2f7" });
      storeOverlayTheme(payload);
      expect(getStoredOverlayTheme()).toEqual(payload);
    });
  });
});
