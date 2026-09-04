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
 *  preview the moment a real recording takes the overlay; this is how the
 *  button finds out. */
const RECORDING_POLL_MS = 1500;

export interface OnScreenPreviewProps {
  style: OverlayStyle;
  /** The settings-window theme on its own — what "Copy theme as JSON" puts on
   *  the clipboard, which is deliberately *not* the resolved theme: that one
   *  has the theme file's own values folded in, and copying those back out
   *  would hand a tool author a document echoing its own input. */
  settingsTheme: OverlayTheme;
  resetDisabled: boolean;
  hasThemeFileOwnership: boolean;
  onResetConfirm: () => void;
  /** Awaited before the preview starts, so the overlay never comes up showing
   *  tokens a pending debounce hasn't sent yet. */
  onFlushDrafts: () => Promise<void>;
  /** The last Material or Glass style change the user made in the groups
   *  below, which may start the preview by itself — see `autoStartFor`. The
   *  tab reports the change rather than starting anything, so the decision
   *  stays in one place and this card keeps sole ownership of the preview. */
  lastSurfaceChange?: PreviewChangeRequest | null;
  /** `glass_support.available` from the resolved theme: whether Glass is what
   *  the overlay would actually draw. A change to Glass on a Mac that cannot
   *  render it right now has nothing to show, so it starts nothing. */
  glassAvailable: boolean;
  /** Told whenever the overlay becomes (or stops being) the tab's to repaint
   *  live — see `overlayAcceptsDrafts`. This card owns the preview, so it is
   *  the only thing that knows; the token rows above need it to decide
   *  whether dragging one is worth an IPC message per frame. */
  onAcceptsDraftsChange?: (accepts: boolean) => void;
}

/**
 * The On-Screen Preview card: one Start/Stop button that keeps the *real*
 * overlay on screen while the theme is edited, the chips that pin which state
 * it holds, and the whole-theme actions (reset, copy as JSON).
 *
 * Preview mode ends when this card goes away — navigating to another section
 * unmounts it, and the settings window going off screen is handled in Rust,
 * which is the only place that hears about it (closing to the tray hides the
 * window without unmounting anything, so React never learns of it).
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
  // The state machine's input has to be the *current* mode even inside a
  // callback captured a render ago (the poll below, the unmount cleanup), so
  // the ref is the source of truth and `mode` is only what gets rendered.
  const modeRef = useRef<PreviewMode>(IDLE_PREVIEW);
  // Whether this card is still on screen. Every backend call below is awaited,
  // so any of them can come back to a tab the user has already left. Set on
  // mount rather than at construction: StrictMode mounts, unmounts and mounts
  // again, and a ref initialised once would stay false for the rest of the
  // component's life.
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
            // Leaving the tab during that flush already sent the stop for this
            // preview; starting now would put an overlay on screen with
            // nothing left to take it down.
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
        // A refused start (the backend is the authority on whether it may run)
        // must not leave the button saying Stop.
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

  // The overlay style can change under a running preview (the Overlay group is
  // right above this card): re-pin, or stop outright when it is turned off.
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

  // Selecting Glass, or changing the Glass style, shows itself: the overlay
  // comes up cycling so the change is on screen at once. What that is worth
  // doing about is `answerPreviewRequest`'s call, including the once-per-
  // request rule; all this effect owns is the ref remembering which request
  // was answered, and the start it dispatches carries `send`'s mounted guard.
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

  // A real recording pre-empts the preview: the backend stops driving and
  // leaves the overlay to the session that took it, so the tab only has to
  // stop claiming it is running.
  useEffect(() => {
    if (isRecording && modeRef.current.running) dispatch({ kind: "preempted" });
  }, [isRecording, dispatch]);

  // Whether a draft may go out at all, reported up as one boolean rather than
  // as the mode: what the rows need to know is the decision, not the state
  // machine behind it.
  const acceptsDrafts = overlayAcceptsDrafts(mode, isRecording);
  useEffect(() => {
    onAcceptsDraftsChange?.(acceptsDrafts);
  }, [acceptsDrafts, onAcceptsDraftsChange]);

  // Leaving the tab stops the preview. Through the reducer like every other
  // action, so what a departure sends stays in the state machine — but not
  // through `dispatch`: this runs after the component is gone, so the call
  // goes out without the state that would follow it.
  useEffect(
    () => () => {
      const { call } = reducePreview(modeRef.current, { kind: "leave" });
      if (call === "stop") void commands.stopOverlayPreview();
    },
    [],
  );

  const [copied, setCopied] = useState(false);
  // Cleared on unmount (and before a second copy restarts it) so the timer can
  // never call setState on a tab the user has already navigated away from.
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
