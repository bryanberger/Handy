import { describe, expect, test } from "bun:test";
import type {
  GlassEngine,
  Material,
  OverlayTheme,
  ResolvedOverlayTheme,
} from "@/bindings";
import {
  applyOverlayTheme,
  autoForeground,
  BORDER_INHERIT,
  BORDER_OPACITY_INHERIT,
  getStoredOverlayTheme,
  GLASS_TINT_INHERIT,
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
  engine: GlassEngine = "visual_effect",
): ResolvedOverlayTheme {
  return {
    theme: { ...INHERIT_ALL, ...theme },
    effective_material: effective,
    glass_support: {
      supported: effective === "glass",
      available: false,
      engine,
    },
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
    expect(SURFACE_OPACITY_INHERIT).toBe(0.98);
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
      // The Glass tint reaches zero, where the Flat opacity stops at 0.30:
      // untinted glass is a look, an invisible Flat card is not.
      glass_tint: { min: 0.0, max: 1.0, step: 0.01 },
      border_opacity: { min: 0.0, max: 1.0, step: 0.01 },
      size_scale: { min: 0.8, max: 1.5, step: 0.05 },
      radius: { min: 0, max: 32, step: 1 },
      border_width: { min: 0, max: 4, step: 1 },
      padding: { min: 0, max: 20, step: 1 },
      waveform_gap: { min: 0, max: 5, step: 1 },
      waveform_width: { min: 2, max: 6, step: 1 },
    });
  });

  test("lengths are written raw, for CSS to multiply by the scale", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({
          size_scale: 1.1,
          radius: 12,
          border_width: 2,
          padding: 14,
          waveform_gap: 2,
          waveform_width: 5,
        }),
      ),
    ).toEqual({
      "--ov-scale": "1.1",
      "--ov-radius": "12px",
      "--ov-border-w": "2px",
      "--ov-pad-x": "14px",
      "--ov-wave-gap": "2px",
      "--ov-wave-w": "5px",
    });
  });

  test("a zero border width still writes the property", () => {
    // 0 is a value, not "unset": without the property the stylesheet's own
    // 1px would come back and the native window would be two points wider
    // than the card.
    expect(
      resolveOverlayThemeVars(resolved({ border_width: 0 }))["--ov-border-w"],
    ).toBe("0px");
  });
});

describe("the border", () => {
  test("its two tokens compose into one property", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({ border: "#ff0000", border_opacity: 0.4 }),
      ),
    ).toEqual({
      "--s-border": "color-mix(in srgb, #ff0000 40%, transparent)",
    });
  });

  test("a colour alone inherits the Flat opacity, and vice versa", () => {
    expect(BORDER_OPACITY_INHERIT.flat).toBe(0.12);
    expect(BORDER_INHERIT).toBe("var(--s-text)");

    expect(
      resolveOverlayThemeVars(resolved({ border: "#ff0000" }))["--s-border"],
    ).toBe("color-mix(in srgb, #ff0000 12%, transparent)");
    expect(
      resolveOverlayThemeVars(resolved({ border_opacity: 0.4 }))["--s-border"],
    ).toBe("color-mix(in srgb, var(--s-text) 40%, transparent)");
  });

  test("a fully transparent edge is a value, not an unset token", () => {
    expect(
      resolveOverlayThemeVars(resolved({ border_opacity: 0 }))["--s-border"],
    ).toBe("color-mix(in srgb, var(--s-text) 0%, transparent)");
  });

  test("neither token alone disturbs the rest of the neutrals", () => {
    const vars = resolveOverlayThemeVars(resolved({ border: "#ff0000" }));
    expect(vars["--s-muted"]).toBeUndefined();
    expect(vars["--s-faint"]).toBeUndefined();
    expect(vars["--s-hair"]).toBeUndefined();
  });
});

describe("Glass", () => {
  test("writes the surface, the neutrals and the edge unconditionally", () => {
    // The tint is thin enough for the blur to read as blur, and the edge is
    // the same foreground mix Flat uses, only at a stronger alpha.
    expect(GLASS_TINT_INHERIT).toBe(0.45);
    expect(BORDER_OPACITY_INHERIT.glass).toBe(0.25);

    expect(resolveOverlayThemeVars(resolved({}, "glass"))).toEqual({
      "--s-surface":
        "color-mix(in srgb, var(--color-background) 45%, transparent)",
      "--s-muted": "color-mix(in srgb, var(--s-text) 78%, transparent)",
      "--s-faint": "color-mix(in srgb, var(--s-text) 52%, transparent)",
      "--s-hair": "color-mix(in srgb, var(--s-text) 12%, transparent)",
      "--s-border": "color-mix(in srgb, var(--s-text) 25%, transparent)",
    });
  });

  test("a set border still wins over the Glass edge", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({ border: "#7aa2f7", border_opacity: 0.5 }, "glass"),
      )["--s-border"],
    ).toBe("color-mix(in srgb, #7aa2f7 50%, transparent)");
  });

  /** Liquid Glass (macOS 26) paints the same card as the older blur: the
   *  engine changes which native view draws behind it and which of the two
   *  engine tokens applies, never what the card writes. Rust hands
   *  `NSGlassEffectView` the same surface as its `tintColor`, composed from
   *  the identical `surface`/`glass_tint` pair. */
  test("the engine does not change what the card paints", () => {
    const older = resolveOverlayThemeVars(resolved({}, "glass"));
    const liquid = resolveOverlayThemeVars(resolved({}, "glass", "liquid"));
    expect(liquid).toEqual(older);
    expect(liquid["--s-surface"]).toBe(
      "color-mix(in srgb, var(--color-background) 45%, transparent)",
    );
  });

  test("Flat is Flat on every engine", () => {
    expect(resolveOverlayThemeVars(resolved({}, "flat", "liquid"))).toEqual({});
  });

  test("a requested Glass downgraded to Flat renders Flat neutrals", () => {
    const vars = resolveOverlayThemeVars(resolved({ material: "glass" }));
    expect(vars["--s-surface"]).toBeUndefined();
    expect(vars["--s-muted"]).toBeUndefined();
    expect(vars["--s-border"]).toBeUndefined();
  });
});

describe("the two alphas", () => {
  /** The bug the split exists for: a card set opaque under Flat used to
   *  follow the user into Glass and paint an opaque pane. */
  test("an opaque Flat card is still glass the moment Glass renders", () => {
    const opaqueFlat: Partial<OverlayTheme> = {
      surface: "#000000",
      surface_opacity: 1.0,
    };

    expect(resolveOverlayThemeVars(resolved(opaqueFlat))["--s-surface"]).toBe(
      "color-mix(in srgb, #000000 100%, transparent)",
    );
    // Same theme, Glass rendering: the opacity is not read, so the tint is
    // the Glass default and the blur shows through.
    expect(
      resolveOverlayThemeVars(resolved(opaqueFlat, "glass"))["--s-surface"],
    ).toBe("color-mix(in srgb, #000000 45%, transparent)");
  });

  test("the Glass tint drives the card under Glass, and nothing else", () => {
    expect(
      resolveOverlayThemeVars(resolved({ glass_tint: 0.15 }, "glass"))[
        "--s-surface"
      ],
    ).toBe("color-mix(in srgb, var(--color-background) 15%, transparent)");
    expect(
      resolveOverlayThemeVars(
        resolved({ surface: "#1a1b26", glass_tint: 0.6 }, "glass"),
      )["--s-surface"],
    ).toBe("color-mix(in srgb, #1a1b26 60%, transparent)");

    // A fully transparent tint is a value: untinted glass, Apple's own look.
    expect(
      resolveOverlayThemeVars(resolved({ glass_tint: 0 }, "glass"))[
        "--s-surface"
      ],
    ).toBe("color-mix(in srgb, var(--color-background) 0%, transparent)");
  });

  test("the Glass tint writes nothing under Flat", () => {
    // Not even the surface property: under Flat an untouched card is the
    // stylesheet's own 98%, and the tint token has no say in it.
    expect(resolveOverlayThemeVars(resolved({ glass_tint: 0.15 }))).toEqual({});
  });

  test("the surface colour is shared: it is the tint colour under Glass", () => {
    const surfaceOnly = resolved({ surface: "#1a1b26" }, "glass");
    expect(resolveOverlayThemeVars(surfaceOnly)["--s-surface"]).toBe(
      "color-mix(in srgb, #1a1b26 45%, transparent)",
    );
    // …and it still picks the foreground, as it does under Flat.
    expect(resolveOverlayThemeVars(surfaceOnly)["--s-text"]).toBe("#fbfbfb");
  });
});

describe("the worked example", () => {
  /** The README's "A full theme", every one of the sixteen tokens set. */
  const FULL_THEME: Partial<OverlayTheme> = {
    accent: "#7aa2f7",
    surface: "#1a1b26",
    surface_opacity: 0.92,
    glass_tint: 0.45,
    text: "#c0caf5",
    border: "#ffffff",
    border_opacity: 0.3,
    material: "glass",
    glass_material: "popover",
    glass_style: "clear",
    size_scale: 1.1,
    radius: 12,
    border_width: 1,
    padding: 14,
    waveform_gap: 2,
    waveform_width: 4,
  };

  // The README's full theme file and the CSS it resolves to. Its
  // `material: "glass"` is read here with the Flat neutrals, so this is the
  // rendering where Glass was requested and downgraded (the percentages in the
  // Glass case are covered above) — which is also why `--s-surface` carries
  // the 92% Flat opacity and not the 45% tint; `glass_material` and
  // `glass_style` are the two tokens with no CSS at all — each sets a native
  // view's property — so neither writes anything. The
  // contract's derivation rules mix the neutrals from `var(--s-text)`, which
  // resolves to the `--s-text` written beside them, while the explicit
  // `border` replaces that mix for the edge.
  test("resolves to exactly the fourteen properties listed", () => {
    expect(resolveOverlayThemeVars(resolved(FULL_THEME))).toEqual({
      "--s-accent": "#7aa2f7",
      "--s-accent-soft": "color-mix(in srgb, #7aa2f7 20%, transparent)",
      "--s-surface": "color-mix(in srgb, #1a1b26 92%, transparent)",
      "--s-text": "#c0caf5",
      "--s-muted": "color-mix(in srgb, var(--s-text) 60%, transparent)",
      "--s-faint": "color-mix(in srgb, var(--s-text) 38%, transparent)",
      "--s-border": "color-mix(in srgb, #ffffff 30%, transparent)",
      "--s-hair": "color-mix(in srgb, var(--s-text) 7%, transparent)",
      "--ov-scale": "1.1",
      "--ov-radius": "12px",
      "--ov-border-w": "1px",
      "--ov-pad-x": "14px",
      "--ov-wave-gap": "2px",
      "--ov-wave-w": "4px",
    });
  });

  test("every property it writes is registered for removal", () => {
    // With every token set at once the module writes everything it can write,
    // so the two lists have to match exactly: a property missing from
    // OVERLAY_THEME_CSS_PROPERTIES would keep painting after its token went
    // back to inherit, and one listed but never written would be dead weight.
    const written = Object.keys(
      resolveOverlayThemeVars(resolved(FULL_THEME, "glass")),
    );
    for (const property of written) {
      expect(OVERLAY_THEME_CSS_PROPERTIES).toContain(property);
    }
    expect([...written].sort()).toEqual(
      [...OVERLAY_THEME_CSS_PROPERTIES].sort(),
    );
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
        "glass_tint": 5,
        "text": "#c0caf5ff",
        "material": "glass",
        "size_scale": 9,
        "radius": -4,
        "border_opacity": 5,
        "border_width": 9,
        "padding": "14px",
        "waveform_gap": null,
        "waveform_width": 1
      },
      "effective_material": "opaque",
      "glass_support": { "supported": true, "available": true, "engine": "liquid" },
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
