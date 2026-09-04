import React, {
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { CSSProperties } from "react";
import type { Material, OverlayTheme, ResolvedOverlayTheme } from "@/bindings";
import {
  INHERIT_ALL,
  OVERLAY_THEME_COLOR_PROPERTIES,
  resolveOverlayThemeVars,
  type OverlayColorKey,
  type OverlayThemeKey,
} from "@/lib/overlayTheme";
import { OverlayThemeProbes } from "./OverlayThemeProbes";

/** Theme file state shown before the first `resolved` payload arrives. */
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
 * Merge a draft over a resolved theme, per key, skipping keys the theme file
 * owns. A settings-level edit must never outrank a file-owned token. Those
 * controls are disabled, so no draft can exist for them; this holds anyway.
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
    // uses, so this is sound; TypeScript cannot correlate a runtime key read.
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
 * One numeric channel, not followed by another digit, a dot or a `%`. Without
 * the `%` guard `rgb(100% 50% 25%)` would match as `rgb(100, 50, 25)` and
 * paint a plausible but wrong color. This parser does not handle percentages,
 * so it must fail and let the caller fall back rather than guess.
 */
const CHANNEL = String.raw`([\d.]+)(?![\d.%])`;
const COLOR_SRGB = new RegExp(
  String.raw`color\(srgb\s+${CHANNEL}\s+${CHANNEL}\s+${CHANNEL}`,
);
const RGB_FUNCTION = new RegExp(
  String.raw`rgba?\(\s*${CHANNEL}[,\s]+${CHANNEL}[,\s]+${CHANNEL}`,
);

/**
 * `getComputedStyle(...).color` resolves a `color-mix()` custom property to a
 * plain color, but the serialization varies with the browser and with whether
 * alpha is present. Current WebKit has been observed to use all three of:
 *  - legacy comma syntax: `rgb(r, g, b)` / `rgba(r, g, b, a)`, 0-255 integers;
 *  - CSS Color 4 space syntax: `rgb(r g b / a)`, 0-255 integers;
 *  - CSS Color 4 `color()`: `color(srgb r g b)` / `color(srgb r g b / a)`,
 *    what an srgb `color-mix()` serializes to once alpha is involved. Channels
 *    are 0-1 fractions, not 0-255 integers.
 * All three are accepted so one probe works for a plain color token (`accent`,
 * `text`) and a translucent one (`surface`, mixed with its opacity). Anything
 * else returns `null` (percentage channels, a named color, `lab()`), and the
 * caller shows a neutral placeholder rather than a made-up hex.
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
 * Two such maps key the preview's layout effects, the resolved custom
 * properties and the probed colours, and identity is not a safe "unchanged"
 * test for either. React may re-derive a `useState` value (a functional updater
 * re-runs on a re-render, twice per render under `StrictMode`), so `draft` can
 * arrive fresh with the same tokens, and anything memoized on its identity
 * churns too. A layout effect keyed on that identity that also sets state
 * unconditionally leaves another sync update pending every commit, until React
 * throws "Maximum update depth exceeded". Comparing by value at both ends, the
 * memo's key and the effect's payload, breaks the cycle.
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
 * `next`, with a reference that changes only when its entries do. React's
 * documented "hold the last value" ref cache, so a fresh-but-equal map cannot
 * invalidate a dependency array.
 */
function useStableMap<T extends Record<string, string | null>>(next: T): T {
  const held = useRef(next);
  if (!sameStringMap(held.current, next)) held.current = next;
  return held.current;
}

export interface UseOverlayThemeVarsResult {
  /** The measuring device this hook reads `resolvedDefaults` off, already
   *  wired. Render it anywhere in the tab; nothing else is needed. Hiding it
   *  here rather than handing the caller its three props is the point, since
   *  mounting it wrong would silently leave every resolved default `null`. */
  probes: React.ReactElement;
  effectiveMaterial: Material;
  /** The probed, theme-aware default per color token, shown by a ColorField
   *  (muted, italic) while unset. `null` until the first measurement. */
  resolvedDefaults: Record<OverlayColorKey, string | null>;
  isLocked: (key: OverlayThemeKey) => boolean;
  /** The value a control shows: the file's value when locked, else the draft,
   *  else the persisted (resolved, pre-draft) value. */
  effectiveValue: <K extends OverlayThemeKey>(
    key: K,
  ) => NonNullable<OverlayTheme[K]> | null;
}

/**
 * Turns (draft ∪ resolved) overlay-theme tokens into the probe host's inline
 * style, plus the theme-aware "resolved default" readback, so an unset color
 * field shows what it will inherit rather than a hardcoded guess.
 *
 * `remeasureSignal` should change whenever something outside `resolved`/`draft`
 * could change what a custom property resolves to, which in practice is the app
 * theme, since `--color-logo-primary` etc. flip with it.
 */
export function useOverlayThemeVars(
  resolved: ResolvedOverlayTheme | null,
  draft: Partial<OverlayTheme>,
  remeasureSignal: unknown,
): UseOverlayThemeVarsResult {
  const baseTheme = resolved?.theme ?? INHERIT_ALL;

  // Memoized so most renders reuse the same object, but deliberately not what
  // the layout effect below keys off. A re-derived fresh-but-equal `draft`
  // would give this a new identity too (see `sameStringMap`).
  const mergedTheme: ResolvedOverlayTheme = useMemo(() => {
    return resolved
      ? {
          ...resolved,
          theme: mergeDraft(resolved.theme, draft, resolved.file.owned_keys),
        }
      : {
          theme: mergeDraft(INHERIT_ALL, draft, []),
          effective_material: "flat" as const,
          // No resolved theme means no window, so the shadow has no screen edge
          // to keep clear of.
          shadow_edge_slack: 0,
          glass_support: {
            supported: false,
            available: false,
            engine: "none" as const,
          },
          file: EMPTY_FILE_STATE,
        };
  }, [resolved, draft]);

  // What the probe host wears and what the readback effect below depends on, so
  // its reference must change exactly when a probe-visible custom property
  // changed. Lengths are left out on purpose. No `--s-…` colour derives from
  // `--ov-scale` or `--ov-radius`, so carrying them would re-style the host and
  // force four pointless style recalculations per frame of a Size Scale drag.
  const colorVars = useStableMap(
    useMemo(() => {
      const vars = resolveOverlayThemeVars(mergedTheme);
      return Object.fromEntries(
        OVERLAY_THEME_COLOR_PROPERTIES.filter(
          (property) => vars[property] !== undefined,
        ).map((property) => [property, vars[property]]),
      );
    }, [mergedTheme]),
  );
  // Only the colours reach the probe host, which exists to read colours back. A
  // length written there would re-style it every frame of a Size Scale drag to
  // re-measure four values that cannot have moved.
  const probeVars = colorVars as unknown as CSSProperties;

  const accentRef = useRef<HTMLSpanElement>(null);
  const surfaceRef = useRef<HTMLSpanElement>(null);
  const textRef = useRef<HTMLSpanElement>(null);
  const borderRef = useRef<HTMLSpanElement>(null);
  // The four refs are stable for the hook's lifetime, but the object holding
  // them would be a fresh identity every render. It goes straight into a
  // child's props, so memoizing it keeps that child out of the "new object
  // every render" trap that caused the render loop above.
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

  // Runs before paint, so the first frame shows a measured value. Re-reads when
  // the new vars or the app theme could change what the probes resolve to.
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
    // Skip the call rather than rely on React to bail out of it. A scheduled
    // update inside a layout effect counts towards the nested-update limit even
    // when it renders to the same tree.
    if (lastMeasured.current && sameStringMap(lastMeasured.current, measured)) {
      return;
    }
    lastMeasured.current = measured;
    setResolvedDefaults(measured);
    // `colorVars` gets a new identity only when a colour custom property
    // actually changed (`useStableMap`, above), exactly the trigger this effect
    // wants. `effectiveMaterial` is listed because it is written onto the probe
    // host and the neutrals are mixed per Material. The four refs are stable
    // for the component's lifetime and need no listing.
  }, [colorVars, mergedTheme.effective_material, remeasureSignal]);

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
    probes: (
      <OverlayThemeProbes
        probeVars={probeVars}
        effectiveMaterial={mergedTheme.effective_material}
        probeRefs={colorProbeRefs}
      />
    ),
    effectiveMaterial: mergedTheme.effective_material,
    resolvedDefaults,
    isLocked,
    effectiveValue,
  };
}
