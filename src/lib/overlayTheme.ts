import type { Material, OverlayTheme, ResolvedOverlayTheme } from "@/bindings";

/**
 * The apply layer: the one module that turns a resolved overlay theme into the
 * overlay's CSS custom properties and its material attribute.
 *
 * Both callers use this module, so they cannot drift:
 *  - the overlay window writes the map onto `document.documentElement`;
 *  - the Appearance tab's preview writes the same map as inline style on its
 *    own wrapper.
 *
 * Hard constraint: this module is loaded by the overlay entry, which is the
 * memory-sensitive webview (#1279). It must NOT import React, i18next or the
 * settings store — pure functions, constants and localStorage only. The only
 * import is a type-only one, which disappears at build time.
 *
 * Two rules govern the whole file:
 *
 *  1. **Removal.** A property is written only while its token is set, and
 *     [`applyOverlayTheme`] removes every property it does not write. Inline
 *     style beats any stylesheet, so a property written once and never removed
 *     would survive a reset to inherit forever.
 *  2. **Re-validation at the boundary.** Rust canonicalises every colour and
 *     clamps every number, but the localStorage mirror below bypasses Rust and
 *     a user can edit it. Every value is therefore re-checked here before it is
 *     interpolated into CSS; anything that fails is treated as unset.
 */

/** A token name: simultaneously an `OverlayTheme` field and a theme-file key. */
export type OverlayThemeKey = keyof OverlayTheme;

/** The four tokens whose value is a colour. */
export type OverlayColorKey = "accent" | "surface" | "text" | "border";

/** The eight tokens whose value is a number. */
export type OverlayNumericKey =
  | "surface_opacity"
  | "border_opacity"
  | "size_scale"
  | "radius"
  | "border_width"
  | "padding"
  | "waveform_gap"
  | "waveform_width";

/**
 * The numeric tokens' bounds, straight from the token contract's table.
 *
 * Two consumers: the re-validation below, which treats anything outside a
 * token's range as unset, and the Appearance tab's sliders. Every px value is
 * expressed at size scale 1; the scale multiplies it in CSS.
 */
export const OVERLAY_TOKEN_BOUNDS: Record<
  OverlayNumericKey,
  { min: number; max: number; step: number }
> = {
  surface_opacity: { min: 0.3, max: 1.0, step: 0.01 },
  border_opacity: { min: 0.0, max: 1.0, step: 0.01 },
  size_scale: { min: 0.8, max: 1.5, step: 0.05 },
  radius: { min: 0, max: 32, step: 1 },
  border_width: { min: 0, max: 4, step: 1 },
  padding: { min: 0, max: 20, step: 1 },
  waveform_gap: { min: 0, max: 5, step: 1 },
  waveform_width: { min: 2, max: 6, step: 1 },
};

/** The theme that inherits every token — Handy's overlay as it ships. */
export const INHERIT_ALL: OverlayTheme = {
  accent: null,
  surface: null,
  surface_opacity: null,
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
 * The surface alpha an unset `surface_opacity` resolves to, per Material.
 *
 * Flat's 0.98 is today's near-opaque card. Glass's 0.45 is measured, not
 * guessed: over a split light/dark striped desktop the card passes about 53
 * levels of the backdrop through at 0.45 against 27 at the 0.70 this feature
 * first shipped with, which is the difference between "a dark card" and
 * "frosted glass" — while the worst-case contrast of the transcript over the
 * brightest backdrop stays at 6.1:1, comfortably past WCAG AA. Going further
 * to 0.30 buys another 10 levels but drops that worst case to 4.9:1, which a
 * pure-white desktop would push under the line.
 *
 * Liquid Glass (macOS 26) was measured against the same desktop and keeps the
 * same 0.45: at 0.30 the worst-case transcript contrast falls to 4.3:1 under a
 * Light app theme, under WCAG AA, while 0.45 holds 5.6-9.6:1 across both Glass
 * styles and both app themes. Rust composes the native `tintColor` from the
 * identical number (`SURFACE_OPACITY_INHERIT_GLASS` in
 * `src-tauri/src/overlay_theme.rs`).
 */
export const SURFACE_OPACITY_INHERIT: Record<Material, number> = {
  flat: 0.98,
  glass: 0.45,
};

/**
 * What an unset `border` mixes from: the foreground, on every Material.
 *
 * One value and not a per-Material pair, because the obvious Glass default —
 * a white rim, the way an Apple HUD carries one — was measured and rejected.
 * It only works under a Dark app theme: over a light card (Light theme, where
 * the tint is near-white) a white edge at 30 % moves the pixels by 3 levels,
 * which is no edge at all, against 27 for the foreground mix, and what the
 * *default* has to do is be visible in all four combinations of app theme and
 * backdrop. Only the alpha differs per Material — see
 * [`BORDER_OPACITY_INHERIT`]. A theme that wants the Apple rim can still ask
 * for it — `border: "#ffffff"`, `border_opacity: 0.35` — which is what the
 * token is for.
 */
export const BORDER_INHERIT = "var(--s-text)";

/**
 * The alpha an unset `border_opacity` resolves to, per Material — the second
 * half of [`BORDER_INHERIT`]. Flat's 0.12 is today's hairline strength;
 * Glass's is stronger because the edge is the only hard line a translucent
 * card has, and the thinner tint above leaves it more work to do. Measured
 * again on Liquid Glass, where 0.25 lands as a single 2 px transition of
 * +45 levels over the card — one hairline beside the glass's own rim, not a
 * second line.
 */
export const BORDER_OPACITY_INHERIT: Record<Material, number> = {
  flat: 0.12,
  glass: 0.25,
};

/**
 * The neutral group as a percentage of the foreground, per Material.
 *
 * Flat's are reverse-engineered from today's hand-picked neutrals, so
 * switching to the derivation lands on today's look in both app themes.
 * Glass's are strengthened because muted and faint are what fail first over a
 * blurred background — whichever engine blurs it. These are the most
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
 * them is arithmetic that CSS cannot do: `color-contrast()` is not in the
 * WebKit versions Handy targets.
 */
const INK_DARK = "#0f0f0f";
const INK_LIGHT = "#fbfbfb";

/**
 * The custom properties this module may write.
 *
 * [`applyOverlayTheme`] removes every one of them that the current theme does
 * not produce, which is what makes a reset to inherit actually reset.
 */
export const OVERLAY_THEME_CSS_PROPERTIES: readonly string[] = [
  "--s-accent",
  "--s-accent-soft",
  "--s-surface",
  "--s-text",
  "--s-muted",
  "--s-faint",
  "--s-border",
  "--s-hair",
  "--ov-scale",
  "--ov-radius",
  "--ov-border-w",
  "--ov-pad-x",
  "--ov-wave-gap",
  "--ov-wave-w",
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

/** The 0–1 relative luminance of a `#rrggbb` colour (WCAG 2.x). */
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
 * The foreground for a surface when the theme sets `surface` but not `text`:
 * whichever of the app palette's two inks has the higher WCAG contrast ratio
 * against it.
 *
 * This is the one computed (non-CSS) step in the whole apply layer. It exists
 * so a one-key theme file — `{"surface": "#1a1b26"}`, exactly what an external
 * theming tool writes first — is not black text on a black card.
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
 * Pure: a resolved overlay theme in, the custom properties to write out.
 *
 * A property appears only when its source token is set — plus the two groups
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

  // Both surface tokens feed one property. With either one unset its inherited
  // input is substituted, and because those inputs are literally today's, a
  // theme that sets only the opacity keeps a theme-aware card.
  const surface = validHex(theme.surface);
  const opacity = validToken(theme, "surface_opacity");
  if (surface !== null || opacity !== null || glass) {
    // The card paints this on every engine, Liquid Glass included. Liquid
    // Glass is handed the same colour natively as well, so that it can lens
    // it — but it is not trusted to be the only tint: measured on macOS 26, a
    // card that painted nothing and left the tint to `tintColor` alone came
    // out dark under a Light app theme, with the transcript unreadable on it.
    const alpha = opacity ?? SURFACE_OPACITY_INHERIT[material];
    vars["--s-surface"] = alphaMix(
      surface ?? "var(--color-background)",
      alpha * 100,
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

  // The card's edge. Its own two tokens, but it also follows `text`/`surface`
  // — an edge derived from a foreground the theme has replaced would be the
  // one neutral left behind. Under Glass it is written unconditionally, and at
  // a stronger alpha, because the edge is the only hard line a translucent
  // card has against whatever it is blurring.
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

  // Raw token values only: the CSS does every multiplication with
  // `calc(... * var(--ov-scale))`.
  const scale = validToken(theme, "size_scale");
  if (scale !== null) vars["--ov-scale"] = String(scale);

  const radius = validToken(theme, "radius");
  if (radius !== null) vars["--ov-radius"] = `${radius}px`;

  // The one length the native window geometry also reads: `overlay.rs` adds
  // two of these to the card's footprint, so the two sides must agree on the
  // number, which is why it is a token and not a derived value.
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

/**
 * Write a resolved overlay theme onto an element, and remove every property it
 * does not set.
 *
 * The removal is the point: without it a token that goes back to inherit would
 * keep painting, because inline style beats the stylesheet the inherited value
 * lives in. `data-material` is always set; a `null` theme removes every
 * property in [`OVERLAY_THEME_CSS_PROPERTIES`] and leaves
 * `data-material="flat"`.
 */
export function applyOverlayTheme(
  root: HTMLElement,
  resolved: ResolvedOverlayTheme | null,
): void {
  const vars = resolved ? resolveOverlayThemeVars(resolved) : {};
  for (const property of OVERLAY_THEME_CSS_PROPERTIES) {
    const value = vars[property];
    if (value === undefined) {
      root.style.removeProperty(property);
    } else {
      root.style.setProperty(property, value);
    }
  }
  root.dataset.material = resolved ? effectiveMaterialOf(resolved) : "flat";
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
 * Returns `null` — apply nothing, leave today's cascade — when the mirror is
 * missing, unreadable or not shaped like a payload.
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
 * Written only by the overlay window: the settings window has no root to paint,
 * so it never contributes to the mirror.
 */
export function storeOverlayTheme(resolved: ResolvedOverlayTheme): void {
  try {
    localStorage.setItem(OVERLAY_THEME_STORAGE_KEY, JSON.stringify(resolved));
  } catch {
    // localStorage may be unavailable; the resolved theme still arrives from
    // the backend on show, so this only costs failure tolerance on that path.
  }
}
