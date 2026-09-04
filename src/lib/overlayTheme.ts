import type { Material, OverlayTheme, ResolvedOverlayTheme } from "@/bindings";

/**
 * The apply layer, the one module that turns a resolved overlay theme into
 * the overlay's CSS custom properties and its material attribute.
 *
 * Both callers use this module, so they cannot drift:
 *  - the overlay window writes the map onto `document.documentElement`;
 *  - the Appearance tab's preview writes the same map as inline style on its
 *    own wrapper.
 *
 * Hard constraint. The overlay entry loads this module, and that entry is the
 * memory-sensitive webview (#1279). It must NOT import React, i18next or the
 * settings store. Pure functions, constants and localStorage only. The one
 * import it has is type-only, so it disappears at build time.
 *
 * Two rules govern the whole file.
 *
 *  1. Removal. This module writes a property only while its token is set, and
 *     [`applyOverlayTheme`] removes every property it does not write. Inline
 *     style beats any stylesheet, so a property written once and never removed
 *     would survive a reset to inherit forever.
 *  2. Re-validation at the boundary. Rust canonicalises every colour and
 *     clamps every number, but the localStorage mirror below bypasses Rust and
 *     a user can edit it. So this module re-checks every value before it goes
 *     into CSS, and treats anything that fails as unset.
 */

/** A token name, both an `OverlayTheme` field and a theme-file key. */
export type OverlayThemeKey = keyof OverlayTheme;

/** The four tokens whose value is a colour. */
export type OverlayColorKey = "accent" | "surface" | "text" | "border";

/** The nine tokens whose value is a number. */
export type OverlayNumericKey =
  | "surface_opacity"
  | "glass_tint"
  | "border_opacity"
  | "size_scale"
  | "radius"
  | "border_width"
  | "padding"
  | "waveform_gap"
  | "waveform_width";

/**
 * The numeric tokens' bounds, the same numbers Rust clamps every value to.
 *
 * Two consumers: the re-validation below, which treats anything outside a
 * token's range as unset, and the Appearance tab's sliders. Every px value
 * here is at size scale 1; the scale multiplies it in CSS.
 */
export const OVERLAY_TOKEN_BOUNDS: Record<
  OverlayNumericKey,
  { min: number; max: number; step: number }
> = {
  surface_opacity: { min: 0.3, max: 1.0, step: 0.01 },
  glass_tint: { min: 0.0, max: 1.0, step: 0.01 },
  border_opacity: { min: 0.0, max: 1.0, step: 0.01 },
  size_scale: { min: 0.8, max: 1.5, step: 0.05 },
  radius: { min: 0, max: 32, step: 1 },
  border_width: { min: 0, max: 4, step: 1 },
  padding: { min: 0, max: 20, step: 1 },
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
  size_scale: null,
  radius: null,
  border_width: null,
  padding: null,
  waveform_gap: null,
  waveform_width: null,
};

/**
 * The card alpha an unset `surface_opacity` resolves to. Today's near-opaque
 * card, straight from `RecordingOverlay.css`.
 *
 * Flat's number, and only Flat's. Glass never reads the token at all; see
 * [`GLASS_TINT_INHERIT`] for why the two are separate tokens rather than one
 * value with a per-Material default.
 */
export const SURFACE_OPACITY_INHERIT = 0.98;

/**
 * The tint alpha an unset `glass_tint` resolves to, on both engines.
 *
 * 0.45 is a measured number. Over a split light/dark striped desktop the
 * card passes about 53 levels of the backdrop through at 0.45, against 27 at
 * the 0.70 this feature first shipped with, which is the difference between
 * "a dark card" and "frosted glass". The worst-case contrast of the
 * transcript over the brightest backdrop still stays at 6.1:1, comfortably
 * past WCAG AA. Going further to 0.30 buys another 10 levels but drops that
 * worst case to 4.9:1, which a pure-white desktop would push under the line.
 *
 * Liquid Glass (macOS 26) was measured against the same desktop and keeps the
 * same 0.45. At 0.30 the worst-case transcript contrast falls to 4.3:1 under a
 * Light app theme, under WCAG AA, while 0.45 holds 5.6-9.6:1 across both Glass
 * styles and both app themes. Rust composes the native `tintColor` from the
 * identical number (`GLASS_TINT_INHERIT` in
 * `src-tauri/src/overlay_theme.rs`).
 *
 * Why Glass has its own token. While the two Materials shared
 * `surface_opacity`, they were mutually exclusive in practice. A card set
 * opaque under Flat stayed opaque when the user picked Glass, so Glass looked
 * broken the first time it was chosen and nothing on screen said why. Each
 * Material now carries its own alpha, so switching Material always lands on
 * that Material's own value and Glass is glass immediately.
 */
export const GLASS_TINT_INHERIT = 0.45;

/**
 * What an unset `border` mixes from. The foreground, on every Material.
 *
 * One value and not a per-Material pair, because the obvious Glass default,
 * a white rim of the kind an Apple HUD carries, was measured and rejected.
 * It only works under a Dark app theme. Over a light card (Light theme, where
 * the tint is near-white) a white edge at 30 % moves the pixels by 3 levels,
 * which is no edge at all, against 27 for the foreground mix, and what the
 * default has to do is be visible in all four combinations of app theme and
 * backdrop. Only the alpha differs per Material; see
 * [`BORDER_OPACITY_INHERIT`]. A theme that wants the Apple rim can still ask
 * for it with `border: "#ffffff"` and `border_opacity: 0.35`, which is what
 * the token is for.
 */
export const BORDER_INHERIT = "var(--s-text)";

/**
 * The alpha an unset `border_opacity` resolves to, per Material. The second
 * half of [`BORDER_INHERIT`]. Flat's 0.12 is today's hairline strength;
 * Glass's is stronger because the edge is the only hard line a translucent
 * card has, and the thinner tint above leaves it more work to do. Measured
 * again on Liquid Glass, where 0.25 lands as a single 2 px transition of
 * +45 levels over the card, reading as one hairline beside the glass's own
 * rim rather than a second line.
 */
export const BORDER_OPACITY_INHERIT: Record<Material, number> = {
  flat: 0.12,
  glass: 0.25,
};

/** Each numeric token's single-number inherit, from `RecordingOverlay.css`. */
const STATIC_NUMERIC_INHERIT: Record<
  Exclude<OverlayNumericKey, "border_opacity">,
  number
> = {
  surface_opacity: SURFACE_OPACITY_INHERIT, // --s-surface's own 98%
  glass_tint: GLASS_TINT_INHERIT, // measured, not a CSS default
  size_scale: 1, // --ov-scale
  radius: 24, // --ov-radius
  border_width: 1, // --ov-border-w
  padding: 10, // --ov-pad-x
  waveform_gap: 3, // --ov-wave-gap
  waveform_width: 4, // --ov-wave-w
};

/**
 * What a numeric token resolves to while it is unset. The number a control
 * shows, and the number the card is actually painted with.
 *
 * The Material is a parameter because one token's inherit depends on it. The
 * card's edge is stronger over glass. Asking for the Material unconditionally
 * is what keeps every caller from having to know which token that is.
 *
 * The numbers live here rather than in the Appearance tab, beside the two
 * alphas above, so one module answers "what does an unset token inherit".
 * `overlay_token_inherit_values_match_the_css` in
 * `src-tauri/src/overlay_theme.rs` pins them to the `:root` block in
 * `RecordingOverlay.css`, the stylesheet that actually paints an unset token,
 * so a length changed in one place and not the other fails a test instead of
 * showing the wrong number on a slider.
 *
 * Callers ask for `border_opacity` through this function rather than reading
 * it out of the table, because its inherit differs per Material; see
 * [`BORDER_OPACITY_INHERIT`].
 */
export function inheritedTokenValue(
  key: OverlayNumericKey,
  material: Material,
): number {
  return key === "border_opacity"
    ? BORDER_OPACITY_INHERIT[material]
    : STATIC_NUMERIC_INHERIT[key];
}

/**
 * The neutral group as a percentage of the foreground, per Material.
 *
 * Flat's are reverse-engineered from today's hand-picked neutrals, so
 * switching to the derivation lands on today's look in both app themes.
 * Glass's are strengthened because muted and faint are what fail first over a
 * blurred background, whichever engine blurs it. These are the most
 * eyeball-tunable numbers in the feature.
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
 * The two foreground candidates for [`autoForeground`].
 *
 * These are the app palette's light and dark text colours
 * (`src/styles/theme.css`, `--light-color-text` / `--dark-color-text`). They are
 * the one pair of hex values this module spells out, because picking between
 * them is arithmetic that CSS cannot do. `color-contrast()` is not in the
 * WebKit versions Handy targets.
 */
const INK_DARK = "#0f0f0f";
const INK_LIGHT = "#fbfbfb";

/**
 * The custom properties that carry a colour.
 *
 * Named apart from the lengths below because a colour is the only kind of
 * property whose computed value has to be read back off the page. The
 * Appearance tab's probes resolve these to a hex to show what an unset token
 * inherits, and there is no point re-measuring after a change that could only
 * have touched a length.
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
  "--ov-pad-x",
  "--ov-wave-gap",
  "--ov-wave-w",
];

/**
 * The custom properties this module may write.
 *
 * [`applyOverlayTheme`] removes every one of them that the current theme does
 * not produce, which is what makes a reset to inherit actually reset.
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
 * A percentage without its floating-point tail: `0.92 * 100` is
 * `92.00000000000001` in IEEE 754, and that would be echoed into the CSS.
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
 * The foreground for a surface when the theme sets `surface` but not `text`.
 * Whichever of the app palette's two inks has the higher WCAG contrast ratio
 * against it.
 *
 * This is the one computed (non-CSS) step in the whole apply layer. It exists
 * so that a one-key theme file is not black text on a black card.
 * `{"surface": "#1a1b26"}` is exactly what an external theming tool writes
 * first.
 *
 * An unparseable surface yields the dark ink, matching Handy's light default.
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
 * Pure. A resolved overlay theme in, the custom properties to write out.
 *
 * A property appears only when its source token is set, plus the two groups
 * Glass writes unconditionally, because a 98% surface would hide the blur and
 * the Flat neutrals are too weak to read over it.
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
  // reads `surface_opacity`, Glass reads `glass_tint` and ignores the opacity
  // entirely, which is what lets an opaque Flat card and see-through Glass
  // live in one theme. With the colour unset the mix below substitutes its
  // inherited input, and because that input is literally today's, a theme
  // that sets only an alpha keeps a theme-aware card.
  const surface = validHex(theme.surface);
  const opacity = validToken(theme, "surface_opacity");
  const tint = validToken(theme, "glass_tint");
  if (glass) {
    // Written unconditionally under Glass, because the CSS default of 98%
    // would hide the blur. The card paints this on every engine, Liquid Glass
    // included. Rust hands Liquid Glass the same colour natively so that it
    // can lens it, but that native tint is not trusted to be the only one.
    // Measured on macOS 26, a card that painted nothing and left the tint to
    // `tintColor` alone came out dark under a Light app theme, with the
    // transcript unreadable on it.
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
  // under Glass. They mix from `--s-text`, which resolves to the property above
  // when it is written and to the app's text colour when it is not.
  if (text !== null || surface !== null || glass) {
    const neutrals = NEUTRALS[material];
    vars["--s-muted"] = alphaMix("var(--s-text)", neutrals.muted);
    vars["--s-faint"] = alphaMix("var(--s-text)", neutrals.faint);
    vars["--s-hair"] = alphaMix("var(--s-text)", neutrals.hair);
  }

  // The card's edge. It has its own two tokens, but it also follows
  // `text`/`surface`, because an edge derived from a foreground the theme has
  // replaced would be the one neutral left behind. Under Glass this writes it
  // unconditionally, and at a stronger alpha, because the edge is the only
  // hard line a translucent card has against whatever it is blurring.
  const border = validHex(theme.border);
  const borderOpacity = validToken(theme, "border_opacity");
  if (
    border !== null ||
    borderOpacity !== null ||
    text !== null ||
    surface !== null ||
    glass
  ) {
    vars["--s-border"] = alphaMix(
      border ?? BORDER_INHERIT,
      (borderOpacity ?? BORDER_OPACITY_INHERIT[material]) * 100,
    );
  }

  // Raw token values only. The CSS does every multiplication with
  // `calc(... * var(--ov-scale))`.
  const scale = validToken(theme, "size_scale");
  if (scale !== null) vars["--ov-scale"] = String(scale);

  const radius = validToken(theme, "radius");
  if (radius !== null) vars["--ov-radius"] = `${radius}px`;

  // The one length the native window geometry also reads.
  // `overlay_geometry.rs` adds two of these to the card's footprint, so the
  // two sides must agree on the number, which is why it is a token and not a
  // derived value.
  const borderWidth = validToken(theme, "border_width");
  if (borderWidth !== null) vars["--ov-border-w"] = `${borderWidth}px`;

  const padding = validToken(theme, "padding");
  if (padding !== null) vars["--ov-pad-x"] = `${padding}px`;

  const waveformGap = validToken(theme, "waveform_gap");
  if (waveformGap !== null) vars["--ov-wave-gap"] = `${waveformGap}px`;

  const waveformWidth = validToken(theme, "waveform_width");
  if (waveformWidth !== null) vars["--ov-wave-w"] = `${waveformWidth}px`;

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
 * `next`.
 *
 * `previous` is what this module last wrote onto the element, or `null` when
 * that is unknown, which is the first apply, where every property this module
 * may write has to be cleared in case something else left one behind. After
 * that, only what was actually written is worth removing, and only what
 * actually changed is worth writing. The overlay theme is now applied on
 * every frame of a slider drag, and each `setProperty` on the document
 * element invalidates style for the whole card.
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
 * The assumption this rests on. After the first apply, this module is the
 * only writer of the `--s-*` and `--ov-*` inline properties on that element.
 * Nobody else may set or remove one, not the overlay component, not the tab,
 * not a devtools poke that expects to survive. If something else did, the
 * removal rule would go blind. The map would still list a property this
 * module no longer controls, and a token going back to inherit would be
 * "removed" from a value it never wrote.
 *
 * It holds today because the two callers pass elements they own (the
 * overlay's `document.documentElement`, the tab's own probe host) and every
 * write to these properties in this repository goes through here. A `grep`
 * for `--ov-` and `--s-` outside this module and the two stylesheets that
 * declare the inherited values finds nothing.
 *
 * A `WeakMap` and not a field on the element so an element that goes away
 * takes its record with it, and so nothing user-visible is stored on the DOM.
 */
const lastApplied = new WeakMap<HTMLElement, Record<string, string>>();

/**
 * Write a resolved overlay theme onto an element, and remove every property it
 * does not set.
 *
 * The removal is the point. Without it a token that goes back to inherit
 * would keep painting, because inline style beats the stylesheet the
 * inherited value lives in. `data-material` is always set; a `null` theme
 * removes every property in [`OVERLAY_THEME_CSS_PROPERTIES`] and leaves
 * `data-material="flat"`.
 *
 * It writes only the differences [`overlayThemeStyleDelta`] finds, which is
 * invisible from the outside. Either way the element ends up carrying exactly
 * the properties `resolveOverlayThemeVars` produced.
 */
export function applyOverlayTheme(
  root: HTMLElement,
  resolved: ResolvedOverlayTheme | null,
): void {
  const vars = resolved ? resolveOverlayThemeVars(resolved) : {};
  // The first apply onto this element has no record to diff against, and
  // `undefined` from the map means exactly that. This passes it on as an
  // explicit `null`, the delta's own name for "assume nothing, clear every
  // property this module could have written", so the first apply is a full
  // reset even if a previous page load, a hot reload or a hand-edited
  // inline style left one behind.
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
 *
 * Returns `null` when the mirror is missing, unreadable or not shaped like a
 * payload. A `null` means apply nothing and leave today's cascade.
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
 * Remember a resolved theme for the next boot.
 *
 * Only the overlay window calls this. The settings window has no root to
 * paint, so it never contributes to the mirror.
 */
export function storeOverlayTheme(resolved: ResolvedOverlayTheme): void {
  try {
    localStorage.setItem(OVERLAY_THEME_STORAGE_KEY, JSON.stringify(resolved));
  } catch {
    // localStorage may be unavailable; the resolved theme still arrives from
    // the backend on show, so this only costs failure tolerance on that path.
  }
}
