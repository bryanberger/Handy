import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  StreamPhase,
  StreamTextEvent,
  StreamWorkKind,
  WaveformStyle,
} from "@/bindings";
import { liveCardState } from "./cardShape";
import { WaveformCanvas } from "./waveform/WaveformCanvas";
import { isCanvasWaveformStyle } from "./waveform/waveformStyles";

/**
 * `inert` is a standard HTML boolean attribute, untyped on `HTMLAttributes` by the pinned
 * `@types/react` (18.3.26). Presence alone makes an element inert, so `inert={false}` risks
 * a falsy value the browser still treats as present. Spread it in only when active.
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
  /**
   * The `show_waveform` token. False empties the control row's centre column
   * and puts `nowave` on the two resting shapes, which then shrink to the row
   * that is left (`--ov-bare-w`). The working pill and the open panel keep
   * their tuned widths, so every morph stays a grow.
   */
  showWaveform?: boolean;
  /**
   * The `show_cancel` token. False drops the button from every row; the
   * keyboard shortcut and `--cancel` still cancel. The apply layer takes the
   * row's 22 px side floor away with it, that floor being the room the button
   * needed. With the waveform gone too it adds `nocancel`, which centres the
   * dot left alone on the row.
   */
  showCancel?: boolean;
  /**
   * The `waveform_style` token. `bars` keeps today's DOM capsules; the other
   * five draw on one canvas in the same lane, so the card's footprint is the
   * same whichever is chosen.
   */
  waveformStyle?: WaveformStyle;
  /**
   * The 16 smoothed microphone buckets. A canvas style reads them inside its
   * own loop, so a level frame costs no React render; the bars read `levels`
   * above, which is a slice of the same numbers as state.
   */
  levelsRef?: React.MutableRefObject<number[]>;
  /** Bumped whenever the apply layer repaints the card, so a canvas style
   *  re-reads the colours and lengths it caches. */
  themeRevision?: number;
  /**
   * Called once when the browser gives no 2D context. The overlay owns that
   * fact, not the card: it decides which style is drawn and, with it, whether
   * the bars' level state still has to flow. A card rendered without the
   * handler keeps the empty canvas, which no caller does.
   */
  onCanvasUnavailable?: () => void;
  /** Bumped to remount the card fresh (replays the pop-in). */
  session: number;
  direction: "ltr" | "rtl";
  /**
   * True for a decorative, non-interactive rendering, the Appearance tab's preview. Puts
   * native `inert` on the whole card (no focus, no pointer events, hidden from assistive tech).
   */
  inert?: boolean;
  /** Omitted (rather than a no-op) by the preview, which relies on `inert`. */
  onCancel?: () => void;
}

/**
 * The overlay's presentational half, the `.ov-stage` / `.scard` tree for every state.
 * Split from `RecordingOverlay.tsx`, which keeps every Tauri listener, the elapsed
 * timer and the position fetch, so the Appearance tab's preview renders the exact
 * markup a real dictation does and cannot drift from it. It owns the card's own DOM
 * concerns, the live-text scroll pin and the top-edge overflow fade.
 *
 * The markup is verbatim except the three class names reading `isVisible`, state
 * `RecordingOverlay` keeps and never passes here. All three were constant, since
 * `RecordingOverlay` returns `null` when `!isVisible`. `isVisible ? "" : "leaving"` was
 * always `""` (the `.leaving` rule was dead CSS, deleted), `isVisible ? "show" : ""` and
 * `working && isVisible` took their true branch. All three are inlined at those values.
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
  showWaveform = true,
  showCancel = true,
  waveformStyle = "bars",
  levelsRef,
  themeRevision = 0,
  onCanvasUnavailable,
  session,
  direction,
  inert = false,
  onCancel,
}) => {
  const { t } = useTranslation();
  // True once live text overflows the cap. A top overlay fades its top edge only
  // while overflowing, so the resting first line stays crisp under the pill.
  const [overflowing, setOverflowing] = useState(false);
  // Live-text scroll-back. The text sticks to the newest line while the user is at
  // the bottom; scrolling up to read history pauses auto-follow until they return.
  const capRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);

  // Stick to the bottom as text streams in, but only while pinned, so a user who
  // scrolled up to read history isn't yanked back down by the next chunk.
  useLayoutEffect(() => {
    const el = capRef.current;
    if (!el) return;
    // Fade the top edge only once text overflows the cap.
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
  // The five canvas styles need the levels ref; without it there is nothing to
  // draw from, so the bars stand in, which is also the inherit. A failed 2D
  // context is reported upward and comes back as `bars` on the next render.
  const canvasStyle = isCanvasWaveformStyle(waveformStyle)
    ? waveformStyle
    : null;
  const waveform = (
    <div className={`swave ${captureReady ? "ready" : "arming"}`}>
      {canvasStyle && levelsRef ? (
        <WaveformCanvas
          style={canvasStyle}
          levelsRef={levelsRef}
          ready={captureReady}
          themeRevision={themeRevision}
          onUnavailable={onCanvasUnavailable}
        />
      ) : (
        levels.map((v, i) => (
          <i
            key={i}
            style={{
              // Bar heights are computed here, the one length the CSS cannot
              // scale on its own, so multiply by --ov-scale inline.
              height: `calc(${Math.max(3, Math.min(18, 3 + Math.pow(v, 0.7) * 15))}px * var(--ov-scale))`,
            }}
          />
        ))
      )}
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

  // The classes a resting shape takes from the two visibility tokens. `nowave`
  // shrinks the pill to the row that is left; `nocancel` on top of it centres
  // the dot that is then alone on that row. Neither ever reaches the working
  // pill or the open panel, both tuned to their content, so every morph out of
  // a shrunken shape stays a grow.
  const restingClasses = showWaveform
    ? ""
    : showCancel
      ? "nowave"
      : "nowave nocancel";

  // dot (left) | waveform (center) | timer + cancel (right), same structure
  // for pill & panel, so the Live morph is a pure width change.
  // The grid keeps its three columns whatever is hidden, so the waveform and
  // the working label stay centred and the dot stays at the left of its track.
  // Once the dot is alone on the row the stylesheet centres it instead.
  const listeningRow = (showTimer: boolean) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className={`sdot ${captureReady ? "ready" : "arming"}`} />
      </div>
      {showWaveform ? waveform : <span />}
      <div className="sbase-r">
        {showTimer && <span className="stimer">{fmtTime(elapsed)}</span>}
        {showCancel && cancelBtn}
      </div>
    </div>
  );

  // spinner (left) | label (center) | cancel (right), the same 3-zone grid as
  // the listening row, so the label is centered.
  const workingRow = (label: string) => (
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
    // Shared with `cardShape`, which reports the same two flags to the backend, so
    // the native window under Glass morphs with the card, not alongside it.
    const { open, collapsed } = liveCardState(hasText, working);

    return (
      <div
        dir={direction}
        className={`ov-stage ${position}`}
        {...inertAttribute(inert)}
      >
        <div
          key={session}
          className={`scard ${open ? "open" : ""} ${collapsed ? "working" : ""} ${
            // The Live pill, the one Live shape that rests. The resting classes
            // never meet `open` or `working`, so the width rules cannot collide.
            !open && !collapsed ? restingClasses : ""
          }`}
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
                  {/* Drop the blinking caret once finalizing. Nothing is being
                      captured, and a static spinner conveys the work. */}
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
              )
            : listeningRow(open)}
        </div>
      </div>
    );
  }

  // ---- Minimal overlay: exactly one row at a time. Waveform (recording), or a
  // spinner + label (transcribing / processing). Never both. The pill animates
  // its width between them; the cancel button is in both rows so it stays put.
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
      <div
        className={`scard compact ${working ? "cworking" : ""} ${
          working ? "" : restingClasses
        }`}
      >
        {working ? workingRow(workLabel) : listeningRow(false)}
      </div>
    </div>
  );
};

export default OverlayCard;
