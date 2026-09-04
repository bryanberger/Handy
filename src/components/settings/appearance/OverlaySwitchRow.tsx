import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "@/components/ui/ToggleSwitch";

export interface OverlaySwitchRowProps {
  labelKey: string;
  descriptionKey: string;
  /** An extra line under the row, for a rule the switch cannot state itself. */
  noteKey?: string;
  checked: boolean;
  /** The tab holds one of these per switch, stable for its life, so a row is
   *  only re-rendered by its own value (see [`OverlayTokenRow`]). */
  onChange: (checked: boolean) => void;
  locked: boolean;
  lockedDescription: string;
}

/**
 * One overlay-theme token rendered as a switch: the two visibility tokens, and
 * `shadow_strength` under Glass, where macOS's window shadow is on or off and
 * `NSWindow` exposes no strength to slide.
 *
 * A switch commits straight through `setOverlayThemeToken`, as the Material and
 * the Glass style do. There is nothing to settle: a click is one value, not the
 * tail of a drag, so the draft path the sliders need would only add a delay.
 *
 * Memoised for the same reason [`OverlayTokenRow`] is. The tab re-renders on
 * every frame of a slider drag, which cannot have moved a switch.
 */
const OverlaySwitchRowInner: React.FC<OverlaySwitchRowProps> = ({
  labelKey,
  descriptionKey,
  noteKey,
  checked,
  onChange,
  locked,
  lockedDescription,
}) => {
  const { t } = useTranslation();

  return (
    <div>
      <ToggleSwitch
        grouped
        descriptionMode="tooltip"
        label={t(labelKey)}
        description={locked ? lockedDescription : t(descriptionKey)}
        checked={checked}
        onChange={onChange}
        disabled={locked}
      />
      {noteKey && (
        <p className="-mt-1 px-4 pb-2 text-xs text-mid-gray">{t(noteKey)}</p>
      )}
    </div>
  );
};

export const OverlaySwitchRow = React.memo(OverlaySwitchRowInner);
