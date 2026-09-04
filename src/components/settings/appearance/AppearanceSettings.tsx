import React, { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { useResolvedOverlayTheme } from "@/hooks/useResolvedOverlayTheme";
import { useSettings } from "@/hooks/useSettings";
import {
  inheritedTokenValue,
  INHERIT_ALL,
  type OverlayThemeKey,
} from "@/lib/overlayTheme";
import type {
  GlassStyle,
  GlassSupport,
  Material,
  OverlayStyle,
  OverlayTheme,
} from "@/bindings";
import { ShowOverlay } from "../ShowOverlay";
import { ThemeSelector } from "../ThemeSelector";
import { GlassStyleSelector } from "./GlassStyleSelector";
import { MaterialSelector } from "./MaterialSelector";
import { OnScreenPreview } from "./OnScreenPreview";
import {
  overlayTokenFieldsFor,
  type OverlayTokenField,
} from "./overlayTokenFields";
import type { PreviewChange, PreviewChangeRequest } from "./previewMode";
import { OverlayTokenRow } from "./OverlayTokenRow";
import { ThemeFileGroup } from "./ThemeFileGroup";
import { setOverlayThemeToken, useDraftSetting } from "./useDraftSetting";
import { EMPTY_FILE_STATE, useOverlayThemeVars } from "./useOverlayThemeVars";

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
  // Whether the overlay on screen is this tab's to repaint live. Owned by the
  // preview card below, which is the only thing that knows; without it a drag
  // would send a draft to the backend every frame for it to refuse.
  const [overlayIsOurs, setOverlayIsOurs] = useState(false);
  const { draft, setDraft, flush, flushAll, reset } =
    useDraftSetting(overlayIsOurs);

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

  // Stable for the tab's lifetime, so a memoised row's props only change when
  // its own value does — see `OverlayTokenRow`. `setDraft`, `flush` and
  // `reset` are already stable (`useDraftSetting`); these only drop the
  // promises the two async ones return, which no caller here awaits.
  const handleFlush = useCallback(
    (key: OverlayThemeKey) => void flush(key),
    [flush],
  );
  const currentMaterial = vars.effectiveValue("material") ?? "flat";
  const currentGlassStyle = vars.effectiveValue("glass_style") ?? "regular";
  const handleSelectMaterial = useCallback(
    (next: Material) => {
      if (next !== currentMaterial)
        reportSurfaceChange({ kind: "material", to: next });
      void setOverlayThemeToken("material", next);
    },
    [currentMaterial, reportSurfaceChange],
  );
  const handleSelectGlassStyle = useCallback(
    (next: GlassStyle) => {
      if (next !== currentGlassStyle)
        reportSurfaceChange({ kind: "glassStyle" });
      void setOverlayThemeToken("glass_style", next);
    },
    [currentGlassStyle, reportSurfaceChange],
  );

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
      // The two enum tokens with a row get their own selectors — Material for
      // the Glass gating and the unavailable note, the Glass style for its
      // engine gating — rather than a generic dropdown, but both still live in
      // the descriptor table so the group is driven the same way as Color and
      // Size & Spacing.
      case "material":
        return (
          <MaterialSelector
            key={field.key}
            value={currentMaterial}
            onSelect={handleSelectMaterial}
            glassSupport={glassSupport}
            locked={locked}
            lockedDescription={lockedDescription}
          />
        );

      case "glassStyle":
        return (
          <GlassStyleSelector
            key={field.key}
            value={currentGlassStyle}
            onSelect={handleSelectGlassStyle}
            material={currentMaterial}
            glassSupport={glassSupport}
            locked={locked}
            lockedDescription={lockedDescription}
          />
        );

      case "color":
        return (
          <OverlayTokenRow
            key={field.key}
            field={field}
            value={vars.effectiveValue(field.key)}
            resolvedDefault={vars.resolvedDefaults[field.key]}
            locked={locked}
            lockedDescription={lockedDescription}
            isResetting={resettingWhole}
            onDraft={setDraft}
            onFlush={handleFlush}
            onReset={reset}
          />
        );

      case "length":
      case "factor":
        return (
          <OverlayTokenRow
            key={field.key}
            field={field}
            value={
              vars.effectiveValue(field.key) ??
              inheritedTokenValue(field.key, vars.effectiveMaterial)
            }
            locked={locked}
            lockedDescription={lockedDescription}
            isResetting={resettingWhole}
            onDraft={setDraft}
            onFlush={handleFlush}
            onReset={reset}
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
          lastSurfaceChange={lastSurfaceChange}
          glassAvailable={glassSupport.available}
          onAcceptsDraftsChange={setOverlayIsOurs}
        />
      </SettingsGroup>

      {/* Not a preview: an off-screen measuring device the colour fields read
          their "resolved default" back off, wired by the hook that reads it.
          Mounted whenever the tab is, so the refs are attached before the
          fields below ask for a reading — and it costs nothing while the
          overlay is off and they are hidden. */}
      {vars.probes}

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
