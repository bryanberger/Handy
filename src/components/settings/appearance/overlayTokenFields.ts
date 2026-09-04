import type { Material } from "@/bindings";
import {
  OVERLAY_TOKEN_BOUNDS,
  type OverlayBooleanKey,
  type OverlayColorKey,
  type OverlayNumericKey,
} from "@/lib/overlayTheme";

/** The four groups the tab renders token rows in, in display order. Listed
 *  once so anything walking every row on screen, like the theme file's
 *  "sets N of M values" line, cannot miss a group. */
export const OVERLAY_TOKEN_GROUPS = [
  "color",
  "material",
  "elements",
  "size",
] as const;

export type OverlayTokenGroup = (typeof OVERLAY_TOKEN_GROUPS)[number];

interface OverlayTokenFieldBase {
  group: OverlayTokenGroup;
  labelKey: string;
  descriptionKey: string;
  /**
   * The one Material this row belongs to; absent means both, true of every row
   * but the card's alpha. `surface_opacity` paints the Flat card and
   * `glass_tint` tints the glass, so showing both would give two controls for
   * one line of CSS, one inert on screen. See [`overlayTokenFieldsFor`].
   */
  onlyUnder?: Material;
}

/**
 * The overlay-theme token contract as data for the Appearance tab, one entry
 * per token. Instead of a hand-written row each, `AppearanceSettings` renders
 * the Overlay Color / Overlay Material / Overlay Elements / Overlay Size &
 * Spacing groups as `fields.filter(f => f.group === …).map(renderField)`.
 *
 * A discriminated union on `kind`, so `key` narrows with it. A color field's
 * key is one of the four color tokens, a length/factor field's one of the
 * twelve numeric ones, a toggle's one of the two switches, and the two enum
 * fields carry their own kind, so `renderField` reads `effectiveValue(field.key)`
 * at the right type per branch, not a cast.
 *
 * Numeric bounds come from `OVERLAY_TOKEN_BOUNDS` (the apply layer's
 * re-validation table, `src/lib/overlayTheme.ts`) rather than a copy here, so
 * a bound can only be typed wrong in one place.
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
  | (OverlayTokenFieldBase & { kind: "toggle"; key: OverlayBooleanKey })
  | (OverlayTokenFieldBase & {
      kind: "glassShadow";
      key: "shadow_strength";
      /** The note under the switch: macOS draws the Glass shadow, so the
       *  strength is on or off and the offset does nothing. */
      noteKey: string;
    })
  | (OverlayTokenFieldBase & { kind: "material"; key: "material" })
  | (OverlayTokenFieldBase & { kind: "glassStyle"; key: "glass_style" });

const TOKENS = "settings.appearance.tokens";
const MATERIAL = "settings.appearance.material";
const GLASS_STYLE = "settings.appearance.glassStyle";

/** Token order matches the contract's table, the same order the theme file's
 *  `TOKENS` table lists: accent, surface, surface_opacity, glass_tint, text,
 *  border, border_opacity, material, glass_material, glass_style,
 *  shadow_strength, shadow_offset_y, show_waveform, show_cancel, size_scale,
 *  radius, border_width, padding, element_gap, waveform_gap, waveform_width.
 *  That is also the display order, since every group's rows are contiguous in
 *  it. `glass_material` is the only token with no row, since it drives the
 *  pre-macOS-26 fallback engine alone and is set from the theme file.
 *
 *  `shadow_strength` has two rows, one per Material, the shape the two card
 *  alphas already use. Under Flat it is a slider over a CSS shadow this
 *  stylesheet draws; under Glass the shadow is macOS's own and `NSWindow`
 *  offers no strength at all, so the only honest control is a switch. */
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
  // The Flat card's alpha then the Glass tint's, in that order and never both
  // on screen. Each is the other's counterpart on the Material not in view.
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
  // Material is a single enum token, but a dedicated MaterialSelector renders
  // it and owns the one declaration of the Flat/Glass options (platform
  // gating and the unavailable note need them anyway). It stays in this table
  // so the "material" group runs off the same filter/map as the others.
  {
    key: "material",
    group: "material",
    kind: "material",
    labelKey: `${MATERIAL}.title`,
    descriptionKey: `${MATERIAL}.description`,
  },
  // The Glass style matters only while Material is Glass and Liquid Glass is
  // the engine, so its own selector owns that rule and the two option labels.
  {
    key: "glass_style",
    group: "material",
    kind: "glassStyle",
    labelKey: `${GLASS_STYLE}.title`,
    descriptionKey: `${GLASS_STYLE}.description`,
  },
  // The shadow, after the Glass style: how the card sits against the desktop
  // is the Material group's subject, and the shadow's behaviour is the clearest
  // per-Material difference there is.
  {
    key: "shadow_strength",
    group: "material",
    kind: "factor",
    onlyUnder: "flat",
    ...OVERLAY_TOKEN_BOUNDS.shadow_strength,
    labelKey: `${TOKENS}.shadowStrength.title`,
    descriptionKey: `${TOKENS}.shadowStrength.description`,
  },
  {
    key: "shadow_strength",
    group: "material",
    kind: "glassShadow",
    onlyUnder: "glass",
    labelKey: `${TOKENS}.shadowStrength.title`,
    descriptionKey: `${TOKENS}.shadowStrength.glassDescription`,
    noteKey: `${TOKENS}.shadowStrength.glassNote`,
  },
  // Flat only: macOS places its own window shadow and takes no offset, so the
  // row would be inert under Glass rather than merely unused.
  {
    key: "shadow_offset_y",
    group: "material",
    kind: "length",
    onlyUnder: "flat",
    ...OVERLAY_TOKEN_BOUNDS.shadow_offset_y,
    labelKey: `${TOKENS}.shadowOffsetY.title`,
    descriptionKey: `${TOKENS}.shadowOffsetY.description`,
  },
  {
    key: "show_waveform",
    group: "elements",
    kind: "toggle",
    labelKey: `${TOKENS}.showWaveform.title`,
    descriptionKey: `${TOKENS}.showWaveform.description`,
  },
  {
    key: "show_cancel",
    group: "elements",
    kind: "toggle",
    labelKey: `${TOKENS}.showCancel.title`,
    descriptionKey: `${TOKENS}.showCancel.description`,
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
    key: "element_gap",
    group: "size",
    kind: "length",
    ...OVERLAY_TOKEN_BOUNDS.element_gap,
    labelKey: `${TOKENS}.elementGap.title`,
    descriptionKey: `${TOKENS}.elementGap.description`,
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
 * Keyed on the effective Material, what is painted, not what was asked for.
 * Where Glass cannot render, the Flat card is on screen, so Flat's opacity is
 * the live control. The apply layer picks its alpha the same way.
 *
 * Pure, and the only place the "never both alphas", "one shadow control per
 * Material" and "no shadow offset under Glass" rules are written down.
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
