import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/Input";
import { ResetButton } from "@/components/ui/ResetButton";
import { SettingContainer } from "@/components/ui/SettingContainer";

const HEX_PATTERN = /^#[0-9a-f]{6}$/i;

/** Shown instead of a hex while the resolved default is unmeasured (the first
 *  frame, or a computed color this parser cannot read). Not `#000000`, because
 *  a real-looking value there reads as "the default is black". */
const UNRESOLVED_PLACEHOLDER = "—";

/** What the OS picker opens on when there is no value. Mid-gray, not black,
 *  for the same reason. The control is transparent, so it is never painted. */
const UNRESOLVED_PICKER_VALUE = "#808080";

/** Accepts `#rrggbb`, `rrggbb` (missing `#`) and any case. Anything else
 *  fails, including the 3/4/8-digit forms `<input type="color">` cannot
 *  express, because the token contract's colors are `#RRGGBB` only, no alpha.
 *
 *  Exported for the unit tests; nothing outside this file imports it. */
export function normalizeHexInput(raw: string): string | null {
  const trimmed = raw.trim();
  const withHash = trimmed.startsWith("#") ? trimmed : `#${trimmed}`;
  return HEX_PATTERN.test(withHash) ? withHash.toLowerCase() : null;
}

export interface ColorFieldProps {
  label: string;
  description: string;
  /** The effective value (draft, or the persisted token). `null` means
   *  inherit, so the field shows `resolvedDefault`. */
  value: string | null;
  /** Theme-aware value an unset token resolves to. `null` until measured. */
  resolvedDefault: string | null;
  /** A new value was picked or typed; schedules/updates the draft. */
  onChange: (hex: string) => void;
  /** Flush the draft now. Focus leaving the swatch, or Enter or blur on the
   *  hex field, means the edit is settled and needs no more waiting. */
  onCommitNow: () => void;
  onReset: () => void;
  locked?: boolean;
  lockedDescription?: string;
  isResetting?: boolean;
  grouped?: boolean;
}

/**
 * One color token's row: a swatch, a hex field and a reset. The swatch is a
 * `<div>` painted with the current color under a transparent, absolutely
 * positioned `<input type="color">`, because the native control's chrome is
 * unthemeable and its OS picker needs a themed trigger.
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
  const displayHex = value ?? resolvedDefault;
  const displayText = displayHex ?? UNRESOLVED_PLACEHOLDER;
  const [text, setText] = useState(displayText);
  const focusedRef = useRef(false);

  // Follow external changes (a swatch pick, a reset, a theme-file lock landing)
  // while the field isn't being edited. A focused field keeps what the user is
  // mid-typing, so "#f" is rejected on commit, not on every keystroke.
  useEffect(() => {
    if (!focusedRef.current) setText(displayText);
  }, [displayText]);

  const commitText = () => {
    const parsed = normalizeHexInput(text);
    if (parsed) {
      onChange(parsed);
      onCommitNow();
    } else {
      // The revert is the feedback for an unparseable value. No toast.
      setText(displayText);
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
            style={{ backgroundColor: displayHex ?? "transparent" }}
          />
          <input
            type="color"
            aria-label={t("settings.appearance.color.swatchLabel")}
            // A draft, never a commit. macOS's color panel is continuous and
            // WebKit turns every update into a form-control change event
            // (measured: eight panel updates in 255 ms, eight events), which
            // React surfaces as `onChange`. Committing here issued one
            // `change_overlay_theme_setting` per drag frame. The commit belongs
            // to the 120 ms trailing debounce this draft feeds, which collapses
            // a drag into one write; `onBlur` flushes it early.
            value={
              displayHex && HEX_PATTERN.test(displayHex)
                ? displayHex
                : UNRESOLVED_PICKER_VALUE
            }
            onChange={(e) => onChange(e.currentTarget.value)}
            onBlur={onCommitNow}
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
              setText(displayText);
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
