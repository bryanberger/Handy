import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties } from "react";
import type { Material, OverlayTheme, ResolvedOverlayTheme } from "@/bindings";
import {
  INHERIT_ALL,
  resolveOverlayThemeVars,
  type OverlayColorKey,
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
 * token. File-owned controls are disabled, so no draft can exist for them in
 * practice; this makes the rule true even if one did.
 *
 * Exported for the unit tests; nothing outside this file imports it.
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
 * One numeric channel, which must **not** be followed by another digit, a dot
 * or a `%`. The `%` guard is the whole point: without it `rgb(100% 50% 25%)`
 * would match as `rgb(100, 50, 25)` and paint a plausible-looking but entirely
 * wrong color. A percentage form is a shape this parser does not handle, so it
 * must fail to match and let the caller fall back, not guess.
 */
const CHANNEL = String.raw`([\d.]+)(?![\d.%])`;
const COLOR_SRGB = new RegExp(
  String.raw`color\(srgb\s+${CHANNEL}\s+${CHANNEL}\s+${CHANNEL}`,
);
const RGB_FUNCTION = new RegExp(
  String.raw`rgba?\(\s*${CHANNEL}[,\s]+${CHANNEL}[,\s]+${CHANNEL}`,
);

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
 * opacity). Anything else — a percentage-channel form, a named color, a
 * `lab()` — returns `null`, and the caller shows a neutral placeholder rather
 * than a made-up hex.
 *
 * Exported for the unit tests; nothing outside this file imports it.
 */
export function parseComputedColor(value: string): string | null {
  const colorFn = value.match(COLOR_SRGB);
  if (colorFn) {
    return `#${clampedHex(Number(colorFn[1]) * 255)}${clampedHex(Number(colorFn[2]) * 255)}${clampedHex(Number(colorFn[3]) * 255)}`;
  }
  const rgbFn = value.match(RGB_FUNCTION);
  if (!rgbFn) return null;
  return `#${clampedHex(Number(rgbFn[1]))}${clampedHex(Number(rgbFn[2]))}${clampedHex(Number(rgbFn[3]))}`;
}

/**
 * Whether two flat maps hold the same entries.
 *
 * The preview hangs its layout effects off two such maps — the resolved custom
 * properties and the probed colours — and object *identity* is not a safe
 * stand-in for "unchanged" for either of them. React is free to re-derive a
 * `useState` value (it re-runs a functional updater on a re-render, and does so
 * twice per render under `StrictMode`), so `draft` can arrive as a fresh object
 * holding exactly the same tokens; anything memoized on its identity then
 * churns too. When a layout effect keyed on that identity also sets state
 * unconditionally, every commit leaves another sync update pending and React
 * eventually throws "Maximum update depth exceeded". Comparing by value at both
 * ends — the memo's key and the effect's payload — is what breaks that cycle.
 *
 * Exported for the unit tests; nothing outside this file imports it.
 */
export function sameStringMap(
  a: Record<string, string | null>,
  b: Record<string, string | null>,
): boolean {
  const keys = Object.keys(a);
  if (keys.length !== Object.keys(b).length) return false;
  return keys.every((key) => key in b && a[key] === b[key]);
}

/**
 * `next`, but with a reference that only changes when its entries do — the
 * "hold the last value" cache React documents for refs, so a fresh-but-equal
 * map cannot invalidate a dependency array.
 */
function useStableMap<T extends Record<string, string | null>>(next: T): T {
  const held = useRef(next);
  if (!sameStringMap(held.current, next)) held.current = next;
  return held.current;
}

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
  colorProbeRefs: Record<OverlayColorKey, React.RefObject<HTMLSpanElement>>;
  /** The probed, theme-aware default for each color token — what a ColorField
   *  shows (muted, italic) while that token is unset. `null` until the first
   *  measurement. */
  resolvedDefaults: Record<OverlayColorKey, string | null>;
  isLocked: (key: OverlayThemeKey) => boolean;
  /** The value a control should show: the file's value when locked, else the
   *  draft, else the persisted (resolved, pre-draft) value. */
  effectiveValue: <K extends OverlayThemeKey>(
    key: K,
  ) => NonNullable<OverlayTheme[K]> | null;
}

/**
 * Turns (draft ∪ resolved) overlay-theme tokens into the preview's inline
 * style, plus the theme-aware "resolved default" readback, so an unset color
 * field shows what it will actually inherit rather than a hardcoded guess.
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

  // Memoized so most renders reuse the same object. It is deliberately *not*
  // what the layout effect below keys off: `draft` can be re-derived into a
  // fresh-but-equal object, which would give this a new identity too (see
  // `sameStringMap`).
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

  // The one value the preview's layout effects (here and `OverlayPreview`'s
  // fit measurement) depend on, so its reference must change if and only if a
  // custom property actually changed.
  const previewVars = useStableMap(
    useMemo(() => resolveOverlayThemeVars(previewTheme), [previewTheme]),
  ) as unknown as CSSProperties;

  const accentRef = useRef<HTMLSpanElement>(null);
  const surfaceRef = useRef<HTMLSpanElement>(null);
  const textRef = useRef<HTMLSpanElement>(null);
  const borderRef = useRef<HTMLSpanElement>(null);
  // The four refs are stable for the hook's lifetime, but the object holding
  // them would be a fresh identity every render — and it is passed straight
  // into a child's props, so memoizing it keeps that child out of the same
  // "new object every render" trap that caused the render loop above.
  const colorProbeRefs = useMemo(
    () => ({
      accent: accentRef,
      surface: surfaceRef,
      text: textRef,
      border: borderRef,
    }),
    [],
  );
  const [resolvedDefaults, setResolvedDefaults] = useState<
    Record<OverlayColorKey, string | null>
  >({ accent: null, surface: null, text: null, border: null });

  const lastMeasured = useRef<Record<OverlayColorKey, string | null> | null>(
    null,
  );

  // Runs before paint, so the first frame already shows a measured value —
  // re-read whenever the vars we just wrote (or the app theme) could have
  // changed what the probes resolve to.
  useLayoutEffect(() => {
    const read = (ref: React.RefObject<HTMLSpanElement>) =>
      ref.current
        ? parseComputedColor(getComputedStyle(ref.current).color)
        : null;
    const measured = {
      accent: read(accentRef),
      surface: read(surfaceRef),
      text: read(textRef),
      border: read(borderRef),
    };
    // Skipping the call, rather than relying on React to bail out of it, is
    // the point: a *scheduled* update inside a layout effect counts towards
    // the nested-update limit even when it renders to the same tree.
    if (lastMeasured.current && sameStringMap(lastMeasured.current, measured)) {
      return;
    }
    lastMeasured.current = measured;
    setResolvedDefaults(measured);
    // `previewVars` only gets a new identity when a custom property actually
    // changed (`useStableMap`, above), which is exactly the "on any token
    // change" trigger this effect wants; the three refs are stable for the
    // component's lifetime and do not need to be listed.
  }, [previewVars, remeasureSignal]);

  const ownedKeys = useMemo(() => resolved?.file.owned_keys ?? [], [resolved]);
  const isLocked = useCallback(
    (key: OverlayThemeKey) => ownedKeys.includes(key),
    [ownedKeys],
  );

  const effectiveValue = useCallback(
    <K extends OverlayThemeKey>(
      key: K,
    ): NonNullable<OverlayTheme[K]> | null => {
      if (isLocked(key))
        return (baseTheme[key] ?? null) as NonNullable<OverlayTheme[K]> | null;
      const draftValue = draft[key];
      if (draftValue !== undefined)
        return (draftValue ?? null) as NonNullable<OverlayTheme[K]> | null;
      return (baseTheme[key] ?? null) as NonNullable<OverlayTheme[K]> | null;
    },
    [baseTheme, draft, isLocked],
  );

  return {
    previewTheme,
    previewVars,
    effectiveMaterial: previewTheme.effective_material,
    colorProbeRefs,
    resolvedDefaults,
    isLocked,
    effectiveValue,
  };
}
