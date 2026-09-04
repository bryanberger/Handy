import React from "react";
import { useTranslation } from "react-i18next";
import { SettingContainer } from "@/components/ui/SettingContainer";
import type { GlassStyle, GlassSupport, Material } from "@/bindings";

/**
 * The two Liquid Glass styles, in the order `GlassStyle::ALL` declares them,
 * each mapped to the i18n key its label lives under.
 *
 * `satisfies Record<GlassStyle, string>` keeps the hand-written list honest.
 * A style added in Rust and regenerated into `src/bindings.ts` fails `tsc`
 * here until it has a row, and a stale one fails the moment it leaves the
 * union. A segmented control is only the right shape while there are two of
 * them, so that failure is the signal to reconsider the control, not just to
 * add a label.
 */
const GLASS_STYLE_LABELS = {
  regular: "regular",
  clear: "clear",
} satisfies Record<GlassStyle, string>;

/** The control's order, which is the declaration order above. */
export const GLASS_STYLE_OPTIONS: readonly GlassStyle[] = Object.keys(
  GLASS_STYLE_LABELS,
) as GlassStyle[];

/**
 * The radiogroup keyboard rule as a table: which option a key selects, given
 * the option the focus is on, or `null` for a key this control leaves to the
 * browser.
 *
 * A segmented control is a radiogroup, so it owes the pattern its keyboard.
 * Both arrow axes move and select (a radiogroup's selection follows focus),
 * wrapping at the ends, and Space and Enter select whatever is focused. Only
 * the selected option is tabbable, via the roving tabindex in the component
 * below, so Tab enters and leaves the group in one step, the way the
 * dropdowns and sliders around it do.
 *
 * Pure, and keyed on the focused option rather than on the selected one, so
 * it stays right in the one case where the two differ: a click whose write
 * the settings store rolled back.
 */
export function glassStyleForKey(
  key: string,
  focused: GlassStyle,
): GlassStyle | null {
  const index = GLASS_STYLE_OPTIONS.indexOf(focused);
  if (index < 0) return null;
  const count = GLASS_STYLE_OPTIONS.length;
  switch (key) {
    case "ArrowRight":
    case "ArrowDown":
      return GLASS_STYLE_OPTIONS[(index + 1) % count];
    case "ArrowLeft":
    case "ArrowUp":
      return GLASS_STYLE_OPTIONS[(index - 1 + count) % count];
    case " ":
    case "Enter":
      return focused;
    default:
      return null;
  }
}

/** What the Glass style row does on this machine, for this theme. */
export type GlassStyleControlState = "hidden" | "disabled" | "enabled";

/**
 * Whether to show the Glass style row, and whether it does anything.
 *
 * - hidden wherever Liquid Glass is not the engine. On Windows, on Linux,
 *   and on macOS before 26 the token can never change a pixel, and the row
 *   would be an offer the machine cannot keep. `glass_material` covers the
 *   fallback engine, from the theme file.
 * - disabled while the theme file owns the token (like every other locked
 *   control) or while the Material is Flat. That is the same rule the
 *   Material row's own dependents follow, and it keeps the row visible so the
 *   user can see what picking Glass will give them.
 * - enabled otherwise. Deliberately still enabled on a Mac where Glass is
 *   merely unavailable right now (Reduce Transparency), because the
 *   preference has to survive the accessibility setting being turned back
 *   off. That is why the Material row keeps Glass selectable there too.
 *
 * Pure, so the rule is a table rather than a chain of `&&` inside JSX.
 */
export function glassStyleControlState(
  material: Material,
  glassSupport: GlassSupport,
  locked: boolean,
): GlassStyleControlState {
  if (glassSupport.engine !== "liquid") return "hidden";
  if (locked || material !== "glass") return "disabled";
  return "enabled";
}

export interface GlassStyleSelectorProps {
  value: GlassStyle;
  onSelect: (value: GlassStyle) => void;
  /** The Material token as it currently reads. The Glass style only affects
   *  anything while this is `glass`. */
  material: Material;
  /** From the resolved theme payload, never a platform or version check in
   *  TypeScript, so the tab and the backend cannot disagree about which
   *  engine is drawing. */
  glassSupport: GlassSupport;
  locked?: boolean;
  lockedDescription?: string;
  grouped?: boolean;
}

/**
 * Which Liquid Glass style the Glass surface is drawn with, as a two-option
 * segmented control directly under the Material row.
 *
 * A segmented control rather than a dropdown because both options fit on one
 * line and the choice is a look rather than a list. With two values a
 * dropdown hides half the answer behind a click. No option carries a
 * subtitle, because the row's own description says what the pair means.
 */
const GlassStyleSelectorInner: React.FC<GlassStyleSelectorProps> = ({
  value,
  onSelect,
  material,
  glassSupport,
  locked = false,
  lockedDescription,
  grouped = true,
}) => {
  const { t } = useTranslation();
  const buttons = React.useRef<
    Partial<Record<GlassStyle, HTMLButtonElement | null>>
  >({});

  const state = glassStyleControlState(material, glassSupport, locked);
  if (state === "hidden") return null;
  const disabled = state === "disabled";

  // Handled on the option rather than on the group, so the rule is told which
  // option has focus. `preventDefault` covers Space and Enter too, since a
  // `<button>` fires its own click on both, which would select the same
  // option a second time.
  const handleKeyDown = (
    event: React.KeyboardEvent<HTMLButtonElement>,
    focused: GlassStyle,
  ) => {
    const next = glassStyleForKey(event.key, focused);
    if (next === null) return;
    event.preventDefault();
    if (next !== value) onSelect(next);
    buttons.current[next]?.focus();
  };

  return (
    <SettingContainer
      title={t("settings.appearance.glassStyle.title")}
      description={
        locked && lockedDescription
          ? lockedDescription
          : t("settings.appearance.glassStyle.description")
      }
      grouped={grouped}
      disabled={disabled}
    >
      <div
        role="radiogroup"
        aria-label={t("settings.appearance.glassStyle.title")}
        className="inline-flex rounded-md border border-mid-gray/20 p-0.5"
      >
        {GLASS_STYLE_OPTIONS.map((option) => {
          const selected = option === value;
          return (
            <button
              key={option}
              ref={(element) => {
                buttons.current[option] = element;
              }}
              type="button"
              role="radio"
              aria-checked={selected}
              // Roving tabindex, so the group is one tab stop and the
              // arrows move within it.
              tabIndex={selected ? 0 : -1}
              disabled={disabled}
              onClick={() => onSelect(option)}
              onKeyDown={(event) => handleKeyDown(event, option)}
              className={`rounded px-3 py-1 text-sm transition-colors ${
                selected
                  ? "bg-background-ui text-white"
                  : "text-text hover:bg-mid-gray/10"
              } ${disabled ? "cursor-not-allowed opacity-50" : ""}`}
            >
              {t(
                `settings.appearance.glassStyle.options.${GLASS_STYLE_LABELS[option]}`,
              )}
            </button>
          );
        })}
      </div>
    </SettingContainer>
  );
};

/** Memoised, because the Appearance tab re-renders on every frame of a
 *  slider drag and a drag cannot have changed this row. */
export const GlassStyleSelector = React.memo(GlassStyleSelectorInner);
