import type {
  GlassStyle,
  Material,
  OverlayTheme,
  ResolvedOverlayTheme,
  WaveformStyle,
} from "@/bindings";

/**
 * The apply layer, the one module turning a resolved overlay theme into the
 * overlay's CSS custom properties and its material attribute.
 *
 * Both callers use it, so they cannot drift:
 *  - the overlay window writes the map onto `document.documentElement`;
 *  - the Appearance tab's preview writes the same map inline on its wrapper.
 *
 * Hard constraint. The overlay entry loads this module, and that entry is the
 * memory-sensitive webview (#1279). It must NOT import React, i18next or the
 * settings store. Pure functions, constants and localStorage only; its one
 * import is type-only and vanishes at build time.
 *
 * Two rules govern the whole file.
 *  1. Removal. A property is written only while its token is set, and
 *     [`applyOverlayTheme`] removes the rest. Inline style beats any
 *     stylesheet, so one never removed would survive a reset to inherit.
 *  2. Re-validation at the boundary. Rust canonicalises colours and clamps
 *     numbers, but the localStorage mirror below bypasses Rust and a user
 *     can edit it, so values are re-checked before CSS; failures are unset.
 */

/** A token name, both an `OverlayTheme` field and a theme-file key. */
export type OverlayThemeKey = keyof OverlayTheme;

/** The four tokens whose value is a colour. */
export type OverlayColorKey = "accent" | "surface" | "text" | "border";

/** The twelve tokens whose value is a number. */
export type OverlayNumericKey =
  | "surface_opacity"
  | "glass_tint"
  | "border_opacity"
  | "shadow_strength"
  | "shadow_offset_y"
  | "size_scale"
  | "radius"
  | "border_width"
  | "padding"
  | "element_gap"
  | "waveform_gap"
  | "waveform_width";

/** The two tokens whose value is a switch. */
export type OverlayBooleanKey = "show_waveform" | "show_cancel";

/**
 * The six waveform styles in the contract's order, the order the Appearance
 * tab's dropdown lists them and `WaveformStyle::ALL` declares them in
 * `src-tauri/src/overlay_theme.rs`.
 *
 * Here rather than beside the renderers because it is the token's value list,
 * the same fact as [`OVERLAY_TOKEN_BOUNDS`] for a number, and what the
 * re-validation below checks. Which draw on a canvas, and which lengths each
 * reads, is renderer knowledge in `src/overlay/waveform/waveformStyles.ts`.
 */
export const WAVEFORM_STYLES: readonly WaveformStyle[] = [
  "bars",
  "ribbon",
  "bloom",
  "motes",
  "matrix",
  "steps",
];

/**
 * The numeric tokens' bounds, the same numbers Rust clamps to. Read by the
 * re-validation below, which treats an out-of-range value as unset, and by the
 * Appearance tab's sliders. Every px value is at size scale 1; the scale
 * multiplies it in CSS.
 */
export const OVERLAY_TOKEN_BOUNDS: Record<
  OverlayNumericKey,
  { min: number; max: number; step: number }
> = {
  surface_opacity: { min: 0.3, max: 1.0, step: 0.01 },
  glass_tint: { min: 0.0, max: 1.0, step: 0.01 },
  border_opacity: { min: 0.0, max: 1.0, step: 0.01 },
  shadow_strength: { min: 0.0, max: 1.0, step: 0.01 },
  shadow_offset_y: { min: 0, max: 16, step: 1 },
  size_scale: { min: 0.8, max: 1.5, step: 0.05 },
  radius: { min: 0, max: 32, step: 1 },
  border_width: { min: 0, max: 4, step: 1 },
  padding: { min: 0, max: 20, step: 1 },
  element_gap: { min: 0, max: 40, step: 1 },
  waveform_gap: { min: 0, max: 5, step: 1 },
  waveform_width: { min: 2, max: 6, step: 1 },
};

/** The theme that inherits every token, Handy's overlay as it ships. */
export const INHERIT_ALL: OverlayTheme = {
  accent: null,
  surface: null,
  surface_opacity: null,
  glass_tint: null,
  text: null,
  border: null,
  border_opacity: null,
  material: null,
  glass_material: null,
  glass_style: null,
  shadow_strength: null,
  shadow_offset_y: null,
  show_waveform: null,
  show_cancel: null,
  size_scale: null,
  radius: null,
  border_width: null,
  padding: null,
  element_gap: null,
  waveform_style: null,
  waveform_gap: null,
  waveform_width: null,
};

/**
 * The card alpha an unset `surface_opacity` resolves to, today's near-opaque
 * card from `RecordingOverlay.css`. Flat's number only; Glass never reads the
 * token. See [`GLASS_TINT_INHERIT`] for why the two are separate tokens rather
 * than one value with a per-Material default.
 */
export const SURFACE_OPACITY_INHERIT = 0.98;

/**
 * The tint alpha an unset `glass_tint` resolves to, on both engines.
 *
 * 0.45 is measured. Over a split light/dark striped desktop the card passes
 * about 53 backdrop levels at 0.45, against 27 at the 0.70 first shipped, a
 * dark card against frosted glass. Worst-case transcript contrast over the
 * brightest backdrop holds 6.1:1, past WCAG AA; 0.30 buys 10 levels more but
 * drops it to 4.9:1, which a pure-white desktop pushes under the line.
 *
 * Liquid Glass (macOS 26), same desktop, keeps 0.45. At 0.30 the worst case
 * falls to 4.3:1 under a Light app theme, under WCAG AA; 0.45 holds 5.6-9.6:1
 * across both Glass styles and both app themes. Rust builds the native
 * `tintColor` from the same number (`GLASS_TINT_INHERIT` in
 * `src-tauri/src/overlay_theme.rs`).
 *
 * Measured a third time on macOS 26 against Spotlight's capsule and kept for
 * both Glass styles. Spotlight sits between them, its level over dark content
 * Clear's (34.6 against Clear's 39.4 here), its transmission Regular's (20.3 %
 * against Clear's 38.8 %). Over dark content the tint barely moves the Clear
 * card, 32 at 0.00 through 43 at 0.45 with Spotlight at 36, so level picks no
 * number; transmission does, and wants the tint *higher*, since reaching
 * Spotlight's 20 % takes about 0.77, no longer glass. Dropping to 0.25 instead
 * takes Clear's worst-case transcript contrast from 5.4:1 to 4.0:1, under WCAG
 * AA. So the Spotlight-shaped frost is out of reach for a webview over
 * `NSGlassEffectView`, and Clear takes only Spotlight's rim
 * ([`BORDER_INHERIT_CLEAR`]) and shadow (`overlay_glass::window_shadow`).
 *
 * Why Glass has its own token. While both Materials shared `surface_opacity`
 * they were mutually exclusive in practice. A card set opaque under Flat
 * stayed opaque under Glass, so Glass looked broken the first time it was
 * chosen and nothing on screen said why. Each Material now carries its own
 * alpha, so switching lands on that Material's own value and Glass is glass at
 * once.
 */
export const GLASS_TINT_INHERIT = 0.45;

/**
 * What an unset `border` mixes from everywhere except Clear glass, the
 * foreground. One value, not a per-Material pair, because a white rim of the
 * Apple HUD kind was measured and rejected for Flat and for Regular glass. It
 * only works under a Dark app theme. Over a light card (Light theme, tint
 * near-white) a white edge at 30 % moves the pixels 3 levels, no edge at all,
 * against 27 for the foreground mix, and the default must be visible in all
 * four combinations of app theme and backdrop. Clear glass alone has a card
 * dark enough in both app themes for a white edge to read, so it has its own;
 * see [`BORDER_INHERIT_CLEAR`]. Otherwise only the alpha differs per Material;
 * see [`BORDER_OPACITY_INHERIT`].
 */
export const BORDER_INHERIT = "var(--s-text)";

/**
 * What an unset `border` mixes from under Clear glass. White in both app
 * themes, so the card carries a highlight, not a hairline. Measured against
 * Spotlight on macOS 26 over a split white/black desktop. Its capsule carries
 * a 1 pt bright rim in both appearances, 113 over a panel of 36 under Dark and
 * 238 over a panel of 183 under Light, a highlight in both, where the
 * foreground mix above is a highlight under Dark and a *dark* hairline under
 * Light, 26 levels *below* the card, the opposite of what a glass edge does.
 * Clear only. The rejection in [`BORDER_INHERIT`] was measured on Flat and on
 * Regular and is not revisited here. Clear is the style measured against
 * Spotlight and the one whose card is dark enough in both appearances for a
 * white edge to register (133 over dark content under Light, against
 * Regular's 151).
 */
export const BORDER_INHERIT_CLEAR = "#ffffff";

/**
 * The alpha an unset `border_opacity` resolves to, per Material, the second
 * half of [`BORDER_INHERIT`]. Flat's 0.12 is today's hairline strength;
 * Glass's is stronger because the edge is the only hard line a translucent
 * card has and the thinner tint above leaves it more work. On Liquid Glass
 * 0.25 measured as one 2 px transition of +45 levels over the card, reading as
 * one hairline beside the glass's own rim rather than a second line. Clear
 * glass takes [`BORDER_OPACITY_INHERIT_CLEAR`] instead.
 */
export const BORDER_OPACITY_INHERIT: Record<Material, number> = {
  flat: 0.12,
  glass: 0.25,
};

/**
 * The alpha an unset `border_opacity` resolves to under Clear glass, the
 * second half of [`BORDER_INHERIT_CLEAR`]. One number for both appearances, a
 * compromise because the two pull opposite ways. Measured on macOS 26 over the
 * split white/black desktop against Spotlight's own rim over dark content, in
 * levels above the card. Under Dark, where Spotlight is +77: 0.00 +42 (the
 * glass's own rim alone), 0.20 +76, 0.25 +85, 0.30 +94, 0.35 +103. Under
 * Light, where Spotlight is +55: 0.00 +3, 0.20 +25, 0.25 +32, 0.30 +37 (see
 * below), 0.35 +44.
 *
 * A later session filled in the Light 0.30 cell, hence its note. Its own 0.25
 * and 0.35 anchors read +33 and +42 there, against +32 and +44 here, because
 * the card's absolute level drifts between appearance sessions while the rim
 * delta barely does. That settles the value rather than moving it. At 0.30 the
 * Light rim is +37, the weaker appearance losing 7 more levels, while the Dark
 * rim still overshoots Spotlight by 17. So 0.35 stays.
 *
 * Dark alone would pick 0.20, Light alone something past 0.50, because Handy's
 * Clear card is lighter than Spotlight's panel under Dark (42 against 36) and
 * darker under Light (136 against 183), so the same white edge has different
 * room in each. 0.35 balances them, a little hot under Dark, short under
 * Light, a visible highlight in both, which the foreground mix is not.
 */
export const BORDER_OPACITY_INHERIT_CLEAR = 0.35;

/**
 * The alpha an unset `shadow_strength` resolves to, per Material, the second
 * token after the border's alpha whose inherit depends on it, and the only one
 * whose two inherits are the ends of its own range.
 *
 * Flat has never cast a shadow: its window is larger than its card and
 * transparent around it, so a window shadow would trace a rectangle nobody can
 * see, and no CSS shadow was drawn. Glass has always cast macOS's own, its
 * window being the card exactly. So each Material inherits its existing look,
 * the token adding a shadow to one and taking one away from the other.
 * `shadow_strength_inherit` in `src-tauri/src/overlay_theme.rs` is this pair.
 */
export const SHADOW_STRENGTH_INHERIT: Record<Material, number> = {
  flat: 0,
  glass: 1,
};

/**
 * The Flat card's drop-shadow blur radius, px at size scale 1, and the
 * `--ov-shadow-blur` the stylesheet paints with.
 *
 * Derived, not a token: `shadow_strength` and `shadow_offset_y` are the
 * shadow's two controls, a third for the blur would make it a project, and a
 * non-black shadow is a tint the border tokens already cover.
 *
 * With the offset it is how far the shadow reaches from the card's edge,
 * exactly the window's shadow slack. `overlay_geometry.rs` holds the same
 * number as `CARD_SHADOW_BLUR` and pins both against this one, since both
 * sides must round the same product to the same integer.
 */
export const SHADOW_BLUR_PX = 20;

/** The Glass style an unset `glass_style` resolves to, re-validated. */
function effectiveGlassStyleOf(theme: OverlayTheme): GlassStyle {
  return theme.glass_style === "clear" ? "clear" : "regular";
}

/**
 * The colour and alpha an unset `border` and `border_opacity` resolve to. One
 * function, not two tables, because the halves move together, and Clear's
 * white rim is only right at Clear's own alpha. The Glass style is asked for
 * unconditionally and ignored under Flat, so no caller need know which
 * Material reads it.
 */
export function inheritedBorder(
  material: Material,
  glassStyle: GlassStyle,
): { color: string; opacity: number } {
  return material === "glass" && glassStyle === "clear"
    ? { color: BORDER_INHERIT_CLEAR, opacity: BORDER_OPACITY_INHERIT_CLEAR }
    : { color: BORDER_INHERIT, opacity: BORDER_OPACITY_INHERIT[material] };
}

/** Each numeric token's single-number inherit, from `RecordingOverlay.css`. */
const STATIC_NUMERIC_INHERIT: Record<
  Exclude<OverlayNumericKey, "border_opacity" | "shadow_strength">,
  number
> = {
  surface_opacity: SURFACE_OPACITY_INHERIT, // --s-surface's own 98%
  glass_tint: GLASS_TINT_INHERIT, // measured, not a CSS default
  shadow_offset_y: 4, // --ov-shadow-y
  size_scale: 1, // --ov-scale
  radius: 24, // --ov-radius
  border_width: 1, // --ov-border-w
  padding: 10, // --ov-pad
  element_gap: 0, // --ov-elem-gap
  waveform_gap: 3, // --ov-wave-gap
  waveform_width: 4, // --ov-wave-w
};

/**
 * What a numeric token resolves to while unset, the number a control shows and
 * the number the card is painted with. The Material is a parameter because one
 * token's inherit depends on it, the card's edge being stronger over glass;
 * asking for it unconditionally keeps callers from having to know which token
 * that is.
 *
 * The numbers live here beside the two alphas above, not in the Appearance
 * tab, so one module answers "what does an unset token inherit".
 * `overlay_token_inherit_values_match_the_css` in
 * `src-tauri/src/overlay_theme.rs` pins them to the `:root` block in
 * `RecordingOverlay.css`, the stylesheet that paints an unset token, so a
 * length changed in one place and not the other fails a test instead of
 * showing a wrong number on a slider. Callers get `border_opacity` from this
 * function, not a table, because its inherit differs per Material *and*, under
 * Glass, per Glass style; see [`inheritedBorder`].
 */
export function inheritedTokenValue(
  key: OverlayNumericKey,
  material: Material,
  glassStyle: GlassStyle,
): number {
  if (key === "border_opacity")
    return inheritedBorder(material, glassStyle).opacity;
  if (key === "shadow_strength") return SHADOW_STRENGTH_INHERIT[material];
  return STATIC_NUMERIC_INHERIT[key];
}

/** What a switch resolves to while unset: both elements on the row, today's
 *  card. No per-Material split, so a constant rather than a function of the
 *  Material like the two above. */
export const BOOLEAN_INHERIT: Record<OverlayBooleanKey, boolean> = {
  show_waveform: true,
  show_cancel: true,
};

/**
 * A switch token, re-validated like the numbers and colours: a non-boolean is
 * unset and inherits (rule 2). The overlay reads its switches from the
 * localStorage mirror at boot, bypassing Rust, so a hand-edited `"true"` or `1`
 * must not reach the card's markup.
 */
export function switchToken(
  theme: OverlayTheme,
  key: OverlayBooleanKey,
): boolean {
  const value = theme[key];
  return typeof value === "boolean" ? value : BOOLEAN_INHERIT[key];
}

/** What an unset `waveform_style` resolves to: today's nine capsules, the one
 *  style drawn as DOM elements, so an unset token costs nothing new. */
export const WAVEFORM_STYLE_INHERIT: WaveformStyle = "bars";

/**
 * The waveform style, re-validated like the switches, numbers and colours:
 * anything outside [`WAVEFORM_STYLES`] inherits (rule 2). The overlay reads it
 * from the localStorage mirror at boot, bypassing Rust, so a hand-edited value
 * must not reach a renderer table with no entry for it.
 */
export function waveformStyleToken(theme: OverlayTheme): WaveformStyle {
  const value = theme.waveform_style;
  return typeof value === "string" &&
    (WAVEFORM_STYLES as readonly string[]).includes(value)
    ? (value as WaveformStyle)
    : WAVEFORM_STYLE_INHERIT;
}

/**
 * The neutral group as a percentage of the foreground, per Material. Flat's
 * are reverse-engineered from today's hand-picked neutrals, so the derivation
 * lands on today's look in both app themes. Glass's are stronger because muted
 * and faint fail first over a blurred background, whichever engine blurs it.
 * The most eyeball-tunable numbers in the feature.
 */
const NEUTRALS: Record<
  Material,
  { muted: number; faint: number; hair: number }
> = {
  flat: { muted: 60, faint: 38, hair: 7 },
  glass: { muted: 78, faint: 52, hair: 12 },
};

/** The derived tint strength of `--s-accent-soft`, as a percentage. */
const ACCENT_SOFT_PERCENT = 20;

/**
 * The two foreground candidates for [`autoForeground`], the app palette's
 * light and dark text colours (`src/styles/theme.css`, `--light-color-text` /
 * `--dark-color-text`). The one pair of hex values this module spells out,
 * because picking between them is arithmetic CSS cannot do; `color-contrast()`
 * is not in the WebKit versions Handy targets.
 */
const INK_DARK = "#0f0f0f";
const INK_LIGHT = "#fbfbfb";

/**
 * The custom properties that carry a colour. Named apart from the lengths
 * below because a colour is the only property whose computed value has to be
 * read back off the page. The Appearance tab's probes resolve these to a hex
 * to show what an unset token inherits, and re-measuring after a length-only
 * change buys nothing.
 */
export const OVERLAY_THEME_COLOR_PROPERTIES: readonly string[] = [
  "--s-accent",
  "--s-accent-soft",
  "--s-surface",
  "--s-text",
  "--s-muted",
  "--s-faint",
  "--s-border",
  "--s-hair",
];

/** The custom properties that carry a length or a plain number. */
export const OVERLAY_THEME_LENGTH_PROPERTIES: readonly string[] = [
  "--ov-scale",
  "--ov-radius",
  "--ov-border-w",
  "--ov-pad",
  "--ov-elem-gap",
  "--ov-wave-gap",
  "--ov-wave-w",
  "--ov-side-min",
  "--ov-row-gaps",
  "--ov-shadow-strength",
  "--ov-shadow-y",
  "--ov-shadow-slack",
  "--ov-shadow-edge-slack",
];

/**
 * The custom properties this module may write. [`applyOverlayTheme`] removes
 * every one the current theme does not produce, which is what makes a reset to
 * inherit actually reset.
 */
export const OVERLAY_THEME_CSS_PROPERTIES: readonly string[] = [
  ...OVERLAY_THEME_COLOR_PROPERTIES,
  ...OVERLAY_THEME_LENGTH_PROPERTIES,
];

const HEX_COLOR = /^#[0-9a-f]{6}$/i;

/** A `#rrggbb` colour, or `null` for anything else (rule 2). */
function validHex(value: unknown): string | null {
  return typeof value === "string" && HEX_COLOR.test(value) ? value : null;
}

/** A finite number inside the token's bounds, or `null` (rule 2). */
function validNumber(value: unknown, min: number, max: number): number | null {
  if (typeof value !== "number" || !Number.isFinite(value)) return null;
  return value >= min && value <= max ? value : null;
}

/** A numeric token, re-validated against its own bounds. */
function validToken(
  theme: OverlayTheme,
  key: OverlayNumericKey,
): number | null {
  const { min, max } = OVERLAY_TOKEN_BOUNDS[key];
  return validNumber(theme[key], min, max);
}

/**
 * A percentage without its floating-point tail. `0.92 * 100` is
 * `92.00000000000001` in IEEE 754, which would be echoed into the CSS.
 */
function percent(strength: number): string {
  return String(Number(strength.toFixed(2)));
}

/** `color-mix(in srgb, <color> <strength>%, transparent)`. */
function alphaMix(color: string, strength: number): string {
  return `color-mix(in srgb, ${color} ${percent(strength)}%, transparent)`;
}

/** The relative luminance of a `#rrggbb` colour, 0 to 1 (WCAG 2.x). */
function relativeLuminance(hex: string): number {
  const channel = (offset: number): number => {
    const value = parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return value <= 0.04045
      ? value / 12.92
      : Math.pow((value + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

/** The WCAG 2.x contrast ratio between two `#rrggbb` colours. */
function contrastRatio(a: string, b: string): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  const [lighter, darker] = la >= lb ? [la, lb] : [lb, la];
  return (lighter + 0.05) / (darker + 0.05);
}

/**
 * The foreground for a surface when the theme sets `surface` but not `text`,
 * whichever of the app palette's two inks has the higher WCAG contrast ratio
 * against it. The one computed (non-CSS) step in the apply layer, so a one-key
 * theme file is not black text on a black card; `{"surface": "#1a1b26"}` is
 * what an external theming tool writes first. An unparseable surface yields
 * the dark ink, matching Handy's light default.
 */
export function autoForeground(surfaceHex: string): string {
  const surface = validHex(surfaceHex);
  if (!surface) return INK_DARK;
  return contrastRatio(INK_DARK, surface) >= contrastRatio(INK_LIGHT, surface)
    ? INK_DARK
    : INK_LIGHT;
}

/** The Material actually rendered, re-validated (rule 2). */
function effectiveMaterialOf(resolved: ResolvedOverlayTheme): Material {
  return resolved.effective_material === "glass" ? "glass" : "flat";
}

/**
 * How far the card is inset from the screen edge the overlay is anchored to,
 * in points, re-validated (rule 2).
 *
 * Derived by Rust, carried on the resolved theme: only Rust knows the gap the
 * card already has to the Dock, the taskbar or the menu bar on this platform,
 * and the native window was sized and placed from that same number. No platform
 * table here, on purpose. A mirror written before the field existed, or a
 * hand-edited one, falls back to the full slack, as the three other sides do.
 */
function edgeSlack(resolved: ResolvedOverlayTheme, slack: number): number {
  const carried = resolved.shadow_edge_slack;
  if (typeof carried !== "number" || !Number.isFinite(carried)) return slack;
  return Math.min(Math.max(Math.round(carried), 0), slack);
}

/**
 * Pure. A resolved theme in, the custom properties out. A property appears
 * only when its source token is set, plus the two groups Glass writes
 * unconditionally, because a 98% surface would hide the blur and the Flat
 * neutrals are too weak to read over it.
 */
export function resolveOverlayThemeVars(
  resolved: ResolvedOverlayTheme,
): Record<string, string> {
  const vars: Record<string, string> = {};
  const theme = resolved.theme;
  const material = effectiveMaterialOf(resolved);
  const glass = material === "glass";

  const accent = validHex(theme.accent);
  if (accent) {
    vars["--s-accent"] = accent;
    vars["--s-accent-soft"] = alphaMix(accent, ACCENT_SOFT_PERCENT);
  }

  // The surface colour is shared by both Materials; its alpha is not. Flat
  // reads `surface_opacity`, Glass reads `glass_tint` and ignores the opacity,
  // which lets an opaque Flat card and see-through Glass live in one theme.
  // With the colour unset the mix below substitutes its inherited input, which
  // is today's, so an alpha-only theme keeps a theme-aware card.
  const surface = validHex(theme.surface);
  const opacity = validToken(theme, "surface_opacity");
  const tint = validToken(theme, "glass_tint");
  if (glass) {
    // Written unconditionally under Glass, because the CSS default of 98%
    // would hide the blur. The card paints this on every engine, Liquid Glass
    // included. Rust hands Liquid Glass the same colour natively to lens, but
    // that native tint is not trusted alone. Measured on macOS 26, a card that
    // painted nothing and left the tint to `tintColor` came out dark under a
    // Light app theme, transcript unreadable.
    vars["--s-surface"] = alphaMix(
      surface ?? "var(--color-background)",
      (tint ?? GLASS_TINT_INHERIT) * 100,
    );
  } else if (surface !== null || opacity !== null) {
    vars["--s-surface"] = alphaMix(
      surface ?? "var(--color-background)",
      (opacity ?? SURFACE_OPACITY_INHERIT) * 100,
    );
  }

  const text = validHex(theme.text);
  if (text) {
    vars["--s-text"] = text;
  } else if (surface) {
    vars["--s-text"] = autoForeground(surface);
  }

  // The neutrals are alpha over the card in every case, so they stay correct
  // under Glass. They mix from `--s-text`, the property above when written,
  // the app's text colour when not.
  if (text !== null || surface !== null || glass) {
    const neutrals = NEUTRALS[material];
    vars["--s-muted"] = alphaMix("var(--s-text)", neutrals.muted);
    vars["--s-faint"] = alphaMix("var(--s-text)", neutrals.faint);
    vars["--s-hair"] = alphaMix("var(--s-text)", neutrals.hair);
  }

  // The card's edge. It has its own two tokens but also follows
  // `text`/`surface`, because an edge derived from a foreground the theme
  // replaced would be the one neutral left behind. Under Glass it is written
  // unconditionally and at a stronger alpha, the edge being the only hard line
  // a translucent card has against what it blurs. Under Clear glass it is a
  // white highlight like Spotlight's own capsule, not a foreground hairline;
  // see [`inheritedBorder`]. Keyed on the `glass_style` token, not the engine,
  // so what the card paints stays a function of the theme alone.
  const border = validHex(theme.border);
  const borderOpacity = validToken(theme, "border_opacity");
  if (
    border !== null ||
    borderOpacity !== null ||
    text !== null ||
    surface !== null ||
    glass
  ) {
    const inherited = inheritedBorder(material, effectiveGlassStyleOf(theme));
    vars["--s-border"] = alphaMix(
      border ?? inherited.color,
      (borderOpacity ?? inherited.opacity) * 100,
    );
  }

  // Raw token values only. The CSS does every multiplication with `calc(... *
  // var(--ov-scale))`.
  const scale = validToken(theme, "size_scale");
  if (scale !== null) vars["--ov-scale"] = String(scale);

  const radius = validToken(theme, "radius");
  if (radius !== null) vars["--ov-radius"] = `${radius}px`;

  // One of the two lengths the native window geometry also reads.
  // `overlay_geometry.rs` adds two of these to the card's footprint, so both
  // sides must agree on the number, which is why it is a token, not derived.
  const borderWidth = validToken(theme, "border_width");
  if (borderWidth !== null) vars["--ov-border-w"] = `${borderWidth}px`;

  // The other one. The control row is a fixed core plus one of these above and
  // below, and the Live transcript's inset follows, so padding grows the
  // window on both axes.
  const padding = validToken(theme, "padding");
  if (padding !== null) vars["--ov-pad"] = `${padding}px`;

  const waveformGap = validToken(theme, "waveform_gap");
  if (waveformGap !== null) vars["--ov-wave-gap"] = `${waveformGap}px`;

  const waveformWidth = validToken(theme, "waveform_width");
  if (waveformWidth !== null) vars["--ov-wave-w"] = `${waveformWidth}px`;

  // The row's own two gaps, which every card width pays for twice.
  const elementGap = validToken(theme, "element_gap");
  if (elementGap !== null) vars["--ov-elem-gap"] = `${elementGap}px`;

  // The side columns' floor exists to hold the cancel button, so it goes with
  // it, and so do the row's right column and the element gap beside it: the row
  // is then the dot column, one gap and the waveform lane. Written only for
  // values that are not the stylesheet's, keeping the 22 and the 2 out of this
  // file: the CSS declares both, and this says "there is no button any more".
  if (theme.show_cancel === false) {
    vars["--ov-side-min"] = "0px";
    vars["--ov-row-gaps"] = "1";
  }

  // The shadow, Flat's only. Under Glass it is macOS's own, drawn outside a
  // window the card fills exactly, and `shadow_strength` just switches it
  // (`overlay_glass::window_shadow`), so nothing is written there.
  //
  // The two slacks are the only pre-scaled properties here: CSS cannot round,
  // and the native window inset the card by these very integers, so a
  // fractional disagreement would drift the card off its screen offset. Each
  // is computed once and written whole.
  const strength =
    validToken(theme, "shadow_strength") ?? SHADOW_STRENGTH_INHERIT[material];
  if (!glass && strength > 0) {
    const offsetY =
      validToken(theme, "shadow_offset_y") ??
      STATIC_NUMERIC_INHERIT.shadow_offset_y;
    const slack = Math.ceil((SHADOW_BLUR_PX + offsetY) * (scale ?? 1));
    vars["--ov-shadow-strength"] = String(strength);
    vars["--ov-shadow-y"] = `${offsetY}px`;
    vars["--ov-shadow-slack"] = `${slack}px`;
    vars["--ov-shadow-edge-slack"] = `${edgeSlack(resolved, slack)}px`;
  }

  return vars;
}

/** The writes and removals that take an element from one set of overlay theme
 *  properties to another. */
export interface OverlayThemeStyleDelta {
  set: readonly (readonly [property: string, value: string])[];
  remove: readonly string[];
}

/**
 * Pure. The smallest set of style operations that turns `previous` into
 * `next`. `previous` is what this module last wrote onto the element, or
 * `null` when that is unknown, the first apply, where every property this
 * module may write has to be cleared in case something else left one behind.
 * After that only what was written is worth removing and only what changed is
 * worth writing. The theme is applied on every frame of a slider drag, and
 * each `setProperty` on the document element invalidates style for the whole
 * card.
 */
export function overlayThemeStyleDelta(
  previous: Record<string, string> | null,
  next: Record<string, string>,
): OverlayThemeStyleDelta {
  const set: [string, string][] = [];
  const remove: string[] = [];
  for (const property of previous
    ? Object.keys(previous)
    : OVERLAY_THEME_CSS_PROPERTIES) {
    if (next[property] === undefined) remove.push(property);
  }
  for (const [property, value] of Object.entries(next)) {
    if (previous?.[property] !== value) set.push([property, value]);
  }
  return { set, remove };
}

/**
 * What [`applyOverlayTheme`] last wrote onto each element it was given.
 *
 * The assumption this rests on. After the first apply, this module is the only
 * writer of the `--s-*` and `--ov-*` inline properties on that element. Nobody
 * else may set or remove one, not the overlay component, the tab, or a
 * devtools poke expecting to survive. If something else did, the removal rule
 * would go blind, still listing a property this module no longer controls, and
 * a token going back to inherit would be "removed" from a value it never
 * wrote. It holds today because the two callers pass elements they own (the
 * overlay's `document.documentElement`, the tab's own probe host) and every
 * write to these properties in this repository goes through here. A `grep` for
 * `--ov-` and `--s-` outside this module and the two stylesheets that declare
 * the inherited values finds nothing. A `WeakMap`, not a field on the element,
 * so an element that goes away takes its record with it and nothing
 * user-visible is stored on the DOM.
 */
const lastApplied = new WeakMap<HTMLElement, Record<string, string>>();

/**
 * Write a resolved overlay theme onto an element and remove every property it
 * does not set. The removal is the point. Without it a token going back to
 * inherit would keep painting, because inline style beats the stylesheet the
 * inherited value lives in. `data-material` is always set; a `null` theme
 * removes every property in [`OVERLAY_THEME_CSS_PROPERTIES`] and leaves
 * `data-material="flat"`. Only the differences [`overlayThemeStyleDelta`]
 * finds are written, invisibly from the outside; either way the element ends
 * up carrying exactly the properties `resolveOverlayThemeVars` produced.
 */
export function applyOverlayTheme(
  root: HTMLElement,
  resolved: ResolvedOverlayTheme | null,
): void {
  const vars = resolved ? resolveOverlayThemeVars(resolved) : {};
  // The first apply has no record to diff against, which is what `undefined`
  // from the map means. It passes on as an explicit `null`, the delta's own
  // name for "assume nothing, clear every property this module could have
  // written", so the first apply is a full reset even if a previous page load,
  // a hot reload or a hand-edited inline style left one behind.
  const previous = lastApplied.get(root);
  const firstApply = previous === undefined;
  const { set, remove } = overlayThemeStyleDelta(
    firstApply ? null : previous,
    vars,
  );
  for (const property of remove) root.style.removeProperty(property);
  for (const [property, value] of set) root.style.setProperty(property, value);
  lastApplied.set(root, vars);

  const material = resolved ? effectiveMaterialOf(resolved) : "flat";
  if (root.dataset.material !== material) root.dataset.material = material;
}

export const OVERLAY_THEME_STORAGE_KEY = "handy.overlayTheme";

/** Shallow shape check; every value is re-validated when it is applied. */
function isResolvedShape(value: unknown): value is ResolvedOverlayTheme {
  if (typeof value !== "object" || value === null) return false;
  const theme = (value as { theme?: unknown }).theme;
  return typeof theme === "object" && theme !== null;
}

/**
 * The last resolved theme the overlay applied, for synchronous use at boot.
 * Returns `null` when the mirror is missing, unreadable or not shaped like a
 * payload, which means apply nothing and leave today's cascade.
 */
export function getStoredOverlayTheme(): ResolvedOverlayTheme | null {
  try {
    const stored = localStorage.getItem(OVERLAY_THEME_STORAGE_KEY);
    if (!stored) return null;
    const parsed: unknown = JSON.parse(stored);
    return isResolvedShape(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

/**
 * Remember a resolved theme for the next boot. Only the overlay window calls
 * this; the settings window has no root to paint, so it never contributes to
 * the mirror.
 */
export function storeOverlayTheme(resolved: ResolvedOverlayTheme): void {
  try {
    localStorage.setItem(OVERLAY_THEME_STORAGE_KEY, JSON.stringify(resolved));
  } catch {
    // localStorage may be unavailable; the resolved theme still arrives from
    // the backend on show, so the only cost is failure tolerance there.
  }
}
