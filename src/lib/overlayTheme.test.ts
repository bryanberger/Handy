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
  BOOLEAN_INHERIT,
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
  SHADOW_BLUR_PX,
  SHADOW_STRENGTH_INHERIT,
  storeOverlayTheme,
  SURFACE_OPACITY_INHERIT,
  switchToken,
  WAVEFORM_STYLE_INHERIT,
  WAVEFORM_STYLES,
  waveformStyleToken,
} from "./overlayTheme";

/**
 * Unit tests for the apply layer. Run with `bun test src`.
 *
 * Every expected value is hand-written from the token rules, the README's
 * worked example and the Glass neutrals, never read off the implementation.
 */

/** A resolved payload with the given tokens; the rest inherit. */
function resolved(
  theme: Partial<OverlayTheme>,
  effective: Material = "flat",
  engine: GlassEngine = "visual_effect",
): ResolvedOverlayTheme {
  return {
    theme: { ...INHERIT_ALL, ...theme },
    effective_material: effective,
    // The anchored edge's room, as macOS's Bottom placement offers it. Only
    // the shadow's own tests move it.
    shadow_edge_slack: 15,
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
      // Added with the theme file, after this fixture was written.
      diagnostics_total: 0,
      stale: false,
    },
  };
}

/** A `document.documentElement` stand-in that records what was written. */
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

/** Run `body` against in-memory localStorage holding `stored`. */
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

    // Same accent, new radius. Only the radius is rewritten, nothing removed.
    applyOverlayTheme(
      root.element,
      resolved({ accent: "#7aa2f7", radius: 12 }),
    );
    expect(root.removed).toEqual([]);
    expect(root.properties.get("--ov-radius")).toBe("12px");
    expect(root.properties.size).toBe(writtenFirst);

    // Dropping the radius removes it and leaves the accent.
    applyOverlayTheme(root.element, resolved({ accent: "#7aa2f7" }));
    expect(root.removed).toEqual(["--ov-radius"]);
    expect(root.properties.get("--s-accent")).toBe("#7aa2f7");
  });

  // The Appearance tab shows these while a token is unset, so they must be what
  // the overlay paints, transcribed from `:root` in RecordingOverlay.css.
  // `overlay_token_inherit_values_match_the_css` in Rust fails if either moves.
  test("every numeric token has the value the stylesheet inherits", () => {
    const lengths: Record<string, number> = {
      size_scale: 1,
      radius: 24,
      border_width: 1,
      padding: 10,
      element_gap: 0,
      waveform_gap: 3,
      waveform_width: 4,
      shadow_offset_y: 4,
    };
    for (const [key, expected] of Object.entries(lengths)) {
      expect(
        inheritedTokenValue(
          key as keyof typeof OVERLAY_TOKEN_BOUNDS,
          "flat",
          "regular",
        ),
      ).toBe(expected);
      // A length is one number on both Materials and Glass styles.
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
    // One tint for both Glass styles. Measured against Spotlight; see its doc.
    expect(inheritedTokenValue("glass_tint", "glass", "clear")).toBe(
      GLASS_TINT_INHERIT,
    );
  });

  // The second token whose inherit depends on the Material, and the only one
  // whose two inherits are its range's ends. Flat's card has never cast a
  // shadow; Glass's window has always cast macOS's.
  test("the shadow inherits none under Flat and macOS's under Glass", () => {
    expect(SHADOW_STRENGTH_INHERIT).toEqual({ flat: 0, glass: 1 });
    for (const glassStyle of ["regular", "clear"] as const) {
      expect(inheritedTokenValue("shadow_strength", "flat", glassStyle)).toBe(
        0,
      );
      expect(inheritedTokenValue("shadow_strength", "glass", glassStyle)).toBe(
        1,
      );
    }
  });

  // Both switches are on, so an unset theme draws today's row.
  test("both row elements are shown while unset", () => {
    expect(BOOLEAN_INHERIT).toEqual({
      show_waveform: true,
      show_cancel: true,
    });
  });

  // The one token whose inherit is not a single number. The card's edge is
  // stronger over glass and stronger again over Clear, so asking per Material
  // and Glass style keeps that rule out of every caller.
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

  // Spotlight's capsule carries a bright rim in both appearances. Clear is our
  // only surface dark enough in both for white to read, so only it inherits
  // white. Flat and Regular keep the foreground mix their Light card shows.
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

  // Every bound has an inherit and every inherit has a bound. One without the
  // other is a slider with no value, or a value no slider can show.
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
    // theme.css's two inks: #0f0f0f on light, #fbfbfb on dark.
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
      // The Glass tint reaches zero, the Flat opacity stops at 0.30. Untinted
      // glass is a look; an invisible Flat card is not.
      glass_tint: { min: 0.0, max: 1.0, step: 0.01 },
      border_opacity: { min: 0.0, max: 1.0, step: 0.01 },
      // The shadow reaches zero on both Materials: that is Flat's inherit, and
      // under Glass it turns macOS's own shadow off.
      shadow_strength: { min: 0.0, max: 1.0, step: 0.01 },
      shadow_offset_y: { min: 0, max: 16, step: 1 },
      size_scale: { min: 0.8, max: 1.5, step: 0.05 },
      radius: { min: 0, max: 32, step: 1 },
      border_width: { min: 0, max: 4, step: 1 },
      padding: { min: 0, max: 20, step: 1 },
      element_gap: { min: 0, max: 40, step: 1 },
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
          element_gap: 6,
          waveform_gap: 2,
          waveform_width: 5,
        }),
      ),
    ).toEqual({
      "--ov-scale": "1.1",
      "--ov-radius": "12px",
      "--ov-border-w": "2px",
      "--ov-pad": "14px",
      "--ov-elem-gap": "6px",
      "--ov-wave-gap": "2px",
      "--ov-wave-w": "5px",
    });
  });

  test("a zero border width still writes the property", () => {
    // 0 is a value, not "unset". Without the property the stylesheet's own 1px
    // comes back and the native window is two points wider than the card.
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
    // The tint is thin enough for the blur to read as blur; the edge is Flat's
    // foreground mix at a stronger alpha. Unset `glass_style` means Regular.
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
  // themes to carry Spotlight's white highlight. Only the edge moves. The
  // tint, neutrals and surface are the native style's business, not the card's.
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

  // The rim is Glass's alone. Under Flat the style has nothing to draw, so a
  // stored `glass_style` cannot leak a white edge onto a Flat card.
  test("the Glass style never reaches a Flat card's edge", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({ glass_style: "clear", border_opacity: 0.4 }),
      )["--s-border"],
    ).toBe("color-mix(in srgb, var(--s-text) 40%, transparent)");
  });

  // Rule 2 at the boundary. The localStorage mirror bypasses Rust, so a
  // hand-edited style falls back to what an unset token resolves to.
  test("an unreadable Glass style inherits Regular's edge", () => {
    expect(
      resolveOverlayThemeVars(
        resolved({ glass_style: "CLEAR" as never }, "glass"),
      )["--s-border"],
    ).toBe("color-mix(in srgb, var(--s-text) 25%, transparent)");
  });

  // The invariant the two constants above exist for. What an unset token
  // resolves to is what the card is painted with, for every Material and Glass
  // style. A default only the tab's slider knew about would never be drawn.
  //
  // Literals rather than a read-back from `inheritedBorder`, because
  // `inheritedTokenValue` delegates to it. Comparing the two would assert a
  // function against its own body and pass whatever the defaults became.
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
      // What the tab shows on both controls while the tokens are unset.
      expect(inheritedBorder(material, glassStyle)).toEqual({ color, opacity });
      expect(inheritedTokenValue("border_opacity", material, glassStyle)).toBe(
        opacity,
      );
      // And what the card paints. Flat writes no edge with all tokens unset
      // (removal rule), so the alpha is handed back to show both halves.
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

  /** Liquid Glass (macOS 26) paints the same card as the older blur. The engine
   *  picks the native view behind it and which of the two engine tokens
   *  applies, never what the card writes. Rust hands `NSGlassEffectView` that
   *  surface as its `tintColor`, from the same `surface`/`glass_tint` pair. */
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
  /** The bug the split exists for. An opaque Flat card used to follow the user
   *  into Glass and paint an opaque pane. */
  test("an opaque Flat card is still glass the moment Glass renders", () => {
    const opaqueFlat: Partial<OverlayTheme> = {
      surface: "#000000",
      surface_opacity: 1.0,
    };

    expect(resolveOverlayThemeVars(resolved(opaqueFlat))["--s-surface"]).toBe(
      "color-mix(in srgb, #000000 100%, transparent)",
    );
    // Same theme under Glass. The opacity is not read, so the tint is the
    // Glass default and the blur shows through.
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

    // A fully transparent tint is a value. Untinted glass is Apple's own look.
    expect(
      resolveOverlayThemeVars(resolved({ glass_tint: 0 }, "glass"))[
        "--s-surface"
      ],
    ).toBe("color-mix(in srgb, var(--color-background) 0%, transparent)");
  });

  test("the Glass tint writes nothing under Flat", () => {
    // Not even the surface property. Under Flat an untouched card is the
    // stylesheet's own 98%, and the tint has no say in it.
    expect(resolveOverlayThemeVars(resolved({ glass_tint: 0.15 }))).toEqual({});
  });

  test("the surface colour is shared: it is the tint colour under Glass", () => {
    const surfaceOnly = resolved({ surface: "#1a1b26" }, "glass");
    expect(resolveOverlayThemeVars(surfaceOnly)["--s-surface"]).toBe(
      "color-mix(in srgb, #1a1b26 45%, transparent)",
    );
    // …and it still picks the foreground, as under Flat.
    expect(resolveOverlayThemeVars(surfaceOnly)["--s-text"]).toBe("#fbfbfb");
  });
});

describe("the worked example", () => {
  /** The README's "A full theme", all twenty-two tokens set. */
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
    shadow_strength: 0.35,
    shadow_offset_y: 6,
    show_waveform: true,
    show_cancel: false,
    size_scale: 1.1,
    radius: 12,
    border_width: 1,
    padding: 14,
    element_gap: 8,
    waveform_style: "ribbon",
    waveform_gap: 2,
    waveform_width: 4,
  };

  // The README's full theme file and the CSS it resolves to. Its
  // `material: "glass"` is read with the Flat neutrals, so this is the
  // downgraded rendering (Glass percentages are covered above) and why
  // `--s-surface` carries the 92% Flat opacity, not the 45% tint.
  // `glass_material` and `glass_style` write no CSS, each setting a native
  // view property, and `waveform_style` writes none, a renderer the card picks,
  // not a value painted. Neutrals mix from `var(--s-text)`, resolving to the
  // `--s-text` beside them, while a set `border` replaces it for the edge.
  test("resolves to exactly the twenty-one properties listed", () => {
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
      "--ov-elem-gap": "8px",
      "--ov-wave-gap": "2px",
      "--ov-wave-w": "4px",
      // show_cancel: false takes the row's side floor and one of its two
      // element gaps with the button, the right column being gone.
      "--ov-side-min": "0px",
      "--ov-row-gaps": "1",
      // The Flat shadow at the example's own strength and offset, its slack
      // ceil((20 + 6) * 1.1) = 29.
      "--ov-shadow-strength": "0.35",
      "--ov-shadow-y": "6px",
      "--ov-shadow-slack": "29px",
      // …capped on the anchored side at the 15 points the fixture's payload
      // says the card has to the usable edge.
      "--ov-shadow-edge-slack": "15px",
    });
  });

  test("every property it writes is registered for removal", () => {
    // Every token set means the module writes all it can, so the lists must
    // match. One missing from OVERLAY_THEME_CSS_PROPERTIES paints on after its
    // token returns to inherit; one listed but never written is dead weight.
    // Read under Flat, the Material that draws its own shadow; under Glass the
    // four shadow properties are deliberately absent (macOS draws it).
    const written = Object.keys(resolveOverlayThemeVars(resolved(FULL_THEME)));
    for (const property of written) {
      expect(OVERLAY_THEME_CSS_PROPERTIES).toContain(property);
    }
    expect([...written].sort()).toEqual(
      [...OVERLAY_THEME_CSS_PROPERTIES].sort(),
    );
  });
});

describe("the shadow", () => {
  test("Flat writes the strength, the offset and a slack it has ceiled", () => {
    expect(resolveOverlayThemeVars(resolved({ shadow_strength: 0.5 }))).toEqual(
      {
        "--ov-shadow-strength": "0.5",
        // The offset falls back to its inherit, so the slack is 20 + 4.
        "--ov-shadow-y": "4px",
        "--ov-shadow-slack": "24px",
        // …and the anchored screen edge gets the 15 points the fixture's
        // payload says the card has above the Dock.
        "--ov-shadow-edge-slack": "15px",
      },
    );

    expect(
      resolveOverlayThemeVars(
        resolved({ shadow_strength: 1, shadow_offset_y: 16 }),
      )["--ov-shadow-slack"],
    ).toBe("36px");
  });

  test("the slack is the reach scaled and rounded up, never a fraction", () => {
    // CSS cannot ceil and the native window inset the card by an integer, so
    // the apply layer is the one that rounds. 0.8 x 24 is 19.2.
    for (const [scale, offset, slack] of [
      [0.8, 4, "20px"],
      [1, 0, "20px"],
      [1.1, 6, "29px"],
      [1.5, 4, "36px"],
      [1.5, 16, "54px"],
    ] as const) {
      expect(
        resolveOverlayThemeVars(
          resolved({
            shadow_strength: 0.4,
            shadow_offset_y: offset,
            size_scale: scale,
          }),
        )["--ov-shadow-slack"],
      ).toBe(slack);
    }
    expect(SHADOW_BLUR_PX).toBe(20);
  });

  test("no shadow means no properties at all, which is what Flat inherits", () => {
    expect(resolveOverlayThemeVars(resolved({}))).toEqual({});
    expect(
      resolveOverlayThemeVars(
        resolved({ shadow_strength: 0, shadow_offset_y: 12 }),
      ),
    ).toEqual({});
  });

  test("Glass writes none of them, because macOS draws that shadow", () => {
    const vars = resolveOverlayThemeVars(
      resolved({ shadow_strength: 1, shadow_offset_y: 8 }, "glass"),
    );
    expect(vars["--ov-shadow-strength"]).toBeUndefined();
    expect(vars["--ov-shadow-y"]).toBeUndefined();
    expect(vars["--ov-shadow-slack"]).toBeUndefined();
    expect(vars["--ov-shadow-edge-slack"]).toBeUndefined();

    // …and an unset strength under Glass inherits 1, still writing nothing.
    const inherited = resolveOverlayThemeVars(resolved({}, "glass"));
    expect(inherited["--ov-shadow-slack"]).toBeUndefined();
  });

  test("the anchored edge takes the number Rust derived, never its own", () => {
    // The one number this module cannot work out: only Rust knows the gap the
    // card has to the Dock, the taskbar or the menu bar, and the native window
    // was sized and placed from that same integer. Getting it wrong moves the
    // card the moment a shadow is switched on.
    const withRoom = (room: number) => {
      const payload = resolved({ shadow_strength: 0.5 });
      return resolveOverlayThemeVars({ ...payload, shadow_edge_slack: room })[
        "--ov-shadow-edge-slack"
      ];
    };

    // macOS Bottom, macOS Top, Windows and Linux, and a screen edge with no
    // room left at all.
    expect(withRoom(15)).toBe("15px");
    expect(withRoom(16)).toBe("16px");
    expect(withRoom(4)).toBe("4px");
    expect(withRoom(0)).toBe("0px");
    // Never more than the slack itself, whatever arrives.
    expect(withRoom(400)).toBe("24px");
    expect(withRoom(-8)).toBe("0px");
    // A mirror written before this field existed falls back to the full slack,
    // as the other three sides do (rule 2).
    const payload = resolved({ shadow_strength: 0.5 });
    delete (payload as { shadow_edge_slack?: number }).shadow_edge_slack;
    expect(resolveOverlayThemeVars(payload)["--ov-shadow-edge-slack"]).toBe(
      "24px",
    );
  });

  test("a Glass theme downgraded to Flat draws Flat's shadow", () => {
    // `effective_material` decides, as for every other property. A Mac that
    // cannot render Glass shows the Flat card, so it shows the CSS shadow the
    // theme asked for.
    expect(
      resolveOverlayThemeVars(
        resolved({ material: "glass", shadow_strength: 0.6 }, "flat"),
      )["--ov-shadow-strength"],
    ).toBe("0.6");
  });
});

describe("the row's own tokens", () => {
  test("the element gap is written raw, like every other length", () => {
    expect(resolveOverlayThemeVars(resolved({ element_gap: 12 }))).toEqual({
      "--ov-elem-gap": "12px",
    });
    // Zero is a value, not "unset": it is the gap the stylesheet already has,
    // but writing it keeps the removal rule honest.
    expect(resolveOverlayThemeVars(resolved({ element_gap: 0 }))).toEqual({
      "--ov-elem-gap": "0px",
    });
  });

  test("hiding the cancel button drops the row's side floor and a gap", () => {
    // The right column goes with the button, so the row is two tracks with one
    // gap between them, not three with two.
    expect(resolveOverlayThemeVars(resolved({ show_cancel: false }))).toEqual({
      "--ov-side-min": "0px",
      "--ov-row-gaps": "1",
    });
    // Shown, whether by inherit or an explicit true, is the stylesheet's own
    // 22px and two gaps, so nothing is written and no CSS is duplicated here.
    expect(resolveOverlayThemeVars(resolved({ show_cancel: true }))).toEqual(
      {},
    );
    expect(resolveOverlayThemeVars(resolved({}))).toEqual({});
  });

  test("hiding the waveform is markup only, no custom property", () => {
    // The card renders no `.swave` and takes the `nowave` width rule; there is
    // nothing for the apply layer to write.
    expect(resolveOverlayThemeVars(resolved({ show_waveform: false }))).toEqual(
      {},
    );
  });

  test("a switch is a switch only when it is a boolean", () => {
    // The card reads these two rather than a custom property, so they are
    // re-validated where it reads them.
    for (const key of ["show_waveform", "show_cancel"] as const) {
      expect(switchToken({ ...INHERIT_ALL, [key]: false }, key)).toBe(false);
      expect(switchToken({ ...INHERIT_ALL, [key]: true }, key)).toBe(true);
      expect(switchToken(INHERIT_ALL, key)).toBe(BOOLEAN_INHERIT[key]);
    }
  });

  test("the waveform style is markup only, and only ever one of the six", () => {
    // Like the switches, the card reads it rather than a custom property, and
    // a value the renderer table has no entry for would draw nothing at all.
    for (const style of WAVEFORM_STYLES) {
      expect(
        resolveOverlayThemeVars(resolved({ waveform_style: style })),
      ).toEqual({});
      expect(
        waveformStyleToken({ ...INHERIT_ALL, waveform_style: style }),
      ).toBe(style);
    }
    expect(waveformStyleToken(INHERIT_ALL)).toBe(WAVEFORM_STYLE_INHERIT);
    expect(WAVEFORM_STYLE_INHERIT).toBe("bars");
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
    // Payload-shaped, but every value is one Rust would never produce.
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
        "element_gap": 99,
        "waveform_gap": null,
        "waveform_width": 1,
        "shadow_strength": 9,
        "shadow_offset_y": "6px",
        "show_waveform": "false",
        "show_cancel": 0,
        "waveform_style": "spectrum"
      },
      "effective_material": "opaque",
      "glass_support": { "supported": true, "available": true, "engine": "liquid" },
      "file": null
    }`) as ResolvedOverlayTheme;

    // An out-of-range strength is unset, so no shadow is drawn rather than a
    // maximal one, and `"false"` is a string, not the switch it looks like.
    expect(resolveOverlayThemeVars(stale)).toEqual({});
    // The switches the card's markup reads take the same treatment: a string
    // and a 0 are not booleans, so both elements stay on the row.
    expect(switchToken(stale.theme, "show_waveform")).toBe(true);
    expect(switchToken(stale.theme, "show_cancel")).toBe(true);
    // A style this build cannot draw inherits the bars rather than reaching
    // for a renderer that is not there.
    expect(waveformStyleToken(stale.theme)).toBe("bars");

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
