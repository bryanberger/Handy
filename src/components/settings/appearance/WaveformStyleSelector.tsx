import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "@/components/ui/Dropdown";
import { SettingContainer } from "@/components/ui/SettingContainer";
import type { WaveformStyle } from "@/bindings";
import { WAVEFORM_STYLES } from "@/lib/overlayTheme";

export interface WaveformStyleSelectorProps {
  value: WaveformStyle;
  onSelect: (value: WaveformStyle) => void;
  locked?: boolean;
  lockedDescription?: string;
  grouped?: boolean;
}

/**
 * How the waveform is drawn, as a dropdown of the six styles.
 *
 * Labels only, no subtitles: a one-line description of a look explains less
 * than the overlay does; the preview is on screen as this row is edited and
 * repaints the moment a style is picked. A live thumbnail per option was
 * weighed and dropped for the same reason: six more canvases and six more
 * animation loops in the settings window to say what the card already says.
 *
 * A dropdown; the Glass style's segmented control suits two, not six.
 */
const WaveformStyleSelectorInner: React.FC<WaveformStyleSelectorProps> = ({
  value,
  onSelect,
  locked = false,
  lockedDescription,
  grouped = true,
}) => {
  const { t } = useTranslation();

  const options = WAVEFORM_STYLES.map((style) => ({
    value: style,
    label: t(`settings.appearance.waveformStyle.options.${style}`),
  }));

  return (
    <SettingContainer
      title={t("settings.appearance.waveformStyle.title")}
      description={
        locked && lockedDescription
          ? lockedDescription
          : t("settings.appearance.waveformStyle.description")
      }
      grouped={grouped}
      disabled={locked}
    >
      <Dropdown
        options={options}
        selectedValue={value}
        onSelect={(next) => onSelect(next as WaveformStyle)}
        disabled={locked}
      />
    </SettingContainer>
  );
};

/** Memoised, because the Appearance tab re-renders every frame of a slider
 *  drag, which cannot have changed this row. */
export const WaveformStyleSelector = React.memo(WaveformStyleSelectorInner);
