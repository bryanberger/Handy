import { emit, listen } from "@tauri-apps/api/event";
import React, { useEffect, useRef, useState } from "react";
import "./RecordingOverlay.css";
import { commands, events } from "@/bindings";
import type {
  ResolvedOverlayTheme,
  StreamPhase,
  StreamPhaseEvent,
  StreamTextEvent,
  StreamWorkKind,
} from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { applyOverlayTheme, storeOverlayTheme } from "@/lib/overlayTheme";
import { getLanguageDirection } from "@/lib/utils/rtl";
import OverlayCard, { type OverlayState } from "./OverlayCard";
import { useCardShapeReporter } from "./useCardShapeReporter";

// Number of reactive bars in the waveform (the simple, smoothed style shared by
// every overlay form). Mic levels arrive as 16 FFT buckets; we take the first N.
const WAVE_BARS = 9;

// Paint a resolved overlay theme. Returns whether the effective Material is
// Glass, the one thing about the theme this component has to keep in state.
const paintOverlayTheme = (resolved: ResolvedOverlayTheme): boolean => {
  applyOverlayTheme(document.documentElement, resolved);
  return resolved.effective_material === "glass";
};

// Paint a persisted theme and remember it for the next boot. Both the pull on
// show and the push on change do exactly this. A *draft* deliberately does
// not: it has not been persisted, so mirroring it would let a theme the user
// never settled on paint the first frame after a restart.
const paintAndStoreOverlayTheme = (resolved: ResolvedOverlayTheme): boolean => {
  const glass = paintOverlayTheme(resolved);
  storeOverlayTheme(resolved);
  return glass;
};

const RecordingOverlay: React.FC = () => {
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  // `Stream::play()` returning does not mean hardware callbacks are flowing.
  // Stay visually in an arming state until the backend processes the first
  // actual microphone sample chunk.
  const [captureReady, setCaptureReady] = useState(false);
  const [levels, setLevels] = useState<number[]>(Array(WAVE_BARS).fill(0));
  const [streamText, setStreamText] = useState<StreamTextEvent>({
    committed: "",
    tentative: "",
  });
  const [phase, setPhase] = useState<StreamPhase>("listening");
  const [workKind, setWorkKind] = useState<StreamWorkKind>("transcribing");
  const [elapsed, setElapsed] = useState(0);
  // Bumped on each new streaming session so the Live card remounts fresh (replays
  // the pop-in, and never animates in from the previous panel's open size).
  const [session, setSession] = useState(0);
  // Overlay placement (top vs bottom of the screen). The Live panel grows downward
  // from a top overlay (oldest line under the pill) and upward from a bottom one.
  const [position, setPosition] = useState<"top" | "bottom">("bottom");
  // Whether the effective Material is Glass, from the resolved theme painted
  // below. Gates the card-shape reports, which only Glass needs.
  const [glassActive, setGlassActive] = useState(false);

  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    const setupEventListeners = async () => {
      const unlistenShow = await listen("show-overlay", async (event) => {
        const overlayState = event.payload as OverlayState;
        // Reset synchronously before settings I/O. A fast microphone can emit
        // recording-ready while the awaits below are in flight; resetting after
        // them would overwrite that event and leave the overlay stuck arming.
        if (overlayState === "recording" || overlayState === "streaming") {
          setCaptureReady(false);
          smoothedLevelsRef.current = Array(16).fill(0);
          setLevels(Array(WAVE_BARS).fill(0));
          setStreamText({ committed: "", tentative: "" });
        }

        await syncLanguageFromSettings();
        // The Live panel flows downward from a top overlay and upward from a
        // bottom one; read the placement so the layout can flip to match. The
        // overlay theme is pulled alongside it, so the card is painted from the
        // same resolved theme the backend holds — one round trip for both.
        try {
          const [settings, resolved] = await Promise.all([
            commands.getAppSettings(),
            commands.getResolvedOverlayTheme(),
          ]);
          if (settings.status === "ok") {
            setPosition(
              settings.data.overlay_position === "top" ? "top" : "bottom",
            );
          }
          if (resolved.status === "ok") {
            // Imperative, not an effect: the custom properties and
            // `data-material` must be on the root before the first painted
            // frame, which an effect would leave to React's batching.
            setGlassActive(paintAndStoreOverlayTheme(resolved.data));
          }
        } catch {
          // Keep the previous/default placement and theme if either read fails.
        }
        setState(overlayState);
        if (overlayState === "streaming") {
          setPhase("listening");
          setWorkKind("transcribing");
          setElapsed(0);
          setSession((s) => s + 1); // remount the card fresh for this session
        }
        setIsVisible(true);
      });

      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
        setCaptureReady(false);
      });

      const unlistenReady = await listen("recording-ready", () => {
        setElapsed(0);
        setCaptureReady(true);
      });

      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];
        // Exponential smoothing across the 16 buckets, then take the first N
        // bars for the shared waveform.
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3;
        });
        smoothedLevelsRef.current = smoothed;
        setLevels(smoothed.slice(0, WAVE_BARS));
      });

      const unlistenStream = await events.streamTextEvent.listen((event) => {
        setStreamText(event.payload);
      });

      // The theme can change while the overlay is visible (a token committed
      // in the Appearance tab), so repaint on every push as well.
      const unlistenTheme = await events.resolvedOverlayTheme.listen((event) =>
        setGlassActive(paintAndStoreOverlayTheme(event.payload)),
      );

      // The same repaint for a theme still being edited. It arrives per
      // animation frame while a slider is dragged, so it does the least it
      // can: paint, and nothing else.
      const unlistenDraft = await events.overlayThemeDraft.listen((event) =>
        setGlassActive(paintOverlayTheme(event.payload.resolved)),
      );

      const unlistenPhase = await events.streamPhaseEvent.listen((event) => {
        const payload: StreamPhaseEvent = event.payload;
        setPhase(payload.phase);
        if (payload.kind) setWorkKind(payload.kind);
      });

      // Tauri delivers an event only to webviews already listening for it, so
      // every show emitted before this point was dropped. Saying so lets the
      // backend re-run one it missed — without this the first preview after a
      // launch maps an empty overlay window.
      void emit("overlay-webview-ready");

      return () => {
        unlistenShow();
        unlistenHide();
        unlistenReady();
        unlistenLevel();
        unlistenStream();
        unlistenTheme();
        unlistenDraft();
        unlistenPhase();
      };
    };

    setupEventListeners();
  }, []);

  // Elapsed capture timer starts only once microphone samples are flowing.
  useEffect(() => {
    if (state !== "streaming" || !isVisible || !captureReady) return;
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [state, isVisible, captureReady]);

  useCardShapeReporter({ isVisible, glassActive, state, streamText, phase });

  if (!isVisible) return null;

  // The presentational markup — the `.ov-stage` / `.scard` tree — lives in
  // OverlayCard so the Appearance tab's preview renders identically to a real
  // dictation. This component owns only the Tauri listeners, the elapsed
  // timer and the overlay position above.
  return (
    <OverlayCard
      state={state}
      captureReady={captureReady}
      levels={levels}
      streamText={streamText}
      phase={phase}
      workKind={workKind}
      elapsed={elapsed}
      position={position}
      session={session}
      direction={direction}
      onCancel={() => commands.cancelOperation()}
    />
  );
};

export default RecordingOverlay;
