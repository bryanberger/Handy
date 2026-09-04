import React, { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { Slider } from "@/components/ui/Slider";
import { useResolvedOverlayTheme } from "@/hooks/useResolvedOverlayTheme";
import { useSettings } from "@/hooks/useSettings";
import {
  BORDER_OPACITY_INHERIT,
  GLASS_TINT_INHERIT,
  INHERIT_ALL,
  SURFACE_OPACITY_INHERIT,
  type OverlayNumericKey,
} from "@/lib/overlayTheme";
import type {
  GlassSupport,
  Material,
  OverlayStyle,
  OverlayTheme,
} from "@/bindings";
import { ShowOverlay } from "../ShowOverlay";
import { ThemeSelector } from "../ThemeSelector";
import { ColorField } from "./ColorField";
import { GlassStyleSelector } from "./GlassStyleSelector";
import { MaterialSelector } from "./MaterialSelector";
import { OnScreenPreview } from "./OnScreenPreview";
import {
  overlayTokenFieldsFor,
  type OverlayTokenField,
} from "./overlayTokenFields";
import type { PreviewChange, PreviewChangeRequest } from "./previewMode";
import { OverlayThemeProbes } from "./OverlayThemeProbes";
import { ThemeFileGroup } from "./ThemeFileGroup";
import { setOverlayThemeToken, useDraftSetting } from "./useDraftSetting";
import { EMPTY_FILE_STATE, useOverlayThemeVars } from "./useOverlayThemeVars";

/** Token contract defaults that do not vary with the app theme; mirrors
 *  `RecordingOverlay.css`'s `:root` block, which is the actual source of
 *  truth these must be kept in step with by hand. The three alphas are
 *  excluded — theirs live in the apply layer, beside the composition that
 *  reads them, so they can never drift from what is actually painted. */
const STATIC_NUMERIC_DEFAULTS: Partial<Record<OverlayNumericKey, number>> = {
  size_scale: 1,
  radius: 24,
  border_width: 1,
  padding: 10,
  waveform_gap: 3,
  waveform_width: 4,
};

function numericDefault(key: OverlayNumericKey, material: Material): number {
  if (key === "surface_opacity") return SURFACE_OPACITY_INHERIT;
  if (key === "glass_tint") return GLASS_TINT_INHERIT;
  if (key === "border_opacity") return BORDER_OPACITY_INHERIT[material];
  return STATIC_NUMERIC_DEFAULTS[key] ?? 0;
}

/** What the tab assumes before the first resolved payload arrives: no Glass,
 *  and so no engine. */
const NO_GLASS: GlassSupport = {
  supported: false,
  available: false,
  engine: "none",
};

function isOverlayThemeDefault(theme: OverlayTheme): boolean {
  return (Object.values(theme) as unknown[]).every(
    (value) => value === null || value === undefined,
  );
}

/**
 * The Appearance tab: the app theme picker and the overlay style/position
 * (both reused unchanged from About/Advanced), the on-screen preview, and the
 * overlay-theme tokens grouped as Color / Material / Size & Spacing, plus the
 * Theme File group. Groups 4 onward are driven by
 * `OVERLAY_TOKEN_FIELDS` rather than hardcoded, so that table is the only
 * place a token's shape is declared.
 */
export const AppearanceSettings: React.FC = () => (
  <ErrorBoundary context="Appearance tab">
    <AppearanceSettingsInner />
  </ErrorBoundary>
);

const AppearanceSettingsInner: React.FC = () => {
  const { t } = useTranslation();
  const { settings, isUpdating, resetSetting } = useSettings();
  const { resolved, isReloading, reload } = useResolvedOverlayTheme();
  const { draft, setDraft, flush, flushAll, reset } = useDraftSetting();

  const style: OverlayStyle = settings?.overlay_style ?? "live";

  // What the user last did to the surface itself, handed to the preview card,
  // which decides whether it is worth putting the overlay on screen. The
  // counter makes two identical picks two requests.
  const [lastSurfaceChange, setLastSurfaceChange] =
    useState<PreviewChangeRequest | null>(null);
  const reportSurfaceChange = useCallback((change: PreviewChange) => {
    setLastSurfaceChange((previous) => ({
      change,
      seq: (previous?.seq ?? 0) + 1,
    }));
  }, []);

  const vars = useOverlayThemeVars(resolved, draft, settings?.theme);
  const glassSupport = resolved?.glass_support ?? NO_GLASS;

  const overlayTheme = settings?.overlay_theme ?? INHERIT_ALL;
  const resettingWhole = isUpdating("overlay_theme");
  const resetDisabled = isOverlayThemeDefault(overlayTheme) || resettingWhole;
  const hasThemeFileOwnership = (resolved?.file.owned_keys.length ?? 0) > 0;
  const lockedDescription = t(
    "settings.appearance.themeFile.lockedDescription",
  );

  const renderField = (field: OverlayTokenField) => {
    const locked = vars.isLocked(field.key);

    switch (field.kind) {
      case "color":
        return (
          <ColorField
            key={field.key}
            label={t(field.labelKey)}
            description={t(field.descriptionKey)}
            value={vars.effectiveValue(field.key)}
            resolvedDefault={vars.resolvedDefaults[field.key]}
            onChange={(hex) => setDraft(field.key, hex)}
            onCommitNow={() => void flush(field.key)}
            onReset={() => reset(field.key)}
            locked={locked}
            lockedDescription={lockedDescription}
            isResetting={resettingWhole}
          />
        );

      case "length":
      case "factor": {
        const isLength = field.kind === "length";
        const value =
          vars.effectiveValue(field.key) ??
          numericDefault(field.key, vars.effectiveMaterial);
        return (
          <div
            key={field.key}
            onPointerUp={() => void flush(field.key)}
            // React's synthetic onBlur bubbles (unlike the native `blur`
            // event), so this fires when the range input inside loses focus —
            // e.g. tabbing away mid-drag, which onPointerUp alone would miss.
            onBlur={() => void flush(field.key)}
          >
            <Slider
              grouped
              descriptionMode="tooltip"
              label={t(field.labelKey)}
              description={locked ? lockedDescription : t(field.descriptionKey)}
              value={value}
              onChange={(next) => setDraft(field.key, next)}
              min={field.min}
              max={field.max}
              step={field.step}
              disabled={locked}
              formatValue={(v) =>
                isLength ? `${Math.round(v)}px` : `${v.toFixed(2)}×`
              }
              onReset={() => reset(field.key)}
              isResetting={resettingWhole}
            />
          </div>
        );
      }

      // The two enum tokens with a row get their own selectors — Material for
      // the Glass gating and the unavailable note, the Glass style for its
      // engine gating — rather than a generic dropdown, but both still live in
      // the descriptor table so the group is driven the same way as Color and
      // Size & Spacing.
      case "material": {
        const current = vars.effectiveValue(field.key) ?? "flat";
        return (
          <MaterialSelector
            key={field.key}
            value={current}
            onSelect={(next) => {
              if (next !== current)
                reportSurfaceChange({ kind: "material", to: next });
              void setOverlayThemeToken(field.key, next);
            }}
            glassSupport={glassSupport}
            locked={locked}
            lockedDescription={lockedDescription}
          />
        );
      }

      case "glassStyle": {
        const current = vars.effectiveValue(field.key) ?? "regular";
        return (
          <GlassStyleSelector
            key={field.key}
            value={current}
            onSelect={(next) => {
              if (next !== current) reportSurfaceChange({ kind: "glassStyle" });
              void setOverlayThemeToken(field.key, next);
            }}
            material={vars.effectiveValue("material") ?? "flat"}
            glassSupport={glassSupport}
            locked={locked}
            lockedDescription={lockedDescription}
          />
        );
      }
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.appearance.groups.app")}>
        <ThemeSelector descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.appearance.groups.overlay")}>
        <ShowOverlay descriptionMode="tooltip" grouped={true} />
      </SettingsGroup>

      <SettingsGroup title={t("settings.appearance.groups.preview")}>
        <OnScreenPreview
          style={style}
          settingsTheme={overlayTheme}
          resetDisabled={resetDisabled}
          hasThemeFileOwnership={hasThemeFileOwnership}
          onResetConfirm={() => void resetSetting("overlay_theme")}
          onFlushDrafts={flushAll}
          lastSurfaceChange={lastSurfaceChange}
          glassAvailable={glassSupport.available}
        />
      </SettingsGroup>

      {/* Not a preview: an off-screen measuring device the colour fields read
          their "resolved default" back off. Mounted whenever the tab is, so
          the refs are attached before the fields below ask for a reading —
          and it costs nothing while the overlay is off and they are hidden. */}
      <OverlayThemeProbes
        themeVars={vars.themeVars}
        effectiveMaterial={vars.effectiveMaterial}
        probeRefs={vars.colorProbeRefs}
      />

      {style !== "none" && (
        <>
          {/* Each group shows the rows its Material has: under Flat the
              surface opacity, under Glass the tint strength in its place. */}
          <SettingsGroup title={t("settings.appearance.groups.color")}>
            {overlayTokenFieldsFor("color", vars.effectiveMaterial).map(
              renderField,
            )}
          </SettingsGroup>

          <SettingsGroup title={t("settings.appearance.groups.material")}>
            {overlayTokenFieldsFor("material", vars.effectiveMaterial).map(
              renderField,
            )}
          </SettingsGroup>

          <SettingsGroup title={t("settings.appearance.groups.size")}>
            {overlayTokenFieldsFor("size", vars.effectiveMaterial).map(
              renderField,
            )}
          </SettingsGroup>

          <SettingsGroup title={t("settings.appearance.groups.themeFile")}>
            <ThemeFileGroup
              file={resolved?.file ?? EMPTY_FILE_STATE}
              material={vars.effectiveMaterial}
              onReload={() => void reload()}
              isReloading={isReloading}
            />
          </SettingsGroup>
        </>
      )}
    </div>
  );
};
