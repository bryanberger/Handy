import React, { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { SettingsGroup } from "@/components/ui/SettingsGroup";
import {
  commitOverlayTheme,
  useResolvedOverlayTheme,
} from "@/hooks/useResolvedOverlayTheme";
import { useSettings } from "@/hooks/useSettings";
import {
  BOOLEAN_INHERIT,
  inheritedTokenValue,
  INHERIT_ALL,
  SHADOW_STRENGTH_INHERIT,
  WAVEFORM_STYLE_INHERIT,
  type OverlayBooleanKey,
  type OverlayThemeKey,
} from "@/lib/overlayTheme";
import type {
  GlassStyle,
  GlassSupport,
  Material,
  OverlayStyle,
  OverlayTheme,
  WaveformStyle,
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
import { WaveformStyleSelector } from "./WaveformStyleSelector";
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
// declared outside the component so every switch row keeps one `onChange` for
// the tab's life and stays memoised through a slider drag.
const TOGGLE_HANDLERS: Record<OverlayBooleanKey, (next: boolean) => void> = {
  show_waveform: (next) => {
    void setOverlayThemeToken("show_waveform", next);
  },
  show_cancel: (next) => {
    void setOverlayThemeToken("show_cancel", next);
  },
};

// Under Glass macOS owns the shadow and `NSWindow` offers no strength, so the
// switch writes both range ends, not a boolean the contract lacks.
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
 * Color / Material / Elements / Size & Spacing / Waveform, and the Theme File
 * group. Groups 4 on read `OVERLAY_TOKEN_FIELDS`, not hardcoded rows, so that
 * table alone declares a token's shape.
 *
 * Every token row writes `overlay_theme.json`, so the tab is an editor for
 * that file rather than a second place a theme could live. The rows go
 * read-only together when the file is one Handy reads and does not write.
 */
export const AppearanceSettings: React.FC = () => (
  <ErrorBoundary context="Appearance tab">
    <AppearanceSettingsInner />
  </ErrorBoundary>
);

const AppearanceSettingsInner: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const { resolved, isReloading, isCommitting, reload } =
    useResolvedOverlayTheme();
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
  const currentWaveformStyle =
    vars.effectiveValue("waveform_style") ?? WAVEFORM_STYLE_INHERIT;
  // The whole Waveform group goes with the waveform: none of its rows could
  // change anything while the card draws none.
  const showsWaveform =
    vars.effectiveValue("show_waveform") ?? BOOLEAN_INHERIT.show_waveform;
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
  // An enum is one value, not the tail of a drag, so it commits straight
  // through, as the Material and the Glass style do.
  const handleSelectWaveformStyle = useCallback((next: WaveformStyle) => {
    void setOverlayThemeToken("waveform_style", next);
  }, []);

  // The theme file is the overlay theme, so the persisted tokens are its own.
  const overlayTheme = resolved?.theme ?? INHERIT_ALL;
  const locked = vars.locked;
  const resetDisabled =
    isOverlayThemeDefault(overlayTheme) || isCommitting || locked;
  const lockedDescription = t(
    "settings.appearance.themeFile.lockedDescription",
  );

  const renderField = (field: OverlayTokenField) => {
    switch (field.kind) {
      // The three enum tokens with a row get their own selectors, not a generic
      // dropdown. Material owns the Glass gating and the unavailable note, the
      // Glass style its engine gating, the waveform style its six labels. All
      // three live in the descriptor table, so their groups run like Color and
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

      case "waveformStyle":
        return (
          <WaveformStyleSelector
            key={field.key}
            value={currentWaveformStyle}
            onSelect={handleSelectWaveformStyle}
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
            isResetting={isCommitting}
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
            isResetting={isCommitting}
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
          onResetConfirm={() => void commitOverlayTheme(INHERIT_ALL)}
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
              opacity, under Glass the tint strength in its place. Only the
              Waveform group takes the style, its two lengths being the only
              rows a style can hide. */}
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

          {showsWaveform && (
            <SettingsGroup title={t("settings.appearance.groups.waveform")}>
              {overlayTokenFieldsFor(
                "waveform",
                vars.effectiveMaterial,
                currentWaveformStyle,
              ).map(renderField)}
            </SettingsGroup>
          )}

          <SettingsGroup title={t("settings.appearance.groups.themeFile")}>
            <ThemeFileGroup
              file={resolved?.file ?? EMPTY_FILE_STATE}
              material={vars.effectiveMaterial}
              showWaveform={showsWaveform}
              watching={resolved?.watching ?? false}
              onReload={() => void reload()}
              isReloading={isReloading}
            />
          </SettingsGroup>
        </>
      )}
    </div>
  );
};
