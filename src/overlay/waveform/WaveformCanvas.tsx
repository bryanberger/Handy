import React, { useEffect, useLayoutEffect, useRef } from "react";
import {
  drawWaveformFrame,
  initialWaveformState,
  MAX_DPR,
  type WaveformCanvasState,
} from "./waveformFrame";
import type { CanvasWaveformStyle } from "./waveformStyles";

/**
 * The one canvas the five non-bars waveform styles are drawn on, and the loop
 * that feeds them.
 *
 * Why a canvas rather than SVG or more DOM: an organic style would rewrite the
 * document every frame in the webview issue #1279 is about, while this is one
 * element, one context and no DOM writes once mounted.
 *
 * What it owns, so no style has to:
 *  - the backing store, recomputed only when the lane's CSS box or the device
 *    pixel ratio changes;
 *  - the levels, read out of the shared ref inside the loop, so a microphone
 *    frame costs no React render (fewer than the bars, which set state);
 *  - the two colours, resolved off probes once per repaint because the
 *    overlay's `--s-*` properties are `color-mix()` values `fillStyle` rejects;
 *  - the two waveform lengths after the size scale, measured off a third
 *    probe for the same reason;
 *  - `prefers-reduced-motion`, honoured by every style ignoring its clock.
 *
 * The frame itself is in `waveformFrame.ts`, so what decides whether a frame is
 * drawn is readable and testable without a browser.
 */

export interface WaveformCanvasProps {
  style: CanvasWaveformStyle;
  /** The 16 smoothed microphone buckets, read inside the loop rather than
   *  passed as a prop, which is what keeps a level frame off React. */
  levelsRef: React.MutableRefObject<number[]>;
  /** False until the first microphone sample: each style draws its own idle
   *  at the muted colour. */
  ready: boolean;
  /** Bumped whenever the apply layer repaints the card, so the cached colours
   *  and lengths are re-read. */
  themeRevision: number;
  /** Called once when the browser gives no 2D context. The overlay owns that
   *  fact, since the fallback bars need the level state a canvas style skips;
   *  a card without a handler keeps the empty canvas. */
  onUnavailable?: () => void;
}

const WaveformCanvasInner: React.FC<WaveformCanvasProps> = ({
  style,
  levelsRef,
  ready,
  themeRevision,
  onUnavailable,
}) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const accentRef = useRef<HTMLSpanElement>(null);
  const mutedRef = useRef<HTMLSpanElement>(null);
  const metricRef = useRef<HTMLSpanElement>(null);
  // Built on the first render alone: the loop mutates this object in place, and
  // a fresh one each render would cost the measurements it draws from.
  // `useRef`'s argument is evaluated whatever the ref holds, so the state is
  // created behind the check.
  const stateRef = useRef<WaveformCanvasState | null>(null);
  const state = (stateRef.current ??= initialWaveformState(style));

  // The props the loop reads. Mirrored after every render rather than closed
  // over, so the loop starts once and survives a style or readiness change.
  useEffect(() => {
    state.flags.ready = ready;
    state.style = style;
  });

  // Before paint, so the first frame is already the right colour and size. A
  // repaint of the theme and a change of style both land here.
  useLayoutEffect(() => {
    const accent = accentRef.current;
    const muted = mutedRef.current;
    const metric = metricRef.current;
    if (accent) state.colors.accent = getComputedStyle(accent).color;
    if (muted) state.colors.muted = getComputedStyle(muted).color;
    if (metric) {
      const box = metric.getBoundingClientRect();
      state.unitCss = box.width;
      state.gapCss = box.height;
    }
    state.moved = true;
  }, [state, themeRevision, style]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) {
      onUnavailable?.();
      return;
    }

    state.ctx = context;
    state.start = performance.now();

    const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    const schemeQuery = window.matchMedia("(prefers-color-scheme: dark)");
    state.flags.reduceMotion = motionQuery.matches;

    // The app palette flips with the OS scheme without any event, so the two
    // cached colours are re-read here as well as on a repaint.
    const remeasureColors = () => {
      if (accentRef.current)
        state.colors.accent = getComputedStyle(accentRef.current).color;
      if (mutedRef.current)
        state.colors.muted = getComputedStyle(mutedRef.current).color;
      state.moved = true;
    };
    const onMotionChange = () => {
      state.flags.reduceMotion = motionQuery.matches;
      state.moved = true;
    };
    motionQuery.addEventListener("change", onMotionChange);
    schemeQuery.addEventListener("change", remeasureColors);

    // The lane's box changes with the size scale and the two waveform lengths,
    // never with the style. Observed, not measured per frame, which would force
    // layout thirty times a second.
    const observer = new ResizeObserver((entries) => {
      const box = entries[0]?.contentRect;
      if (!box) return;
      state.cssWidth = box.width;
      state.cssHeight = box.height;
      state.moved = true;
    });
    observer.observe(canvas);

    let frame = 0;
    const tick = (now: number) => {
      frame = requestAnimationFrame(tick);
      drawWaveformFrame(
        canvas,
        state,
        levelsRef.current,
        now,
        Math.min(window.devicePixelRatio || 1, MAX_DPR),
      );
    };
    const start = () => {
      if (frame === 0 && !document.hidden) frame = requestAnimationFrame(tick);
    };
    const stop = () => {
      if (frame !== 0) cancelAnimationFrame(frame);
      frame = 0;
    };
    // A hidden document stops delivering animation frames anyway; stopping
    // explicitly is what makes the loop bounded rather than merely starved.
    const onVisibility = () => {
      if (document.hidden) stop();
      else start();
    };
    document.addEventListener("visibilitychange", onVisibility);
    start();

    return () => {
      stop();
      observer.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
      motionQuery.removeEventListener("change", onMotionChange);
      schemeQuery.removeEventListener("change", remeasureColors);
      state.ctx = null;
    };
  }, [state, levelsRef, onUnavailable]);

  return (
    <>
      <canvas ref={canvasRef} className="swave-canvas" aria-hidden="true" />
      {/* The probes, out of flow and zero-sized, so the lane is still the
          canvas alone: two colours resolved off `color`, and the two waveform
          lengths off a box the stylesheet sizes. */}
      <span className="swave-probes" aria-hidden="true">
        <span ref={accentRef} style={{ color: "var(--s-accent)" }} />
        <span ref={mutedRef} style={{ color: "var(--s-muted)" }} />
        <span ref={metricRef} className="swave-metric" />
      </span>
    </>
  );
};

/** Memoised: the card re-renders for the elapsed timer and for streaming text,
 *  neither of which this reads. */
export const WaveformCanvas = React.memo(WaveformCanvasInner);
