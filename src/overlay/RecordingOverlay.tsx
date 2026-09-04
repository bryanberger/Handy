import { emit, listen } from "@tauri-apps/api/event";
import React, { useCallback, useEffect, useRef, useState } from "react";
import "./RecordingOverlay.css";
import { commands, events } from "@/bindings";
import type {
  ResolvedOverlayTheme,
  StreamPhase,
  StreamPhaseEvent,
  StreamTextEvent,
  StreamWorkKind,
  WaveformStyle,
} from "@/bindings";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import {
  applyOverlayTheme,
  BOOLEAN_INHERIT,
  storeOverlayTheme,
  switchToken,
  WAVEFORM_STYLE_INHERIT,
  waveformStyleToken,
} from "@/lib/overlayTheme";
import { getLanguageDirection } from "@/lib/utils/rtl";
import OverlayCard, { type OverlayState } from "./OverlayCard";
import { useCardShapeReporter } from "./useCardShapeReporter";
import { LEVEL_BUCKETS } from "./waveform/waveformLane";
import {
  drawnWaveformStyle,
  isCanvasWaveformStyle,
} from "./waveform/waveformStyles";

// Number of reactive bars in the waveform (the simple, smoothed style shared by
// every overlay form). Mic levels arrive as LEVEL_BUCKETS FFT buckets; we take
// the first N.
const WAVE_BARS = 9;

/** The theme facts this component keeps in state, because the card's markup
 *  reads them rather than a custom property. */
interface OverlayThemeFacts {
  /** Whether the effective Material is Glass. Gates the card-shape reports. */
  glass: boolean;
  /** `show_waveform`, unset meaning shown. */
  showWaveform: boolean;
  /** `show_cancel`, unset meaning shown. */
  showCancel: boolean;
  /** `waveform_style`, unset meaning today's bars. */
  waveformStyle: WaveformStyle;
}

// The resolved theme can come from the localStorage mirror, which bypasses
// Rust, so the two switches go through the apply layer's re-validation like
// every number and colour it paints.
const themeFacts = (resolved: ResolvedOverlayTheme): OverlayThemeFacts => ({
  glass: resolved.effective_material === "glass",
  showWaveform: switchToken(resolved.theme, "show_waveform"),
  showCancel: switchToken(resolved.theme, "show_cancel"),
  waveformStyle: waveformStyleToken(resolved.theme),
});

// Paint a resolved overlay theme and report the facts the markup needs.
const paintOverlayTheme = (
  resolved: ResolvedOverlayTheme,
): OverlayThemeFacts => {
  applyOverlayTheme(document.documentElement, resolved);
  return themeFacts(resolved);
};

// Paint a persisted theme and store it for the next boot. The show pull and the
// change push both do this. A draft does not. It is unpersisted, so mirroring it
// would paint a restart's first frame with a theme the user never settled on.
const paintAndStoreOverlayTheme = (
  resolved: ResolvedOverlayTheme,
): OverlayThemeFacts => {
  const facts = paintOverlayTheme(resolved);
  storeOverlayTheme(resolved);
  return facts;
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
  // The theme facts the markup reads, from the resolved theme painted below:
  // whether Glass is in effect (which gates the card-shape reports) and which
  // of the row's two elements the theme keeps.
  const [theme, setTheme] = useState<OverlayThemeFacts>({
    glass: false,
    showWaveform: BOOLEAN_INHERIT.show_waveform,
    showCancel: BOOLEAN_INHERIT.show_cancel,
    waveformStyle: WAVEFORM_STYLE_INHERIT,
  });
  // Bumped on every repaint, so a canvas waveform style re-reads the colours
  // and the two waveform lengths it caches. A number rather than the facts
  // object above, because a repaint that changes no fact still moves a colour.
  const [themeRevision, setThemeRevision] = useState(0);

  const smoothedLevelsRef = useRef<number[]>(Array(LEVEL_BUCKETS).fill(0));
  // Whether the waveform is on a canvas, read by the microphone listener,
  // which is registered once and cannot see the state above.
  const canvasWaveformRef = useRef(false);
  // The browser gave no 2D context, so every style falls back to the bars. Kept
  // here, not in the card: the bars are fed from the level state above, which a
  // canvas style skips, so a card falling back on its own would draw bars
  // frozen at zero for the rest of the session. Cleared when the style changes,
  // so one failure is not carried into a later choice.
  const [canvasUnavailable, setCanvasUnavailable] = useState(false);
  const canvasUnavailableRef = useRef(false);
  const paintedStyleRef = useRef<WaveformStyle>(WAVEFORM_STYLE_INHERIT);
  const reportCanvasUnavailable = useCallback(() => {
    canvasUnavailableRef.current = true;
    canvasWaveformRef.current = false;
    setCanvasUnavailable(true);
  }, []);
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    const setupEventListeners = async () => {
      // Every repaint lands here: it holds the facts the markup reads, tells
      // the microphone listener whether a canvas is drawing, and moves the
      // revision a canvas style re-measures on.
      const painted = (facts: OverlayThemeFacts) => {
        // A style the user just picked deserves a fresh try at a canvas.
        if (paintedStyleRef.current !== facts.waveformStyle) {
          paintedStyleRef.current = facts.waveformStyle;
          canvasUnavailableRef.current = false;
          setCanvasUnavailable(false);
        }
        setTheme(facts);
        canvasWaveformRef.current = isCanvasWaveformStyle(
          drawnWaveformStyle(facts.waveformStyle, canvasUnavailableRef.current),
        );
        setThemeRevision((revision) => revision + 1);
      };

      const unlistenShow = await listen("show-overlay", async (event) => {
        const overlayState = event.payload as OverlayState;
        // Reset synchronously before settings I/O. A fast microphone can emit
        // recording-ready while the awaits below are in flight; resetting after
        // them would overwrite that event and leave the overlay stuck arming.
        if (overlayState === "recording" || overlayState === "streaming") {
          setCaptureReady(false);
          smoothedLevelsRef.current = Array(LEVEL_BUCKETS).fill(0);
          setLevels(Array(WAVE_BARS).fill(0));
          setStreamText({ committed: "", tentative: "" });
        }

        await syncLanguageFromSettings();
        // The Live panel flows downward from a top overlay and upward from a
        // bottom one; read the placement so the layout can flip to match.
        // This pulls the overlay theme alongside it, one round trip for both,
        // so the card paints the same resolved theme the backend holds.
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
            // Painted here, not in an effect. The custom properties and
            // `data-material` must be on the root before the first painted
            // frame, which an effect would leave to React's batching.
            painted(paintAndStoreOverlayTheme(resolved.data));
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
        // Exponential smoothing across every bucket, then take the first N
        // bars for the shared waveform.
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3;
        });
        smoothedLevelsRef.current = smoothed;
        // A canvas style reads the ref inside its own animation loop, so it
        // skips this state update: those styles cost fewer React renders than
        // the bars, which are DOM elements and need one per frame.
        if (!canvasWaveformRef.current) setLevels(smoothed.slice(0, WAVE_BARS));
      });

      const unlistenStream = await events.streamTextEvent.listen((event) => {
        setStreamText(event.payload);
      });

      // The theme can change while the overlay is visible (a token committed
      // in the Appearance tab), so repaint on every push too.
      const unlistenTheme = await events.resolvedOverlayTheme.listen((event) =>
        painted(paintAndStoreOverlayTheme(event.payload)),
      );

      // The same repaint for a theme still being edited. A draft arrives per
      // animation frame while a slider is dragged, so this handler only paints.
      const unlistenDraft = await events.overlayThemeDraft.listen((event) =>
        painted(paintOverlayTheme(event.payload.resolved)),
      );

      // The app theme is applied in `main.tsx`, which repaints no overlay
      // token, so nothing above moves. A canvas style still has to re-read its
      // colours: the accent follows the app palette.
      const unlistenAppTheme = await listen("theme-changed", () =>
        setThemeRevision((revision) => revision + 1),
      );

      const unlistenPhase = await events.streamPhaseEvent.listen((event) => {
        const payload: StreamPhaseEvent = event.payload;
        setPhase(payload.phase);
        if (payload.kind) setWorkKind(payload.kind);
      });

      // Tauri delivers events only to webviews already listening, so any
      // show before this point was dropped. This lets the backend re-run a
      // missed one, so the first preview after launch is not an empty window.
      void emit("overlay-webview-ready");

      return () => {
        unlistenShow();
        unlistenHide();
        unlistenReady();
        unlistenLevel();
        unlistenStream();
        unlistenTheme();
        unlistenDraft();
        unlistenAppTheme();
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

  useCardShapeReporter({
    isVisible,
    glassActive: theme.glass,
    state,
    streamText,
    phase,
  });

  if (!isVisible) return null;

  // The `.ov-stage` / `.scard` markup lives in OverlayCard so the Appearance
  // tab's preview renders identically to a real dictation. This component owns
  // only the Tauri listeners, the elapsed timer and the overlay position above.
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
      showWaveform={theme.showWaveform}
      showCancel={theme.showCancel}
      waveformStyle={drawnWaveformStyle(theme.waveformStyle, canvasUnavailable)}
      levelsRef={smoothedLevelsRef}
      themeRevision={themeRevision}
      onCanvasUnavailable={reportCanvasUnavailable}
      session={session}
      direction={direction}
      onCancel={() => commands.cancelOperation()}
    />
  );
};

export default RecordingOverlay;
