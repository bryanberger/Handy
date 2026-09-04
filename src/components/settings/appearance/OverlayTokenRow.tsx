import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "@/components/ui/Slider";
import type { OverlayTheme } from "@/bindings";
import type { OverlayThemeKey } from "@/lib/overlayTheme";
import { ColorField } from "./ColorField";
import type { OverlayTokenField } from "./overlayTokenFields";

/** The row kinds this component renders: everything but the two enum tokens,
 *  which have their own selectors. */
export type ValueTokenField = Extract<
  OverlayTokenField,
  { kind: "color" | "length" | "factor" }
>;

export interface OverlayTokenRowProps {
  field: ValueTokenField;
  /** For a colour row, the hex or `null` (inherit); for a numeric row, the
   *  number the control shows, already defaulted. */
  value: string | number | null;
  /** Colour rows only: the theme-aware value an unset token resolves to. */
  resolvedDefault?: string | null;
  locked: boolean;
  lockedDescription: string;
  isResetting: boolean;
  onDraft: <K extends OverlayThemeKey>(key: K, value: OverlayTheme[K]) => void;
  onFlush: (key: OverlayThemeKey) => void;
  onReset: (key: OverlayThemeKey) => void;
}

/**
 * One overlay-theme token's row: a colour field, or a slider.
 *
 * Memoised, and that is the entire reason it exists as a component rather
 * than a `renderField` closure in the tab. A slider drag re-renders the tab on
 * every input event, and until this split that re-rendered all sixteen rows
 * with it — roughly 1200 row renders across a two-second drag, of which
 * sixteen were the row actually moving. The props here are the value, three
 * booleans/strings and three callbacks the tab holds stable, so every row but
 * the dragged one bails out.
 */
const OverlayTokenRowInner: React.FC<OverlayTokenRowProps> = ({
  field,
  value,
  resolvedDefault = null,
  locked,
  lockedDescription,
  isResetting,
  onDraft,
  onFlush,
  onReset,
}) => {
  const { t } = useTranslation();

  if (field.kind === "color") {
    return (
      <ColorField
        label={t(field.labelKey)}
        description={t(field.descriptionKey)}
        value={value as string | null}
        resolvedDefault={resolvedDefault}
        onChange={(hex) => onDraft(field.key, hex)}
        onCommitNow={() => onFlush(field.key)}
        onReset={() => onReset(field.key)}
        locked={locked}
        lockedDescription={lockedDescription}
        isResetting={isResetting}
      />
    );
  }

  const isLength = field.kind === "length";
  return (
    <div
      onPointerUp={() => onFlush(field.key)}
      // React's synthetic onBlur bubbles (unlike the native `blur` event), so
      // this fires when the range input inside loses focus — e.g. tabbing away
      // mid-drag, which onPointerUp alone would miss.
      onBlur={() => onFlush(field.key)}
    >
      <Slider
        grouped
        descriptionMode="tooltip"
        label={t(field.labelKey)}
        description={locked ? lockedDescription : t(field.descriptionKey)}
        value={value as number}
        onChange={(next) => onDraft(field.key, next)}
        min={field.min}
        max={field.max}
        step={field.step}
        disabled={locked}
        formatValue={(v) =>
          isLength ? `${Math.round(v)}px` : `${v.toFixed(2)}×`
        }
        onReset={() => onReset(field.key)}
        isResetting={isResetting}
      />
    </div>
  );
};

export const OverlayTokenRow = React.memo(OverlayTokenRowInner);
