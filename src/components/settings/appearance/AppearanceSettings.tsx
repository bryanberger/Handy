import React from "react";
import { useTranslation } from "react-i18next";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { Slider } from "@/components/ui/Slider";
import { useResolvedOverlayTheme } from "@/hooks/useResolvedOverlayTheme";
import { useSettings } from "@/hooks/useSettings";
import {
  BORDER_OPACITY_INHERIT,
  INHERIT_ALL,
  SURFACE_OPACITY_INHERIT,
  type OverlayNumericKey,
} from "@/lib/overlayTheme";
import type { Material, OverlayStyle, OverlayTheme } from "@/bindings";
import { ShowOverlay } from "../ShowOverlay";
import { ThemeSelector } from "../ThemeSelector";
import { ColorField } from "./ColorField";
import { GlassMaterialSelector } from "./GlassMaterialSelector";
import { MaterialSelector } from "./MaterialSelector";
import { OnScreenPreview } from "./OnScreenPreview";
import {
  OVERLAY_TOKEN_FIELDS,
  type OverlayTokenField,
} from "./overlayTokenFields";
import { OverlayThemeProbes } from "./OverlayThemeProbes";
import { ThemeFileGroup } from "./ThemeFileGroup";
import { setOverlayThemeToken, useDraftSetting } from "./useDraftSetting";
import { EMPTY_FILE_STATE, useOverlayThemeVars } from "./useOverlayThemeVars";

/** Token contract defaults that do not vary with the app theme; mirrors
 *  `RecordingOverlay.css`'s `:root` block, which is the actual source of
 *  truth these must be kept in step with by hand. The two opacities are
 *  excluded — their defaults depend on the effective Material and are read
 *  from the apply layer's own tables instead, so they can never drift from
 *  what is actually painted. */
const STATIC_NUMERIC_DEFAULTS: Partial<Record<OverlayNumericKey, number>> = {
  size_scale: 1,
  radius: 24,
  border_width: 1,
  padding: 10,
  waveform_gap: 3,
  waveform_width: 4,
};

function numericDefault(
  key: OverlayNumericKey,
  effectiveMaterial: Material,
): number {
  if (key === "surface_opacity")
    return SURFACE_OPACITY_INHERIT[effectiveMaterial];
  if (key === "border_opacity")
    return BORDER_OPACITY_INHERIT[effectiveMaterial];
  return STATIC_NUMERIC_DEFAULTS[key] ?? 0;
}

function isOverlayThemeDefault(theme: OverlayTheme): boolean {
  return (Object.values(theme) as unknown[]).every(
    (value) => value === null || value === undefined,
  );
}

/**
 * The Appearance tab: the app theme picker and the overlay style/position
 * (both reused unchanged from About/Advanced), the on-screen preview, and the
 * fourteen overlay-theme tokens grouped as Color / Material / Size & Spacing,
 * plus the Theme File group. Groups 4 onward are driven by
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

      // The two enum tokens get their own selectors — Material for the Glass
      // gating and the unavailable note, the Glass material for its eight
      // option descriptions — rather than a generic dropdown, but both still
      // live in the descriptor table so the group is driven the same way as
      // Color and Size & Spacing.
      case "material":
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

      case "glassMaterial":
        return (
          <GlassMaterialSelector
            key={field.key}
            value={vars.effectiveValue(field.key) ?? "hud_window"}
            onSelect={(next) => void setOverlayThemeToken(field.key, next)}
            material={vars.effectiveValue("material") ?? "flat"}
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
        <OnScreenPreview
          style={style}
          settingsTheme={overlayTheme}
          resetDisabled={resetDisabled}
          hasThemeFileOwnership={hasThemeFileOwnership}
          onResetConfirm={() => void resetSetting("overlay_theme")}
          onFlushDrafts={flushAll}
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
