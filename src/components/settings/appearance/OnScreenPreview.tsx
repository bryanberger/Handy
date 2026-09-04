import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { commands } from "@/bindings";
import type { OverlayStyle, OverlayTheme, PreviewState } from "@/bindings";
import { OverlayThemeReset } from "./OverlayThemeReset";
import { themeAsJsonDocument } from "./ThemeFileGroup";
import {
  answerPreviewRequest,
  IDLE_PREVIEW,
  overlayAcceptsDrafts,
  previewBlocker,
  previewChipsFor,
  reducePreview,
  type PreviewAction,
  type PreviewCall,
  type PreviewChangeRequest,
  type PreviewMode,
} from "./previewMode";

/** How often the tab re-asks whether Handy is recording. The backend ends a
 *  preview when a real recording takes the overlay; the poll is how the button
 *  finds out. */
const RECORDING_POLL_MS = 1500;

export interface OnScreenPreviewProps {
  style: OverlayStyle;
  /** The settings-window theme alone, which is what "Copy theme as JSON"
   *  copies. Deliberately not the resolved theme, whose folded-in theme-file
   *  values would hand a tool author a document echoing its own input. */
  settingsTheme: OverlayTheme;
  resetDisabled: boolean;
  hasThemeFileOwnership: boolean;
  onResetConfirm: () => void;
  /** Awaited before the preview starts, so the overlay never shows tokens a
   *  pending debounce hasn't sent yet. */
  onFlushDrafts: () => Promise<void>;
  /** The last Material or Glass style change from the groups below, which may
   *  start the preview by itself. See `autoStartFor`. The tab reports rather
   *  than starts, keeping the decision in one place and the preview this card's
   *  alone. */
  lastSurfaceChange?: PreviewChangeRequest | null;
  /** `glass_support.available` from the resolved theme, whether Glass is what
   *  the overlay would actually draw. A change to Glass on a Mac that cannot
   *  render it now has nothing to show, so it starts nothing. */
  glassAvailable: boolean;
  /** Told whenever the overlay becomes (or stops being) the tab's to repaint
   *  live. See `overlayAcceptsDrafts`. This card owns the preview, so only it
   *  knows; the token rows above need it to weigh a drag against an IPC message
   *  per frame. */
  onAcceptsDraftsChange?: (accepts: boolean) => void;
}

/**
 * The On-Screen Preview card: one Start/Stop button that keeps the real overlay
 * on screen while the theme is edited, the chips that pin its state, and the
 * whole-theme actions (reset, copy as JSON).
 *
 * Preview mode ends when this card goes away. Navigating elsewhere unmounts it;
 * Rust handles the settings window going off screen, since closing to the tray
 * hides it without unmounting and React never learns of it.
 */
export const OnScreenPreview: React.FC<OnScreenPreviewProps> = ({
  style,
  settingsTheme,
  resetDisabled,
  hasThemeFileOwnership,
  onResetConfirm,
  onFlushDrafts,
  lastSurfaceChange = null,
  glassAvailable,
  onAcceptsDraftsChange,
}) => {
  const { t } = useTranslation();
  const [mode, setMode] = useState<PreviewMode>(IDLE_PREVIEW);
  // The state machine's input must be the current mode even inside a callback
  // captured a render ago (the poll below, the unmount cleanup), so the ref is
  // the source of truth and `mode` is only what renders.
  const modeRef = useRef<PreviewMode>(IDLE_PREVIEW);
  // Whether this card is still on screen. Every backend call below is awaited,
  // so any can return to a tab the user has already left. Set on mount, not at
  // construction, because StrictMode mounts, unmounts and mounts again, and a
  // ref initialised once would stay false for the rest of the component's life.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const send = useCallback(
    async (call: PreviewCall, state: PreviewState): Promise<boolean> => {
      try {
        switch (call) {
          case "start": {
            await onFlushDrafts();
            // Leaving during the flush already sent this preview's stop;
            // starting now would leave an overlay with nothing to take it down.
            if (!mountedRef.current) return true;
            const result = await commands.startOverlayPreview(
              state,
              t("settings.appearance.preview.sampleText"),
            );
            if (result.status === "error") {
              console.error(
                "Failed to start the overlay preview:",
                result.error,
              );
              return false;
            }
            return true;
          }
          case "setState":
            await commands.setOverlayPreviewState(state);
            return true;
          case "stop":
            await commands.stopOverlayPreview();
            return true;
          case "none":
            return true;
        }
      } catch (error) {
        console.error("Overlay preview command failed:", error);
        return false;
      }
    },
    [onFlushDrafts, t],
  );

  const dispatch = useCallback(
    (action: PreviewAction) => {
      const { mode: next, call } = reducePreview(modeRef.current, action);
      modeRef.current = next;
      setMode(next);
      void send(call, next.state).then((ok) => {
        // A refused start (the backend decides whether it may run) must not
        // leave the button saying Stop.
        if (
          !ok &&
          call === "start" &&
          mountedRef.current &&
          modeRef.current.running
        ) {
          const stopped = { ...modeRef.current, running: false };
          modeRef.current = stopped;
          setMode(stopped);
        }
      });
    },
    [send],
  );

  // The overlay style can change under a running preview, since the Overlay
  // group is right above this card. Re-pin, or stop when it is turned off.
  useEffect(() => {
    dispatch({ kind: "restyle", style });
  }, [style, dispatch]);

  const [isRecording, setIsRecording] = useState(false);
  useEffect(() => {
    let cancelled = false;
    const poll = () => {
      commands
        .isRecording()
        .then((recording) => {
          if (!cancelled) setIsRecording(recording);
        })
        .catch(() => {
          // Leave the last known state; the button's own attempt surfaces any
          // real failure when clicked.
        });
    };
    poll();
    const id = setInterval(poll, RECORDING_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  // Selecting Glass, or changing the Glass style, shows itself, cycling so the
  // change is on screen at once. `answerPreviewRequest` decides that,
  // once-per-request rule included; this effect owns only the answered-request
  // ref, and the start it dispatches carries `send`'s mounted guard.
  const answeredSeqRef = useRef(0);
  useEffect(() => {
    const answer = answerPreviewRequest(
      lastSurfaceChange,
      answeredSeqRef.current,
      {
        running: modeRef.current.running,
        style,
        isRecording,
        glassAvailable,
      },
    );
    if (!answer) return;
    answeredSeqRef.current = answer.seq;
    if (answer.action) dispatch(answer.action);
  }, [lastSurfaceChange, style, isRecording, glassAvailable, dispatch]);

  // A real recording pre-empts the preview. The backend stops driving and
  // leaves the overlay to that session, so the tab only stops claiming it runs.
  useEffect(() => {
    if (isRecording && modeRef.current.running) dispatch({ kind: "preempted" });
  }, [isRecording, dispatch]);

  // Whether a draft may go out at all, reported up as one boolean, not the
  // mode. The rows need the decision, not the state machine behind it.
  const acceptsDrafts = overlayAcceptsDrafts(mode, isRecording);
  useEffect(() => {
    onAcceptsDraftsChange?.(acceptsDrafts);
  }, [acceptsDrafts, onAcceptsDraftsChange]);

  // Leaving the tab stops the preview. It goes through the reducer like every
  // other action, so what a departure sends stays in the state machine. Not
  // through `dispatch` though, because this cleanup runs after the component is
  // gone, so the call goes out without the state that would follow it.
  useEffect(
    () => () => {
      const { call } = reducePreview(modeRef.current, { kind: "leave" });
      if (call === "stop") void commands.stopOverlayPreview();
    },
    [],
  );

  const [copied, setCopied] = useState(false);
  // Cleared on unmount (and before a second copy restarts it) so the timer
  // never calls setState on a tab the user has left.
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (copiedTimerRef.current !== null) clearTimeout(copiedTimerRef.current);
    },
    [],
  );
  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(themeAsJsonDocument(settingsTheme));
      setCopied(true);
      if (copiedTimerRef.current !== null) clearTimeout(copiedTimerRef.current);
      copiedTimerRef.current = setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error("Failed to copy the overlay theme as JSON:", error);
    }
  };

  const blocker = previewBlocker(isRecording, style);
  const blockedNote =
    blocker === "recording"
      ? t("settings.appearance.preview.blockedRecording")
      : blocker === "overlayOff"
        ? t("settings.appearance.preview.noneNote")
        : null;
  const chips = style === "none" ? [] : previewChipsFor(style);

  return (
    <div className="space-y-3 p-4">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <Button
          variant="primary"
          size="sm"
          disabled={blocker !== null}
          title={blockedNote ?? undefined}
          onClick={() => dispatch({ kind: mode.running ? "stop" : "start" })}
        >
          {mode.running
            ? t("settings.appearance.preview.stop")
            : t("settings.appearance.preview.start")}
        </Button>
        <div className="flex flex-wrap items-center gap-1">
          {chips.map((chip) => (
            <button
              key={chip}
              type="button"
              disabled={blocker !== null}
              aria-pressed={mode.state === chip}
              onClick={() => dispatch({ kind: "pin", state: chip })}
              className={`rounded-full border px-2 py-0.5 text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
                mode.state === chip
                  ? "border-logo-primary bg-logo-primary/20"
                  : "border-transparent text-mid-gray hover:bg-mid-gray/10"
              }`}
            >
              {t(`settings.appearance.preview.states.${chip}`)}
            </button>
          ))}
        </div>
      </div>

      <p className="text-xs text-mid-gray">
        {blockedNote ?? t("settings.appearance.preview.hint")}
      </p>

      <div className="flex items-center gap-2">
        <OverlayThemeReset
          disabled={resetDisabled}
          hasThemeFileOwnership={hasThemeFileOwnership}
          onConfirm={onResetConfirm}
        />
        <Button variant="ghost" size="sm" onClick={handleCopy}>
          {copied
            ? t("settings.appearance.themeFile.copied")
            : t("settings.appearance.themeFile.copy")}
        </Button>
      </div>
    </div>
  );
};
