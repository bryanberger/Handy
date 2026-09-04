import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { StreamPhase, StreamTextEvent, StreamWorkKind } from "@/bindings";
import { liveCardState } from "./cardShape";

/**
 * `inert` is a standard HTML boolean attribute, but this project's pinned
 * `@types/react` (18.3.26) does not type it on `HTMLAttributes` yet. Being a
 * boolean HTML attribute, its mere presence is what makes an element inert,
 * so a naive `inert={false}` risks React writing a falsy-looking value the
 * browser still treats as present. Spreading the attribute in only when
 * active sidesteps both problems.
 */
function inertAttribute(active: boolean): { inert?: string } {
  return active ? { inert: "" } : {};
}

/** The overlay's coarse recording/transcribing state. */
export type OverlayState =
  | "recording"
  | "streaming"
  | "transcribing"
  | "processing";

export interface OverlayCardProps {
  state: OverlayState;
  captureReady: boolean;
  levels: number[];
  streamText: StreamTextEvent;
  phase: StreamPhase;
  workKind: StreamWorkKind;
  elapsed: number;
  position: "top" | "bottom";
  /** Bumped to remount the card fresh (replays the pop-in). */
  session: number;
  direction: "ltr" | "rtl";
  /**
   * True for a decorative, non-interactive rendering, which is the Appearance
   * tab's preview. Applies the native `inert` attribute (no focus, no pointer
   * events, hidden from assistive tech) to the whole card.
   */
  inert?: boolean;
  /** Omitted (rather than a no-op) by the preview, which relies on `inert`. */
  onCancel?: () => void;
}

/**
 * The overlay's presentational half, the `.ov-stage` / `.scard` tree for every
 * state. Extracted from `RecordingOverlay.tsx` (which keeps every Tauri
 * listener, the elapsed timer and the position fetch) so the Appearance tab's
 * preview can render the exact markup a real dictation does. A preview that
 * could drift from the overlay would be worse than none. This component owns
 * the DOM concerns that belong to the card itself: the live-text scroll pin
 * and the top-edge overflow fade.
 *
 * The markup is verbatim except for the three class names that read
 * `isVisible`, which is state `RecordingOverlay` keeps and this component
 * never receives. All three were already constant at the point they ran,
 * because `RecordingOverlay` returns `null` before rendering the card when
 * `!isVisible`. `isVisible ? "" : "leaving"` was always `""` (the `.leaving`
 * rule was dead CSS and is deleted), and both `isVisible ? "show" : ""` and
 * `working && isVisible` were always their true branch. This file inlines all
 * three at those values.
 */
const OverlayCard: React.FC<OverlayCardProps> = ({
  state,
  captureReady,
  levels,
  streamText,
  phase,
  workKind,
  elapsed,
  position,
  session,
  direction,
  inert = false,
  onCancel,
}) => {
  const { t } = useTranslation();
  // True once live text overflows the cap. A top overlay fades its top edge only
  // while overflowing, so the resting first line stays crisp flush under the pill.
  const [overflowing, setOverflowing] = useState(false);
  // Live-text scroll-back: the text region "sticks" to the newest line while the
  // user is at the bottom; if they scroll up to read history, auto-follow pauses
  // until they scroll back down.
  const capRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);

  // Stick to the bottom as text streams in, but only while pinned, so a user
  // who has scrolled up to read history isn't yanked back down by the next
  // chunk.
  useLayoutEffect(() => {
    const el = capRef.current;
    if (!el) return;
    // Fade the top edge only once text actually overflows the cap.
    setOverflowing(el.scrollHeight > el.clientHeight + 1);
    if (pinnedRef.current) el.scrollTop = el.scrollHeight;
  }, [streamText]);

  // Each fresh streaming session starts pinned to the bottom, fade cleared.
  useEffect(() => {
    pinnedRef.current = true;
    setOverflowing(false);
  }, [session]);

  // Re-pin when the user is within ~a line of the bottom; unpin otherwise.
  const handleStreamScroll = () => {
    const el = capRef.current;
    if (!el) return;
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 16;
  };

  const fmtTime = (s: number) =>
    `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  // ---- Shared building blocks (one visual language, every overlay style) ----
  const waveform = (
    <div className={`swave ${captureReady ? "ready" : "arming"}`}>
      {levels.map((v, i) => (
        <i
          key={i}
          style={{
            // The bar heights are computed here, so they are the one length
            // the CSS cannot scale on its own. Multiply by --ov-scale inline.
            height: `calc(${Math.max(3, Math.min(18, 3 + Math.pow(v, 0.7) * 15))}px * var(--ov-scale))`,
          }}
        />
      ))}
    </div>
  );

  const cancelBtn = (
    <button
      className="sx"
      aria-label={t("overlay.cancel")}
      onClick={() => onCancel?.()}
    >
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M4 4 L12 12 M12 4 L4 12"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
        />
      </svg>
    </button>
  );

  // dot (left) | waveform (center) | timer + cancel (right), same structure
  // for pill & panel, so the Live morph is a pure width change.
  const listeningRow = (showTimer: boolean, showCancel: boolean) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className={`sdot ${captureReady ? "ready" : "arming"}`} />
      </div>
      {waveform}
      <div className="sbase-r">
        {showTimer && <span className="stimer">{fmtTime(elapsed)}</span>}
        {showCancel && cancelBtn}
      </div>
    </div>
  );

  // spinner (left) | label (center) | cancel (right), the same 3-zone grid as
  // the listening row, so the label is centered.
  const workingRow = (label: string, showCancel: boolean) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className="sspinner" />
      </div>
      <span className="swork-label">{label}</span>
      <div className="sbase-r">{showCancel && cancelBtn}</div>
    </div>
  );

  // ---- Live overlay: a pill that sculpts open into a panel ----
  if (state === "streaming") {
    const hasText =
      streamText.committed.length > 0 || streamText.tentative.length > 0;
    const working = phase === "working";
    // Shared with `cardShape`, which reports the same two flags to the backend
    // so the native window under Glass morphs with the card rather than
    // alongside it.
    const { open, collapsed } = liveCardState(hasText, working);

    return (
      <div
        dir={direction}
        className={`ov-stage ${position}`}
        {...inertAttribute(inert)}
      >
        <div
          key={session}
          className={`scard ${open ? "open" : ""} ${collapsed ? "working" : ""}`}
        >
          <div className="stext">
            <div className="stext-clip">
              <div
                className={`stext-cap ${overflowing ? "overflowing" : ""}`}
                ref={capRef}
                onScroll={handleStreamScroll}
              >
                <p>
                  <span className="committed">
                    {streamText.committed ? streamText.committed + " " : ""}
                  </span>
                  <span className="tentative">{streamText.tentative}</span>
                  {/* Drop the blinking caret once finalizing. It's no longer
                      capturing, and a static spinner conveys the work. */}
                  {!working && <span className="scaret" />}
                </p>
              </div>
            </div>
          </div>
          {working
            ? workingRow(
                workKind === "polishing"
                  ? t("overlay.processing")
                  : t("overlay.transcribing"),
                true,
              )
            : listeningRow(open, true)}
        </div>
      </div>
    );
  }

  // ---- Minimal overlay: exactly one row at a time. Waveform (recording), or
  // a spinner + label (transcribing / processing). Never both. The pill
  // animates its width between them; the cancel button is in both rows so it
  // stays put.
  const working = state === "transcribing" || state === "processing";
  const workLabel =
    state === "processing"
      ? t("overlay.processing")
      : t("overlay.transcribing");

  return (
    <div
      dir={direction}
      className={`ov-stage ${position} ov-fade show`}
      {...inertAttribute(inert)}
    >
      <div className={`scard compact ${working ? "cworking" : ""}`}>
        {working ? workingRow(workLabel, true) : listeningRow(false, true)}
      </div>
    </div>
  );
};

export default OverlayCard;
