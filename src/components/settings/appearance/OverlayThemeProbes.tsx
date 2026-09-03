import React, { type CSSProperties } from "react";
import type { Material } from "@/bindings";
import type { OverlayColorKey } from "@/lib/overlayTheme";
import "./OverlayThemeProbes.css";

const PROBE_STYLE = (cssVar: string): CSSProperties => ({
  color: `var(${cssVar})`,
});

export interface OverlayThemeProbesProps {
  /** The overlay theme's custom properties, spread as inline style so the
   *  probes resolve exactly what the overlay would paint. */
  themeVars: CSSProperties;
  effectiveMaterial: Material;
  probeRefs: Record<OverlayColorKey, React.RefObject<HTMLSpanElement>>;
}

/**
 * Four invisible spans, one per colour token, each painted `var(--s-…)`.
 *
 * Reading `getComputedStyle(...).color` back off them is the only reliable way
 * to resolve a `color-mix()` custom property down to a hex, which is what an
 * unset colour field shows as its "resolved default" — the theme-aware value
 * it will actually inherit, rather than a hardcoded guess. Nothing is
 * rendered: this is a measuring device, not a preview.
 */
export const OverlayThemeProbes: React.FC<OverlayThemeProbesProps> = ({
  themeVars,
  effectiveMaterial,
  probeRefs,
}) => (
  <div
    aria-hidden="true"
    className="ov-theme-probes"
    data-material={effectiveMaterial}
    style={themeVars}
  >
    <span style={PROBE_STYLE("--s-accent")} ref={probeRefs.accent} />
    <span style={PROBE_STYLE("--s-surface")} ref={probeRefs.surface} />
    <span style={PROBE_STYLE("--s-text")} ref={probeRefs.text} />
    <span style={PROBE_STYLE("--s-border")} ref={probeRefs.border} />
  </div>
);
