import { OVERLAY_TOKEN_BOUNDS, type OverlayThemeKey } from "@/lib/overlayTheme";

export type OverlayTokenGroup = "color" | "material" | "size";

/**
 * The interface between the overlay-theme token contract and the Appearance
 * tab: one entry per token, published as data so `AppearanceSettings` can
 * render the Overlay Color / Overlay Material / Overlay Size & Spacing groups
 * as `fields.filter(f => f.group === …).map(renderField)` instead of
 * hardcoding nine rows by hand.
 *
 * Numeric bounds are pulled from `OVERLAY_TOKEN_BOUNDS` (the apply layer's own
 * re-validation table, `src/lib/overlayTheme.ts`) rather than repeated here,
 * so there is exactly one place a bound can be typed wrong.
 */
export type OverlayTokenField = {
  key: OverlayThemeKey;
  /** The custom property this token feeds; `material` has none of its own —
   *  it drives the `data-material` attribute instead of a CSS variable. */
  cssVar: `--${string}` | null;
  group: OverlayTokenGroup;
  labelKey: string;
  descriptionKey: string;
} & (
  | { kind: "color" }
  | { kind: "length"; min: number; max: number; step: number }
  | { kind: "factor"; min: number; max: number; step: number }
  | {
      kind: "enum";
      options: { value: string; labelKey: string; macOnly?: boolean }[];
    }
);

const TOKENS = "settings.appearance.tokens";
const MATERIAL = "settings.appearance.material";

/** Token order matches the contract's table (accent, surface, surface_opacity,
 *  text, material, size_scale, radius, padding, waveform_gap). */
export const OVERLAY_TOKEN_FIELDS: readonly OverlayTokenField[] = [
  {
    key: "accent",
    cssVar: "--s-accent",
    group: "color",
    kind: "color",
    labelKey: `${TOKENS}.accent.title`,
    descriptionKey: `${TOKENS}.accent.description`,
  },
  {
    key: "surface",
    cssVar: "--s-surface",
    group: "color",
    kind: "color",
    labelKey: `${TOKENS}.surface.title`,
    descriptionKey: `${TOKENS}.surface.description`,
  },
  {
    key: "surface_opacity",
    cssVar: null,
    group: "color",
    kind: "factor",
    ...OVERLAY_TOKEN_BOUNDS.surface_opacity,
    labelKey: `${TOKENS}.surfaceOpacity.title`,
    descriptionKey: `${TOKENS}.surfaceOpacity.description`,
  },
  {
    key: "text",
    cssVar: "--s-text",
    group: "color",
    kind: "color",
    labelKey: `${TOKENS}.text.title`,
    descriptionKey: `${TOKENS}.text.description`,
  },
  // Material is a single enum token, but — unlike the others — it is rendered
  // by a dedicated MaterialSelector (platform gating, Reduce Transparency
  // note) rather than a generic enum control. It still lives in this table so
  // the "material" group is driven by the same filter/map as the others.
  {
    key: "material",
    cssVar: null,
    group: "material",
    kind: "enum",
    options: [
      { value: "flat", labelKey: `${MATERIAL}.options.flat` },
      {
        value: "glass",
        labelKey: `${MATERIAL}.options.glass`,
        macOnly: true,
      },
    ],
    labelKey: `${MATERIAL}.title`,
    descriptionKey: `${MATERIAL}.description`,
  },
  {
    key: "size_scale",
    cssVar: "--ov-scale",
    group: "size",
    kind: "factor",
    ...OVERLAY_TOKEN_BOUNDS.size_scale,
    labelKey: `${TOKENS}.sizeScale.title`,
    descriptionKey: `${TOKENS}.sizeScale.description`,
  },
  {
    key: "radius",
    cssVar: "--ov-radius",
    group: "size",
    kind: "length",
    ...OVERLAY_TOKEN_BOUNDS.radius,
    labelKey: `${TOKENS}.radius.title`,
    descriptionKey: `${TOKENS}.radius.description`,
  },
  {
    key: "padding",
    cssVar: "--ov-pad-x",
    group: "size",
    kind: "length",
    ...OVERLAY_TOKEN_BOUNDS.padding,
    labelKey: `${TOKENS}.padding.title`,
    descriptionKey: `${TOKENS}.padding.description`,
  },
  {
    key: "waveform_gap",
    cssVar: "--ov-wave-gap",
    group: "size",
    kind: "length",
    ...OVERLAY_TOKEN_BOUNDS.waveform_gap,
    labelKey: `${TOKENS}.waveformGap.title`,
    descriptionKey: `${TOKENS}.waveformGap.description`,
  },
] as const;
