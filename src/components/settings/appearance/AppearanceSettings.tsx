import React from "react";
import { useTranslation } from "react-i18next";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { Slider } from "@/components/ui/Slider";
import { useResolvedOverlayTheme } from "@/hooks/useResolvedOverlayTheme";
import { useSettings } from "@/hooks/useSettings";
import {
  INHERIT_ALL,
  SURFACE_OPACITY_INHERIT,
  type OverlayNumericKey,
} from "@/lib/overlayTheme";
import { getLanguageDirection } from "@/lib/utils/rtl";
import type {
  Material,
  OverlayPosition,
  OverlayStyle,
  OverlayTheme,
} from "@/bindings";
import { ShowOverlay } from "../ShowOverlay";
import { ThemeSelector } from "../ThemeSelector";
import { ColorField } from "./ColorField";
import { MaterialSelector } from "./MaterialSelector";
import {
  OVERLAY_TOKEN_FIELDS,
  type OverlayTokenField,
} from "./overlayTokenFields";
import { OverlayPreview } from "./OverlayPreview";
import { ThemeFileGroup } from "./ThemeFileGroup";
import { setOverlayThemeToken, useDraftSetting } from "./useDraftSetting";
import { EMPTY_FILE_STATE, useOverlayThemeVars } from "./useOverlayThemeVars";

/** Token contract defaults that do not vary with the app theme (ticket 02
 *  §2); mirrors `RecordingOverlay.css`'s `:root` block, which is the actual
 *  source of truth these must be kept in step with by hand. `surface_opacity`
 *  is excluded — its default depends on the effective Material and is read
 *  from the apply layer's own `SURFACE_OPACITY_INHERIT` instead, so it can
 *  never drift from what is actually painted. */
const STATIC_NUMERIC_DEFAULTS: Partial<Record<OverlayNumericKey, number>> = {
  size_scale: 1,
  radius: 24,
  padding: 10,
  waveform_gap: 3,
};

function numericDefault(
  key: OverlayNumericKey,
  effectiveMaterial: Material,
): number {
  if (key === "surface_opacity")
    return SURFACE_OPACITY_INHERIT[effectiveMaterial];
  return STATIC_NUMERIC_DEFAULTS[key] ?? 0;
}

function isOverlayThemeDefault(theme: OverlayTheme): boolean {
  return (Object.values(theme) as unknown[]).every(
    (value) => value === null || value === undefined,
  );
}

/**
 * The Appearance tab: the app theme picker and the overlay style/position
 * (both reused unchanged from About/Advanced), a live preview, and the nine
 * overlay-theme tokens grouped as Color / Material / Size & Spacing, plus the
 * Theme File group. Groups 4 onward are driven by `OVERLAY_TOKEN_FIELDS`
 * rather than hardcoded, so ticket 02's token table is the only place a
 * token's shape is declared.
 */
export const AppearanceSettings: React.FC = () => (
  <ErrorBoundary context="Appearance tab">
    <AppearanceSettingsInner />
  </ErrorBoundary>
);

const AppearanceSettingsInner: React.FC = () => {
  const { t, i18n } = useTranslation();
  const direction = getLanguageDirection(i18n.language);
  const { settings, isUpdating, resetSetting } = useSettings();
  const { resolved, isReloading, reload } = useResolvedOverlayTheme();
  const { draft, setDraft, flush, flushAll, reset } = useDraftSetting();

  const style: OverlayStyle = settings?.overlay_style ?? "live";
  const position: OverlayPosition =
    settings?.overlay_position === "top" ? "top" : "bottom";

  const vars = useOverlayThemeVars(resolved, draft, settings?.theme);

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

      // The one enum token, Material, gets its own selector (the Glass
      // gating and the unavailable note) rather than a generic dropdown, but
      // still lives in the descriptor table so the group is driven the same
      // way as Color and Size & Spacing.
      case "enum":
        return (
          <MaterialSelector
            key={field.key}
            value={vars.effectiveValue(field.key) ?? "flat"}
            onSelect={(next) => void setOverlayThemeToken(field.key, next)}
            glassSupport={
              resolved?.glass_support ?? { supported: false, available: false }
            }
            locked={locked}
            lockedDescription={lockedDescription}
          />
        );
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
        {style === "none" ? (
          <div className="p-4 text-sm text-mid-gray">
            {t("settings.appearance.preview.noneNote")}
          </div>
        ) : (
          <OverlayPreview
            style={style}
            position={position}
            direction={direction}
            previewTheme={vars.previewTheme}
            settingsTheme={overlayTheme}
            previewVars={vars.previewVars}
            colorProbeRefs={vars.colorProbeRefs}
            resetDisabled={resetDisabled}
            hasThemeFileOwnership={hasThemeFileOwnership}
            onResetConfirm={() => void resetSetting("overlay_theme")}
            onFlushDrafts={flushAll}
          />
        )}
      </SettingsGroup>

      {style !== "none" && (
        <>
          <SettingsGroup title={t("settings.appearance.groups.color")}>
            {OVERLAY_TOKEN_FIELDS.filter((f) => f.group === "color").map(
              renderField,
            )}
          </SettingsGroup>

          <SettingsGroup title={t("settings.appearance.groups.material")}>
            {OVERLAY_TOKEN_FIELDS.filter((f) => f.group === "material").map(
              renderField,
            )}
          </SettingsGroup>

          <SettingsGroup title={t("settings.appearance.groups.size")}>
            {OVERLAY_TOKEN_FIELDS.filter((f) => f.group === "size").map(
              renderField,
            )}
          </SettingsGroup>

          <SettingsGroup title={t("settings.appearance.groups.themeFile")}>
            <ThemeFileGroup
              file={resolved?.file ?? EMPTY_FILE_STATE}
              onReload={() => void reload()}
              isReloading={isReloading}
            />
          </SettingsGroup>
        </>
      )}
    </div>
  );
};
