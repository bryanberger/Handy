import type { Material } from "@/bindings";
import {
  OVERLAY_TOKEN_BOUNDS,
  type OverlayColorKey,
  type OverlayNumericKey,
} from "@/lib/overlayTheme";

/** The three groups the tab renders token rows in, in the order they appear.
 *  Listed once so anything that has to walk *every* row on screen — the theme
 *  file's "sets N of M values" line — cannot quietly miss a group. */
export const OVERLAY_TOKEN_GROUPS = ["color", "material", "size"] as const;

export type OverlayTokenGroup = (typeof OVERLAY_TOKEN_GROUPS)[number];

interface OverlayTokenFieldBase {
  group: OverlayTokenGroup;
  labelKey: string;
  descriptionKey: string;
  /**
   * The one Material this row belongs to, or absent for a row that belongs to
   * both — which is all of them but the card's alpha. `surface_opacity`
   * paints the Flat card and `glass_tint` tints the glass, so showing both at
   * once would offer two controls for one line of CSS, one of which does
   * nothing on the Material in front of the user. See
   * [`overlayTokenFieldsFor`].
   */
  onlyUnder?: Material;
}

/**
 * The interface between the overlay-theme token contract and the Appearance
 * tab: one entry per token, published as data so `AppearanceSettings` can
 * render the Overlay Color / Overlay Material / Overlay Size & Spacing groups
 * as `fields.filter(f => f.group === …).map(renderField)` instead of
 * hardcoding a row per token by hand.
 *
 * A discriminated union on `kind`, so the token's `key` narrows with it: a
 * color field's key is one of the four color tokens, a length/factor field's
 * is one of the nine numeric ones, and the two enum fields carry their own
 * kind each. That is what lets `renderField` read `effectiveValue(field.key)`
 * at the right type in each branch instead of asserting it back with a cast.
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
  | (OverlayTokenFieldBase & { kind: "material"; key: "material" })
  | (OverlayTokenFieldBase & { kind: "glassStyle"; key: "glass_style" });

const TOKENS = "settings.appearance.tokens";
const MATERIAL = "settings.appearance.material";
const GLASS_STYLE = "settings.appearance.glassStyle";

/** Token order matches the contract's table, which is also the order the
 *  theme file's `TOKEN_KEYS` lists them in: accent, surface, surface_opacity,
 *  glass_tint, text, border, border_opacity, material, glass_material,
 *  glass_style, size_scale, radius, border_width, padding, waveform_gap,
 *  waveform_width. `glass_material` is the one token with no row: it drives
 *  the pre-macOS-26 fallback engine only, and is set from the theme file. */
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
    onlyUnder: "flat",
    ...OVERLAY_TOKEN_BOUNDS.surface_opacity,
    labelKey: `${TOKENS}.surfaceOpacity.title`,
    descriptionKey: `${TOKENS}.surfaceOpacity.description`,
  },
  // The Flat card's alpha and the Glass tint's, in that order and never both
  // on screen at once: one is the other's counterpart on the Material the
  // user is not looking at.
  {
    key: "glass_tint",
    group: "color",
    kind: "factor",
    onlyUnder: "glass",
    ...OVERLAY_TOKEN_BOUNDS.glass_tint,
    labelKey: `${TOKENS}.glassTint.title`,
    descriptionKey: `${TOKENS}.glassTint.description`,
  },
  {
    key: "text",
    group: "color",
    kind: "color",
    labelKey: `${TOKENS}.text.title`,
    descriptionKey: `${TOKENS}.text.description`,
  },
  {
    key: "border",
    group: "color",
    kind: "color",
    labelKey: `${TOKENS}.border.title`,
    descriptionKey: `${TOKENS}.border.description`,
  },
  {
    key: "border_opacity",
    group: "color",
    kind: "factor",
    ...OVERLAY_TOKEN_BOUNDS.border_opacity,
    labelKey: `${TOKENS}.borderOpacity.title`,
    descriptionKey: `${TOKENS}.borderOpacity.description`,
  },
  // Material is a single enum token, but — unlike the others — it is rendered
  // by a dedicated MaterialSelector, which owns the one declaration of the
  // Flat/Glass options (platform gating and the unavailable note need them
  // anyway). It still lives in this table so the "material" group is driven
  // by the same filter/map as the others.
  {
    key: "material",
    group: "material",
    kind: "material",
    labelKey: `${MATERIAL}.title`,
    descriptionKey: `${MATERIAL}.description`,
  },
  // The Glass style only means anything while Material is Glass and Liquid
  // Glass is the engine, so its own selector owns that rule as well as the
  // two option labels.
  {
    key: "glass_style",
    group: "material",
    kind: "glassStyle",
    labelKey: `${GLASS_STYLE}.title`,
    descriptionKey: `${GLASS_STYLE}.description`,
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
    key: "border_width",
    group: "size",
    kind: "length",
    ...OVERLAY_TOKEN_BOUNDS.border_width,
    labelKey: `${TOKENS}.borderWidth.title`,
    descriptionKey: `${TOKENS}.borderWidth.description`,
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
  {
    key: "waveform_width",
    group: "size",
    kind: "length",
    ...OVERLAY_TOKEN_BOUNDS.waveform_width,
    labelKey: `${TOKENS}.waveformWidth.title`,
    descriptionKey: `${TOKENS}.waveformWidth.description`,
  },
] as const;

/**
 * The rows a group shows under one Material, in contract order.
 *
 * Keyed on the **effective** Material — what is actually painted — not on the
 * token as requested: on a machine where Glass cannot render, the Flat card is
 * on screen, so Flat's own opacity is the control that means something there.
 * The same reasoning the apply layer uses to pick which alpha it reads.
 *
 * Pure, and the only place the "never both alphas" rule is written down.
 */
export function overlayTokenFieldsFor(
  group: OverlayTokenGroup,
  material: Material,
): readonly OverlayTokenField[] {
  return OVERLAY_TOKEN_FIELDS.filter(
    (field) =>
      field.group === group && (field.onlyUnder ?? material) === material,
  );
}
