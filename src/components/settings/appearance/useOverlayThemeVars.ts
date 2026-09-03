import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type { Material, OverlayTheme, ResolvedOverlayTheme } from "@/bindings";
import {
  INHERIT_ALL,
  resolveOverlayThemeVars,
  type OverlayThemeKey,
} from "@/lib/overlayTheme";

/** A theme file state to show before the first `resolved` payload arrives. */
export const EMPTY_FILE_STATE: ResolvedOverlayTheme["file"] = {
  path: "",
  present: false,
  version: null,
  tokens: INHERIT_ALL,
  owned_keys: [],
  diagnostics: [],
  diagnostics_total: 0,
  stale: false,
};

/**
 * Merge a draft on top of a resolved theme, per key, skipping any key the
 * theme file owns — a settings-level edit must never outrank a file-owned
 * token (ticket 08 §3d). File-owned controls are disabled, so no draft can
 * exist for them in practice; this makes the rule true even if one did.
 */
export function mergeDraft(
  theme: OverlayTheme,
  draft: Partial<OverlayTheme>,
  ownedKeys: readonly string[],
): OverlayTheme {
  const merged: OverlayTheme = { ...theme };
  (Object.keys(draft) as OverlayThemeKey[]).forEach((key) => {
    if (ownedKeys.includes(key)) return;
    const value = draft[key];
    if (value === undefined) return;
    // `draft` is built one key at a time from the same union `OverlayTheme`
    // is defined over, so this assignment is sound; TypeScript just can't
    // see the correlation across a key read at runtime.
    (merged as Record<OverlayThemeKey, unknown>)[key] = value;
  });
  return merged;
}

function clampedHex(n: number): string {
  return Math.max(0, Math.min(255, Math.round(n)))
    .toString(16)
    .padStart(2, "0");
}

/**
 * `getComputedStyle(...).color` resolves a `color-mix()` custom property down
 * to a plain color, but the serialization format varies with the browser and
 * with whether alpha is present. Current WebKit has been observed to use all
 * three of:
 *  - legacy comma syntax: `rgb(r, g, b)` / `rgba(r, g, b, a)`, 0-255 integers;
 *  - CSS Color 4 space syntax: `rgb(r g b / a)`, 0-255 integers;
 *  - CSS Color 4 `color()`: `color(srgb r g b)` / `color(srgb r g b / a)`,
 *    which is what a `color-mix()` computed *in* the srgb color space
 *    serializes to once alpha is involved — channels are 0-1 *fractions*,
 *    not 0-255 integers.
 * All three are accepted so the same probe works for a plain color token
 * (`accent`, `text`) and a translucent one (`surface`, mixed with its
 * opacity).
 */
export function parseComputedColor(value: string): string | null {
  const colorFn = value.match(/color\(srgb\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)/);
  if (colorFn) {
    return `#${clampedHex(Number(colorFn[1]) * 255)}${clampedHex(Number(colorFn[2]) * 255)}${clampedHex(Number(colorFn[3]) * 255)}`;
  }
  const rgbFn = value.match(/rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/);
  if (!rgbFn) return null;
  return `#${clampedHex(Number(rgbFn[1]))}${clampedHex(Number(rgbFn[2]))}${clampedHex(Number(rgbFn[3]))}`;
}

export type OverlayColorTokenKey = "accent" | "surface" | "text";

export interface UseOverlayThemeVarsResult {
  /** The resolved theme with the in-flight draft merged in — what the
   *  preview and its "Show on screen" counterpart should agree on. */
  previewTheme: ResolvedOverlayTheme;
  /** `resolveOverlayThemeVars(previewTheme)`, ready to spread as inline style
   *  on `.ov-preview`. */
  previewVars: CSSProperties;
  effectiveMaterial: Material;
  /** Attach one to a 0×0 `aria-hidden` span inside `.ov-preview` per color
   *  token, `style={{ color: "var(--s-accent)" }}` etc. — the only reliable
   *  way to resolve a `color-mix()` custom property down to a hex. */
  colorProbeRefs: Record<
    OverlayColorTokenKey,
    React.RefObject<HTMLSpanElement>
  >;
  /** The probed, theme-aware default for each color token — what a ColorField
   *  shows (muted, italic) while that token is unset. `null` until the first
   *  measurement. */
  resolvedDefaults: Record<OverlayColorTokenKey, string | null>;
  isLocked: (key: OverlayThemeKey) => boolean;
  /** The value a control should show: the file's value when locked, else the
   *  draft, else the persisted (resolved, pre-draft) value. */
  effectiveValue: <K extends OverlayThemeKey>(key: K) => OverlayTheme[K] | null;
}

/**
 * Turns (draft ∪ resolved) overlay-theme tokens into the preview's inline
 * style, plus the theme-aware "resolved default" readback ticket 03 §4 asks
 * for so an unset color field shows what it will actually inherit rather than
 * a hardcoded guess.
 *
 * `remeasureSignal` should be a value that changes whenever something outside
 * `resolved`/`draft` could change what a custom property resolves to — the
 * app theme, concretely, since `--color-logo-primary` etc. flip with it.
 */
export function useOverlayThemeVars(
  resolved: ResolvedOverlayTheme | null,
  draft: Partial<OverlayTheme>,
  remeasureSignal: unknown,
): UseOverlayThemeVarsResult {
  const baseTheme = resolved?.theme ?? INHERIT_ALL;

  // Memoized so the object identity is stable across renders that don't
  // actually change `resolved` or `draft` — the layout effect below depends
  // on `previewVars`, and without this it would be a *new* object on every
  // render (including ones caused by unrelated state elsewhere in the tree),
  // re-firing the effect, calling setState, causing another render: an
  // infinite "Maximum update depth exceeded" loop.
  const previewTheme: ResolvedOverlayTheme = useMemo(() => {
    return resolved
      ? {
          ...resolved,
          theme: mergeDraft(resolved.theme, draft, resolved.file.owned_keys),
        }
      : {
          theme: mergeDraft(INHERIT_ALL, draft, []),
          effective_material: "flat" as const,
          glass_support: { supported: false, available: false },
          file: EMPTY_FILE_STATE,
        };
  }, [resolved, draft]);

  const previewVars = useMemo(
    () => resolveOverlayThemeVars(previewTheme) as unknown as CSSProperties,
    [previewTheme],
  );

  const accentRef = useRef<HTMLSpanElement>(null);
  const surfaceRef = useRef<HTMLSpanElement>(null);
  const textRef = useRef<HTMLSpanElement>(null);
  const [resolvedDefaults, setResolvedDefaults] = useState<
    Record<OverlayColorTokenKey, string | null>
  >({ accent: null, surface: null, text: null });

  // Runs before paint, so the first frame already shows a measured value —
  // re-read whenever the vars we just wrote (or the app theme) could have
  // changed what the probes resolve to.
  useLayoutEffect(() => {
    const read = (ref: React.RefObject<HTMLSpanElement>) =>
      ref.current
        ? parseComputedColor(getComputedStyle(ref.current).color)
        : null;
    setResolvedDefaults({
      accent: read(accentRef),
      surface: read(surfaceRef),
      text: read(textRef),
    });
    // previewVars only gets a new identity when `resolved`/`draft` actually
    // change (it's memoized above), which is exactly the "on any token
    // change" trigger this effect wants; the three refs are stable for the
    // component's lifetime and do not need to be listed.
  }, [previewVars, remeasureSignal]);

  const ownedKeys = resolved?.file.owned_keys ?? [];
  const isLocked = (key: OverlayThemeKey) => ownedKeys.includes(key);

  const effectiveValue = <K extends OverlayThemeKey>(
    key: K,
  ): OverlayTheme[K] | null => {
    if (isLocked(key))
      return (baseTheme[key] ?? null) as OverlayTheme[K] | null;
    const draftValue = draft[key];
    if (draftValue !== undefined) return draftValue as OverlayTheme[K];
    return (baseTheme[key] ?? null) as OverlayTheme[K] | null;
  };

  return {
    previewTheme,
    previewVars,
    effectiveMaterial: previewTheme.effective_material,
    colorProbeRefs: { accent: accentRef, surface: surfaceRef, text: textRef },
    resolvedDefaults,
    isLocked,
    effectiveValue,
  };
}
