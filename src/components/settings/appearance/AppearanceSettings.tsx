import React, { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import { useResolvedOverlayTheme } from "@/hooks/useResolvedOverlayTheme";
import { useSettings } from "@/hooks/useSettings";
import {
  BOOLEAN_INHERIT,
  inheritedTokenValue,
  INHERIT_ALL,
  SHADOW_STRENGTH_INHERIT,
  type OverlayBooleanKey,
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
import { OverlaySwitchRow } from "./OverlaySwitchRow";
import {
  overlayTokenFieldsFor,
  type OverlayTokenField,
} from "./overlayTokenFields";
import type { PreviewChange, PreviewChangeRequest } from "./previewMode";
import { OverlayTokenRow } from "./OverlayTokenRow";
import { ThemeFileGroup } from "./ThemeFileGroup";
import { setOverlayThemeToken, useDraftSetting } from "./useDraftSetting";
import { EMPTY_FILE_STATE, useOverlayThemeVars } from "./useOverlayThemeVars";

/** Assumed before the first resolved payload arrives. No Glass, no engine. */
const NO_GLASS: GlassSupport = {
  supported: false,
  available: false,
  engine: "none",
};

// A switch is one value, not the tail of a drag, so it commits straight
// through, as the Material and the Glass style do. One handler per switch,
// declared here rather than in the component, so every switch row keeps the
// same `onChange` for the tab's life and stays memoised through a slider drag.
const TOGGLE_HANDLERS: Record<OverlayBooleanKey, (next: boolean) => void> = {
  show_waveform: (next) => {
    void setOverlayThemeToken("show_waveform", next);
  },
  show_cancel: (next) => {
    void setOverlayThemeToken("show_cancel", next);
  },
};

// Under Glass the shadow is macOS's own and `NSWindow` offers no strength, so
// the switch writes the two ends of the token's range rather than a boolean
// the contract has no room for.
const handleGlassShadow = (next: boolean) => {
  void setOverlayThemeToken("shadow_strength", next ? 1 : 0);
};

function isOverlayThemeDefault(theme: OverlayTheme): boolean {
  return (Object.values(theme) as unknown[]).every(
    (value) => value === null || value === undefined,
  );
}

/**
 * The Appearance tab. App theme picker, overlay style/position (both reused
 * unchanged from About/Advanced), on-screen preview, overlay-theme tokens as
 * Color / Material / Elements / Size & Spacing, and the Theme File group.
 * Groups 4 on read `OVERLAY_TOKEN_FIELDS` rather than hardcoded rows, so that
 * table alone declares a token's shape.
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
  // Whether the overlay is this tab's to repaint live. The preview card owns
  // it, the only thing that knows; else a drag sends a refused draft per frame.
  const [overlayIsOurs, setOverlayIsOurs] = useState(false);
  const { draft, setDraft, flush, flushAll, reset } =
    useDraftSetting(overlayIsOurs);

  const style: OverlayStyle = settings?.overlay_style ?? "live";

  // The user's last surface change, for the preview card to decide whether to
  // show the overlay. The counter makes two identical picks two requests.
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

  // Stable for the tab's life, so a memoised row's props change only with its
  // own value (see `OverlayTokenRow`). `setDraft`, `flush` and `reset` are
  // stable already (`useDraftSetting`); this drops the two unawaited promises.
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
      // The two enum tokens with a row get their own selectors, not a generic
      // dropdown. Material handles the Glass gating and the unavailable note,
      // the Glass style its engine gating. Both still live in the descriptor
      // table, so the group is driven like Color and Size & Spacing.
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

      case "toggle":
        return (
          <OverlaySwitchRow
            key={field.key}
            labelKey={field.labelKey}
            descriptionKey={field.descriptionKey}
            checked={
              vars.effectiveValue(field.key) ?? BOOLEAN_INHERIT[field.key]
            }
            onChange={TOGGLE_HANDLERS[field.key]}
            locked={locked}
            lockedDescription={lockedDescription}
          />
        );

      case "glassShadow":
        return (
          <OverlaySwitchRow
            key={`${field.key}-glass`}
            labelKey={field.labelKey}
            descriptionKey={field.descriptionKey}
            noteKey={field.noteKey}
            checked={
              (vars.effectiveValue(field.key) ??
                SHADOW_STRENGTH_INHERIT[vars.effectiveMaterial]) > 0
            }
            onChange={handleGlassShadow}
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
              inheritedTokenValue(
                field.key,
                vars.effectiveMaterial,
                currentGlassStyle,
              )
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

      {/* An off-screen measuring device, not a preview. The colour fields read
          their "resolved default" off it, wired by the hook that reads it.
          Mounted with the tab, so the refs attach before the fields ask for a
          reading. Costs nothing while the overlay is off and the probes hidden. */}
      {vars.probes}

      {style !== "none" && (
        <>
          {/* Each group shows the rows its Material has: under Flat the surface
              opacity, under Glass the tint strength in its place. */}
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

          <SettingsGroup title={t("settings.appearance.groups.elements")}>
            {overlayTokenFieldsFor("elements", vars.effectiveMaterial).map(
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
