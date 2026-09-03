import {
  OVERLAY_TOKEN_BOUNDS,
  type OverlayColorKey,
  type OverlayNumericKey,
} from "@/lib/overlayTheme";

export type OverlayTokenGroup = "color" | "material" | "size";

interface OverlayTokenFieldBase {
  group: OverlayTokenGroup;
  labelKey: string;
  descriptionKey: string;
}

/**
 * The interface between the overlay-theme token contract and the Appearance
 * tab: one entry per token, published as data so `AppearanceSettings` can
 * render the Overlay Color / Overlay Material / Overlay Size & Spacing groups
 * as `fields.filter(f => f.group === …).map(renderField)` instead of
 * hardcoding nine rows by hand.
 *
 * A discriminated union on `kind`, so the token's `key` narrows with it: a
 * color field's key is one of the three color tokens, a length/factor field's
 * is one of the five numeric ones, and the single enum field is Material.
 * That is what lets `renderField` read `effectiveValue(field.key)` at the
 * right type in each branch instead of asserting it back with a cast.
 *
 * Numeric bounds are pulled from `OVERLAY_TOKEN_BOUNDS` (the apply layer's own
 * re-validation table, `src/lib/overlayTheme.ts`) rather than repeated here,
 * so there is exactly one place a bound can be typed wrong.
 */
interface NumericTokenFieldBase extends OverlayTokenFieldBase {
  key: OverlayNumericKey;
  min: number;
  max: number;
  step: number;
}

export type OverlayTokenField =
  | (OverlayTokenFieldBase & { kind: "color"; key: OverlayColorKey })
  | (NumericTokenFieldBase & { kind: "length" })
  | (NumericTokenFieldBase & { kind: "factor" })
  | (OverlayTokenFieldBase & { kind: "enum"; key: "material" });

const TOKENS = "settings.appearance.tokens";
const MATERIAL = "settings.appearance.material";

/** Token order matches the contract's table (accent, surface, surface_opacity,
 *  text, material, size_scale, radius, padding, waveform_gap). */
export const OVERLAY_TOKEN_FIELDS: readonly OverlayTokenField[] = [
  {
    key: "accent",
    group: "color",
    kind: "color",
    labelKey: `${TOKENS}.accent.title`,
    descriptionKey: `${TOKENS}.accent.description`,
  },
  {
    key: "surface",
    group: "color",
    kind: "color",
    labelKey: `${TOKENS}.surface.title`,
    descriptionKey: `${TOKENS}.surface.description`,
  },
  {
    key: "surface_opacity",
    group: "color",
    kind: "factor",
    ...OVERLAY_TOKEN_BOUNDS.surface_opacity,
    labelKey: `${TOKENS}.surfaceOpacity.title`,
    descriptionKey: `${TOKENS}.surfaceOpacity.description`,
  },
  {
    key: "text",
    group: "color",
    kind: "color",
    labelKey: `${TOKENS}.text.title`,
    descriptionKey: `${TOKENS}.text.description`,
  },
  // Material is a single enum token, but — unlike the others — it is rendered
  // by a dedicated MaterialSelector, which owns the one declaration of the
  // Flat/Glass options (platform gating and the unavailable note need them
  // anyway). It still lives in this table so the "material" group is driven
  // by the same filter/map as the others.
  {
    key: "material",
    group: "material",
    kind: "enum",
    labelKey: `${MATERIAL}.title`,
    descriptionKey: `${MATERIAL}.description`,
  },
  {
    key: "size_scale",
    group: "size",
    kind: "factor",
    ...OVERLAY_TOKEN_BOUNDS.size_scale,
    labelKey: `${TOKENS}.sizeScale.title`,
    descriptionKey: `${TOKENS}.sizeScale.description`,
  },
  {
    key: "radius",
    group: "size",
    kind: "length",
    ...OVERLAY_TOKEN_BOUNDS.radius,
    labelKey: `${TOKENS}.radius.title`,
    descriptionKey: `${TOKENS}.radius.description`,
  },
  {
    key: "padding",
    group: "size",
    kind: "length",
    ...OVERLAY_TOKEN_BOUNDS.padding,
    labelKey: `${TOKENS}.padding.title`,
    descriptionKey: `${TOKENS}.padding.description`,
  },
  {
    key: "waveform_gap",
    group: "size",
    kind: "length",
    ...OVERLAY_TOKEN_BOUNDS.waveform_gap,
    labelKey: `${TOKENS}.waveformGap.title`,
    descriptionKey: `${TOKENS}.waveformGap.description`,
  },
] as const;
