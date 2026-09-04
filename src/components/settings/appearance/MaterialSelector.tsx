import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "@/components/ui/Dropdown";
import { SettingContainer } from "@/components/ui/SettingContainer";
import type { GlassSupport, Material } from "@/bindings";

export interface MaterialSelectorProps {
  value: Material;
  onSelect: (value: Material) => void;
  /** From the resolved theme payload, never a TypeScript platform check, so
   *  the tab and the backend cannot disagree about Glass. */
  glassSupport: GlassSupport;
  locked?: boolean;
  lockedDescription?: string;
  grouped?: boolean;
}

/**
 * The Material row is always shown. Where Glass is unsupported (off macOS, or a
 * failed install) it is disabled and labelled "(macOS only)", not hidden,
 * because the token exists in the theme file and hiding explains nothing.
 */
const MaterialSelectorInner: React.FC<MaterialSelectorProps> = ({
  value,
  onSelect,
  glassSupport,
  locked = false,
  lockedDescription,
  grouped = true,
}) => {
  const { t } = useTranslation();

  const options = [
    { value: "flat", label: t("settings.appearance.material.options.flat") },
    {
      value: "glass",
      label: glassSupport.supported
        ? t("settings.appearance.material.options.glass")
        : t("settings.appearance.material.options.glassMacOnly"),
      disabled: !glassSupport.supported,
    },
  ];

  // Glass stays selectable when a Mac can't render it now. Disabling would
  // strand the preference until the machine can again. The note says why Flat
  // renders instead without naming a cause, since the payload carries only
  // `supported` and `available`. A cause (Reduce Transparency, say) is a guess.
  const showUnavailableNote = glassSupport.supported && !glassSupport.available;
  // While Glass is showing, the line under the row is better spent pointing
  // at the control that decides how glassy it is. The two are mutually
  // exclusive by construction, so the row never grows two notes.
  const showGlassHint = value === "glass" && glassSupport.available;

  return (
    <div>
      <SettingContainer
        title={t("settings.appearance.material.title")}
        description={
          locked && lockedDescription
            ? lockedDescription
            : t("settings.appearance.material.description")
        }
        grouped={grouped}
        disabled={locked}
      >
        <Dropdown
          options={options}
          selectedValue={value}
          onSelect={(next) => onSelect(next as Material)}
          disabled={locked}
        />
      </SettingContainer>
      {showUnavailableNote && (
        <p className="-mt-1 px-4 pb-2 text-xs text-mid-gray">
          {t("settings.appearance.material.unavailableNote")}
        </p>
      )}
      {showGlassHint && (
        <p className="-mt-1 px-4 pb-2 text-xs text-mid-gray">
          {t("settings.appearance.material.glassHint")}
        </p>
      )}
    </div>
  );
};

/** Memoised, because the Appearance tab re-renders every frame of a slider
 *  drag, which cannot have changed this row. */
export const MaterialSelector = React.memo(MaterialSelectorInner);
