import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/Input";
import { ResetButton } from "@/components/ui/ResetButton";
import { SettingContainer } from "@/components/ui/SettingContainer";

const HEX_PATTERN = /^#[0-9a-f]{6}$/i;

/** Accepts `#rrggbb`, `rrggbb` (missing `#`), and any case; anything else —
 *  including the 3/4/8-digit forms `<input type="color">` cannot express —
 *  fails to parse (ticket 02 §1: colors are `#RRGGBB` only, no alpha). */
export function normalizeHexInput(raw: string): string | null {
  const trimmed = raw.trim();
  const withHash = trimmed.startsWith("#") ? trimmed : `#${trimmed}`;
  return HEX_PATTERN.test(withHash) ? withHash.toLowerCase() : null;
}

export interface ColorFieldProps {
  label: string;
  description: string;
  /** The effective value (draft, or the persisted token) — `null` means
   *  inherit, and `resolvedDefault` is shown instead. */
  value: string | null;
  /** The theme-aware value an unset token currently resolves to. */
  resolvedDefault: string | null;
  /** A new value was picked or typed; schedules/updates the draft. */
  onChange: (hex: string) => void;
  /** Flush the draft immediately — the OS picker closing, or Enter/blur on
   *  the hex field, both need no further debounce. */
  onCommitNow: () => void;
  onReset: () => void;
  locked?: boolean;
  lockedDescription?: string;
  isResetting?: boolean;
  grouped?: boolean;
}

/**
 * Swatch + hex field + reset, for one color token. The swatch is a plain
 * `<div>` painted with the current color, with a transparent, absolutely
 * positioned `<input type="color">` on top — the native control's own chrome
 * is unthemeable, so this is how its OS picker gets a themed trigger.
 */
export const ColorField: React.FC<ColorFieldProps> = ({
  label,
  description,
  value,
  resolvedDefault,
  onChange,
  onCommitNow,
  onReset,
  locked = false,
  lockedDescription,
  isResetting = false,
  grouped = true,
}) => {
  const { t } = useTranslation();
  const isUnset = value === null;
  const displayHex = value ?? resolvedDefault ?? "#000000";
  const [text, setText] = useState(displayHex);
  const focusedRef = useRef(false);

  // Follow external changes (a swatch pick, a reset, a theme-file lock
  // landing) while the field isn't being edited. A focused field keeps
  // whatever the user is mid-typing — "#f" is not rejected on every
  // keystroke, only when it fails to parse on commit.
  useEffect(() => {
    if (!focusedRef.current) setText(displayHex);
  }, [displayHex]);

  const commitText = () => {
    const parsed = normalizeHexInput(text);
    if (parsed) {
      onChange(parsed);
      onCommitNow();
    } else {
      // The revert is the feedback for an unparseable value — no toast.
      setText(displayHex);
    }
  };

  return (
    <SettingContainer
      title={label}
      description={
        locked && lockedDescription ? lockedDescription : description
      }
      grouped={grouped}
      disabled={locked}
    >
      <div className="flex items-center gap-2">
        <div className="relative w-6 h-6 shrink-0 overflow-hidden rounded-md border border-mid-gray/80">
          <div
            aria-hidden="true"
            className="absolute inset-0"
            style={{ backgroundColor: displayHex }}
          />
          <input
            type="color"
            aria-label={t("settings.appearance.color.swatchLabel")}
            value={HEX_PATTERN.test(displayHex) ? displayHex : "#000000"}
            onInput={(e) => onChange(e.currentTarget.value)}
            onChange={(e) => {
              onChange(e.currentTarget.value);
              onCommitNow();
            }}
            disabled={locked}
            className="absolute inset-0 h-full w-full cursor-pointer opacity-0 disabled:cursor-not-allowed"
          />
        </div>
        <Input
          variant="compact"
          value={text}
          aria-label={t("settings.appearance.color.hexLabel")}
          onFocus={() => {
            focusedRef.current = true;
          }}
          onChange={(e) => setText(e.target.value)}
          onBlur={() => {
            focusedRef.current = false;
            commitText();
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.currentTarget.blur();
            } else if (e.key === "Escape") {
              setText(displayHex);
              e.currentTarget.blur();
            }
          }}
          disabled={locked}
          className={`w-[104px] font-mono ${isUnset ? "italic text-mid-gray" : ""}`}
        />
        <ResetButton
          onClick={onReset}
          disabled={locked || isUnset || isResetting}
          ariaLabel={t("settings.appearance.color.resetLabel")}
        />
      </div>
    </SettingContainer>
  );
};
