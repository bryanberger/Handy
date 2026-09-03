import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "@/components/ui/Dropdown";
import { SettingContainer } from "@/components/ui/SettingContainer";
import type { GlassMaterial, GlassSupport, Material } from "@/bindings";

/**
 * The eight macOS materials, in the order `GlassMaterial::ALL` declares them,
 * each mapped to the i18n key its strings live under.
 *
 * Spelled out rather than derived from the bindings, because a TypeScript
 * union type has no runtime value to iterate — and because the order here is
 * a UI decision, not a serialization one. `satisfies Record<GlassMaterial, …>`
 * is what keeps the hand-written list honest: a material added in Rust and
 * regenerated into `src/bindings.ts` fails `tsc` here until it has a row, and
 * a stale one fails the moment it leaves the union.
 */
const GLASS_MATERIAL_LABELS = {
  hud_window: "hudWindow",
  popover: "popover",
  menu: "menu",
  sidebar: "sidebar",
  under_window_background: "underWindowBackground",
  sheet: "sheet",
  tooltip: "tooltip",
  content_background: "contentBackground",
} satisfies Record<GlassMaterial, string>;

/** The dropdown's order, which is the declaration order above. */
export const GLASS_MATERIAL_OPTIONS: readonly GlassMaterial[] = Object.keys(
  GLASS_MATERIAL_LABELS,
) as GlassMaterial[];

/** Where one option's `title` and `description` live. */
function labelKey(material: GlassMaterial): string {
  const name = GLASS_MATERIAL_LABELS[material];
  return `settings.appearance.glassMaterial.options.${name}`;
}

export interface GlassMaterialSelectorProps {
  value: GlassMaterial;
  onSelect: (value: GlassMaterial) => void;
  /** The Material token as it currently reads — the Glass material only
   *  affects anything while this is `glass`. */
  material: Material;
  /** From the resolved theme payload, never a platform check in TypeScript. */
  glassSupport: GlassSupport;
  locked?: boolean;
  lockedDescription?: string;
  grouped?: boolean;
}

/**
 * Which macOS material the Glass blur is drawn with.
 *
 * Shown directly under the Material row and always present, so the token is
 * discoverable and its theme-file key has a visible home; disabled while
 * Material is Flat or Glass is not offerable at all, because nothing it can
 * be set to would change a pixel. It stays *enabled* on a Mac where Glass is
 * merely unavailable right now (Reduce Transparency), for the same reason the
 * Material row keeps Glass selectable there: the preference has to survive
 * the accessibility setting being turned back off.
 */
export const GlassMaterialSelector: React.FC<GlassMaterialSelectorProps> = ({
  value,
  onSelect,
  material,
  glassSupport,
  locked = false,
  lockedDescription,
  grouped = true,
}) => {
  const { t } = useTranslation();

  const disabled = locked || !glassSupport.supported || material !== "glass";

  const options = GLASS_MATERIAL_OPTIONS.map((option) => ({
    value: option,
    label: t(`${labelKey(option)}.title`),
    description: t(`${labelKey(option)}.description`),
  }));

  return (
    <SettingContainer
      title={t("settings.appearance.glassMaterial.title")}
      description={
        locked && lockedDescription
          ? lockedDescription
          : t("settings.appearance.glassMaterial.description")
      }
      grouped={grouped}
      disabled={disabled}
    >
      <Dropdown
        options={options}
        selectedValue={value}
        onSelect={(next) => onSelect(next as GlassMaterial)}
        disabled={disabled}
      />
    </SettingContainer>
  );
};
