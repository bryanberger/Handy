import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "@/components/ui/Dropdown";
import { SettingContainer } from "@/components/ui/SettingContainer";
import type { GlassSupport, Material } from "@/bindings";

export interface MaterialSelectorProps {
  value: Material;
  onSelect: (value: Material) => void;
  /** From the resolved theme payload, never a platform check in TypeScript —
   *  so the tab and the backend can never disagree about Glass. */
  glassSupport: GlassSupport;
  locked?: boolean;
  lockedDescription?: string;
  grouped?: boolean;
}

/**
 * The Material row: always shown, everywhere. Off macOS — or wherever Glass
 * failed to install — the option stays selectable-but-disabled with a
 * "(macOS only)" label rather than hidden, because the token still exists in
 * the theme file and a hidden row would leave no explanation.
 */
export const MaterialSelector: React.FC<MaterialSelectorProps> = ({
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

  // Glass stays selectable on a Mac that can't render it right now —
  // disabling it would strand the user's preference for when the machine can
  // again. The note explains why Flat renders instead, without naming a
  // cause: the payload carries `supported`/`available` and nothing else, so
  // asserting *why* (Reduce Transparency, say) would be a guess.
  const showUnavailableNote = glassSupport.supported && !glassSupport.available;

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
    </div>
  );
};
