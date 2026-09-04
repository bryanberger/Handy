import React from "react";
import { useTranslation } from "react-i18next";
import { Slider } from "@/components/ui/Slider";
import type { OverlayTheme } from "@/bindings";
import type { OverlayThemeKey } from "@/lib/overlayTheme";
import { ColorField } from "./ColorField";
import type { OverlayTokenField } from "./overlayTokenFields";

/** The row kinds this component renders: all but the two enum tokens, which
 *  have their own selectors. */
export type ValueTokenField = Extract<
  OverlayTokenField,
  { kind: "color" | "length" | "factor" }
>;

export interface OverlayTokenRowProps {
  field: ValueTokenField;
  /** A colour row's hex or `null` (inherit); a numeric row's shown number,
   *  already defaulted. */
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
 * Memoised, the whole reason it is a component and not a `renderField` closure
 * in the tab. A slider drag re-renders the tab per input event, which before
 * this split re-rendered all sixteen rows, roughly 1200 row renders in a
 * two-second drag, sixteen of them the row actually moving. Its props are the
 * value, three booleans/strings and three callbacks the tab holds stable, so
 * every row but the dragged one bails out.
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
      // React's synthetic onBlur bubbles, unlike native `blur`, so a tab-away
      // mid-drag blurs the range input and flushes, which onPointerUp misses.
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
