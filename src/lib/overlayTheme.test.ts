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
  BORDER_INHERIT_CLEAR,
  BORDER_OPACITY_INHERIT,
  BORDER_OPACITY_INHERIT_CLEAR,
  getStoredOverlayTheme,
  GLASS_TINT_INHERIT,
  inheritedBorder,
  inheritedTokenValue,
  INHERIT_ALL,
  OVERLAY_THEME_CSS_PROPERTIES,
  OVERLAY_THEME_STORAGE_KEY,
  OVERLAY_TOKEN_BOUNDS,
  overlayThemeStyleDelta,
  resolveOverlayThemeVars,
  storeOverlayTheme,
  SURFACE_OPACITY_INHERIT,
} from "./overlayTheme";

/**
 * Unit tests for the apply layer. Run with `bun test src`.
 *
 * Every expected value here is written out by hand from the token rules, the
 * README's worked example and the Glass neutrals, never read back off the
 * implementation.
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

  test("a second apply only touches what actually moved", () => {
    const root = fakeRoot();
    applyOverlayTheme(root.element, resolved({ accent: "#7aa2f7", radius: 8 }));
    const writtenFirst = root.properties.size;
    root.removed.length = 0;

    // Same accent, new radius. Only the radius is written again, and nothing
    // is removed.
    applyOverlayTheme(
      root.element,
      resolved({ accent: "#7aa2f7", radius: 12 }),
    );
    expect(root.removed).toEqual([]);
    expect(root.properties.get("--ov-radius")).toBe("12px");
    expect(root.properties.size).toBe(writtenFirst);

    // Dropping the radius removes that one property and leaves the accent.
    applyOverlayTheme(root.element, resolved({ accent: "#7aa2f7" }));
    expect(root.removed).toEqual(["--ov-radius"]);
    expect(root.properties.get("--s-accent")).toBe("#7aa2f7");
  });

  // The Appearance tab shows these numbers on a control while its token is
  // unset, so they have to be what the overlay is actually painted with. The
  // literals are transcribed from the `:root` block of RecordingOverlay.css,
  // and a Rust test (`overlay_token_inherit_values_match_the_css`) reads both
  // files and fails if either moves.
  test("every numeric token has the value the stylesheet inherits", () => {
    const lengths: Record<string, number> = {
      size_scale: 1,
      radius: 24,
      border_width: 1,
      padding: 10,
      waveform_gap: 3,
      waveform_width: 4,
    };
    for (const [key, expected] of Object.entries(lengths)) {
      expect(
        inheritedTokenValue(
          key as keyof typeof OVERLAY_TOKEN_BOUNDS,
          "flat",
          "regular",
        ),
      ).toBe(expected);
      // A length is one number on both Materials and both Glass styles.
      expect(
        inheritedTokenValue(
          key as keyof typeof OVERLAY_TOKEN_BOUNDS,
          "glass",
          "clear",
        ),
      ).toBe(expected);
    }

    expect(inheritedTokenValue("surface_opacity", "flat", "regular")).toBe(
      SURFACE_OPACITY_INHERIT,
    );
    expect(inheritedTokenValue("glass_tint", "glass", "regular")).toBe(
      GLASS_TINT_INHERIT,
    );
    // The tint is one number for both Glass styles. Measured against
    // Spotlight and left alone: see the constant's own doc.
    expect(inheritedTokenValue("glass_tint", "glass", "clear")).toBe(
      GLASS_TINT_INHERIT,
    );
  });

  // The one token whose inherit is not one number. The card's edge is
  // stronger over glass and stronger again over Clear glass, and asking for
  // it per Material and Glass style is what keeps that rule out of every
  // caller.
  test("the border alpha inherits per Material and Glass style", () => {
    expect(inheritedTokenValue("border_opacity", "flat", "regular")).toBe(
      BORDER_OPACITY_INHERIT.flat,
    );
    expect(inheritedTokenValue("border_opacity", "glass", "regular")).toBe(
      BORDER_OPACITY_INHERIT.glass,
    );
    expect(inheritedTokenValue("border_opacity", "glass", "clear")).toBe(
      BORDER_OPACITY_INHERIT_CLEAR,
    );
    // The Glass style never reaches Flat's edge.
    expect(inheritedTokenValue("border_opacity", "flat", "clear")).toBe(
      BORDER_OPACITY_INHERIT.flat,
    );
  });

  // Spotlight's capsule carries a bright rim in both appearances. Clear is
  // the one surface of ours dark enough in both for a white edge to read, so
  // it is the one that inherits white; Flat and Regular keep the foreground
  // mix, which is what stays visible over their near-white Light card.
  test("only Clear glass inherits a white rim", () => {
    expect(inheritedBorder("glass", "clear")).toEqual({
      color: BORDER_INHERIT_CLEAR,
      opacity: BORDER_OPACITY_INHERIT_CLEAR,
    });
    expect(BORDER_INHERIT_CLEAR).toBe("#ffffff");
    expect(BORDER_OPACITY_INHERIT_CLEAR).toBe(0.35);
    for (const [material, glassStyle] of [
      ["glass", "regular"],
      ["flat", "regular"],
      ["flat", "clear"],
    ] as const) {
      expect(inheritedBorder(material, glassStyle)).toEqual({
        color: BORDER_INHERIT,
        opacity: BORDER_OPACITY_INHERIT[material],
      });
    }
  });

  // Every bound has an inherit and every inherit has a bound. A token that
  // gained one and not the other would be a slider with no value or a value
  // no slider can show.
  test("every numeric token's inherit is inside its own bounds", () => {
    for (const key of Object.keys(
      OVERLAY_TOKEN_BOUNDS,
    ) as (keyof typeof OVERLAY_TOKEN_BOUNDS)[]) {
      const { min, max } = OVERLAY_TOKEN_BOUNDS[key];
      for (const material of ["flat", "glass"] as const) {
        for (const glassStyle of ["regular", "clear"] as const) {
          const value = inheritedTokenValue(key, material, glassStyle);
          expect(value).toBeGreaterThanOrEqual(min);
          expect(value).toBeLessThanOrEqual(max);
        }
      }
    }
  });
});

describe("overlayThemeStyleDelta", () => {
  test("an unknown previous state clears every property this module may write", () => {
    const { set, remove } = overlayThemeStyleDelta(null, {
      "--s-accent": "#7aa2f7",
    });
    expect(set).toEqual([["--s-accent", "#7aa2f7"]]);
    expect(remove).toEqual(
      OVERLAY_THEME_CSS_PROPERTIES.filter((p) => p !== "--s-accent"),
    );
  });

  test("a known previous state only removes what it actually wrote", () => {
    const { set, remove } = overlayThemeStyleDelta(
      { "--s-accent": "#7aa2f7", "--ov-radius": "8px" },
      { "--s-accent": "#7aa2f7" },
    );
    expect(set).toEqual([]);
    expect(remove).toEqual(["--ov-radius"]);
  });

  test("an unchanged property is not rewritten", () => {
    const { set, remove } = overlayThemeStyleDelta(
      { "--s-accent": "#7aa2f7", "--ov-pad": "10px" },
      { "--s-accent": "#7aa2f7", "--ov-pad": "11px" },
    );
    expect(set).toEqual([["--ov-pad", "11px"]]);
    expect(remove).toEqual([]);
  });

  test("nothing to do is nothing to do", () => {
    expect(
      overlayThemeStyleDelta(
        { "--s-accent": "#7aa2f7" },
        { "--s-accent": "#7aa2f7" },
      ),
    ).toEqual({ set: [], remove: [] });
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
      // The Glass tint reaches zero, where the Flat opacity stops at 0.30.
      // Untinted glass is a look; an invisible Flat card is not.
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
      "--ov-pad": "14px",
      "--ov-wave-gap": "2px",
      "--ov-wave-w": "5px",
    });
  });

  test("a zero border width still writes the property", () => {
    // 0 is a value, not "unset". Without the property the stylesheet's own
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
    // the same foreground mix Flat uses, only at a stronger alpha. Regular is
    // the style an unset `glass_style` resolves to.
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

  // Clear is the see-through style, so its card is dark enough in both app
  // themes to carry the white highlight Spotlight's capsule carries. Only
  // the edge moves: the tint, the neutrals and the surface are the style's
  // business natively, not the card's.
  test("Clear glass swaps the foreground hairline for a white rim", () => {
    const clear = resolveOverlayThemeVars(
      resolved({ glass_style: "clear" }, "glass"),
    );
    expect(clear["--s-border"]).toBe(
      "color-mix(in srgb, #ffffff 35%, transparent)",
    );
    const regular = resolveOverlayThemeVars(resolved({}, "glass"));
    for (const property of [
      "--s-surface",
      "--s-muted",
      "--s-faint",
      "--s-hair",
    ]) {
      expect(clear[property]).toBe(regular[property]);
    }
  });

  // The rim is Glass's alone. Under Flat the style is a token with nothing to
  // draw, so a stored `glass_style` cannot leak a white edge onto a Flat card.
  test("the Glass style never reaches a Flat card's edge", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({ glass_style: "clear", border_opacity: 0.4 }),
      )["--s-border"],
    ).toBe("color-mix(in srgb, var(--s-text) 40%, transparent)");
  });

  // Rule 2 at the boundary: the localStorage mirror bypasses Rust, so a
  // hand-edited style falls back to the one an unset token resolves to.
  test("an unreadable Glass style inherits Regular's edge", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({ glass_style: "CLEAR" as never }, "glass"),
      )["--s-border"],
    ).toBe("color-mix(in srgb, var(--s-text) 25%, transparent)");
  });

  // The invariant the two constants above exist for: what an unset token
  // resolves to is what the card is actually painted with, in every
  // combination of Material and Glass style. A default that only the tab's
  // slider knew about would be a number the overlay never draws.
  //
  // Written out as literals rather than read back from `inheritedBorder`,
  // because `inheritedTokenValue` delegates to that function: comparing the
  // two would assert a function against its own body and pass whatever the
  // defaults became.
  test("an all-unset theme paints exactly the inherited edge", () => {
    const edges = [
      {
        material: "flat",
        glassStyle: "regular",
        color: "var(--s-text)",
        opacity: 0.12,
        painted: "color-mix(in srgb, var(--s-text) 12%, transparent)",
      },
      {
        material: "flat",
        glassStyle: "clear",
        color: "var(--s-text)",
        opacity: 0.12,
        painted: "color-mix(in srgb, var(--s-text) 12%, transparent)",
      },
      {
        material: "glass",
        glassStyle: "regular",
        color: "var(--s-text)",
        opacity: 0.25,
        painted: "color-mix(in srgb, var(--s-text) 25%, transparent)",
      },
      {
        material: "glass",
        glassStyle: "clear",
        color: "#ffffff",
        opacity: 0.35,
        painted: "color-mix(in srgb, #ffffff 35%, transparent)",
      },
    ] as const;
    for (const { material, glassStyle, color, opacity, painted } of edges) {
      // What the tab shows on the two controls while both tokens are unset.
      expect(inheritedBorder(material, glassStyle)).toEqual({ color, opacity });
      expect(inheritedTokenValue("border_opacity", material, glassStyle)).toBe(
        opacity,
      );
      // And what the card is painted with. Flat writes no edge at all while
      // every token is unset (the removal rule), so the alpha is handed back
      // in to make both halves of one card observable in one string.
      expect(
        resolveOverlayThemeVars(
          resolved(
            { glass_style: glassStyle, border_opacity: opacity },
            material,
          ),
        )["--s-border"],
      ).toBe(painted);
    }
  });

  test("a set border still wins over the Glass edge", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({ border: "#7aa2f7", border_opacity: 0.5 }, "glass"),
      )["--s-border"],
    ).toBe("color-mix(in srgb, #7aa2f7 50%, transparent)");
  });

  /** Liquid Glass (macOS 26) paints the same card as the older blur. The
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
  /** The bug the split exists for. A card set opaque under Flat used to
   *  follow the user into Glass and paint an opaque pane. */
  test("an opaque Flat card is still glass the moment Glass renders", () => {
    const opaqueFlat: Partial<OverlayTheme> = {
      surface: "#000000",
      surface_opacity: 1.0,
    };

    expect(resolveOverlayThemeVars(resolved(opaqueFlat))["--s-surface"]).toBe(
      "color-mix(in srgb, #000000 100%, transparent)",
    );
    // Same theme, Glass rendering. The opacity is not read, so the tint is
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
    // Not even the surface property. Under Flat an untouched card is the
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
  // rendering where Glass was requested and downgraded (the percentages in
  // the Glass case are covered above), which is also why `--s-surface`
  // carries the 92% Flat opacity rather than the 45% tint. `glass_material`
  // and `glass_style` are the two tokens with no CSS at all, each setting a
  // native view's property, so neither writes anything. The derivation rules
  // mix the neutrals from `var(--s-text)`, which resolves to the `--s-text`
  // written beside them, while the explicit `border` replaces that mix for
  // the edge.
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
      "--ov-pad": "14px",
      "--ov-wave-gap": "2px",
      "--ov-wave-w": "4px",
    });
  });

  test("every property it writes is registered for removal", () => {
    // With every token set at once the module writes everything it can write,
    // so the two lists have to match exactly. A property missing from
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
