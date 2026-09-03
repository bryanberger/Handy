import React, { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/Input";
import { ResetButton } from "@/components/ui/ResetButton";
import { SettingContainer } from "@/components/ui/SettingContainer";

const HEX_PATTERN = /^#[0-9a-f]{6}$/i;

/** Shown in place of a hex while the resolved default has not been measured
 *  (the first frame, or a computed color this parser does not understand).
 *  Deliberately not `#000000`: a real-looking value there reads as "the
 *  default is black", which is a lie. */
const UNRESOLVED_PLACEHOLDER = "—";

/** What the OS picker opens on when there is no value to open on. Neutral
 *  mid-gray rather than black, for the same reason. The control itself is
 *  transparent, so this is never painted. */
const UNRESOLVED_PICKER_VALUE = "#808080";

/** Accepts `#rrggbb`, `rrggbb` (missing `#`), and any case; anything else —
 *  including the 3/4/8-digit forms `<input type="color">` cannot express —
 *  fails to parse (ticket 02 §1: colors are `#RRGGBB` only, no alpha). */
export function normalizeHexInput(raw: string): string | null {
  const trimmed = raw.trim();
  const withHash = trimmed.startsWith("#") ? trimmed : `#${trimmed}`;
  return HEX_PATTERN.test(withHash) ? withHash.toLowerCase() : null;
}

/** The little of a native color input this module needs, so the commit rule
 *  below can be exercised without a DOM. */
export interface ColorCommitTarget {
  value: string;
  addEventListener(type: string, listener: () => void): void;
  removeEventListener(type: string, listener: () => void): void;
}

/**
 * Commit on the **native `change` event only**, and return the unsubscribe.
 *
 * React maps `onChange` for `<input type="color">` onto the native `input`
 * event, which the OS picker fires continuously while the user drags inside
 * it. Committing there would issue one `change_overlay_theme_setting` command
 * per frame of the drag. The native `change` event fires once, when the picker
 * closes — that is the commit point, and it can only be reached by attaching
 * the listener directly to the element.
 */
export function subscribeColorCommit(
  target: ColorCommitTarget,
  onCommit: (value: string) => void,
): () => void {
  const listener = () => onCommit(target.value);
  target.addEventListener("change", listener);
  return () => target.removeEventListener("change", listener);
}

export interface ColorFieldProps {
  label: string;
  description: string;
  /** The effective value (draft, or the persisted token) — `null` means
   *  inherit, and `resolvedDefault` is shown instead. */
  value: string | null;
  /** The theme-aware value an unset token currently resolves to. `null` while
   *  it has not been measured yet. */
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
  const displayHex = value ?? resolvedDefault;
  const displayText = displayHex ?? UNRESOLVED_PLACEHOLDER;
  const [text, setText] = useState(displayText);
  const focusedRef = useRef(false);
  const pickerRef = useRef<HTMLInputElement>(null);

  // Follow external changes (a swatch pick, a reset, a theme-file lock
  // landing) while the field isn't being edited. A focused field keeps
  // whatever the user is mid-typing — "#f" is not rejected on every
  // keystroke, only when it fails to parse on commit.
  useEffect(() => {
    if (!focusedRef.current) setText(displayText);
  }, [displayText]);

  // The picker's live drag updates the draft (React's `onChange`, i.e. the
  // native `input` event); this is the one place that commits it. The
  // handlers go through a ref so the listener is attached once, on mount,
  // instead of being torn down and re-added on every render.
  const commitRef = useRef({ onChange, onCommitNow });
  useEffect(() => {
    commitRef.current = { onChange, onCommitNow };
  });
  useEffect(() => {
    const picker = pickerRef.current;
    if (!picker) return;
    return subscribeColorCommit(picker, (picked) => {
      commitRef.current.onChange(picked);
      commitRef.current.onCommitNow();
    });
  }, []);

  const commitText = () => {
    const parsed = normalizeHexInput(text);
    if (parsed) {
      onChange(parsed);
      onCommitNow();
    } else {
      // The revert is the feedback for an unparseable value — no toast.
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
            ref={pickerRef}
            type="color"
            aria-label={t("settings.appearance.color.swatchLabel")}
            // React routes `onChange` for a color input through the native
            // `input` event, so this is the per-frame *draft*, not a commit;
            // the commit is the native `change` listener attached above.
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
