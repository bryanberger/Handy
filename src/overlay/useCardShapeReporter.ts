import { useEffect } from "react";
import { commands } from "@/bindings";
import type { StreamPhase, StreamTextEvent } from "@/bindings";
import { cardMorphMs, cardShape, type OverlayState } from "./cardShape";

interface CardShapeReporterOptions {
  /** Whether the card is on screen right now. Nothing is reported while hidden. */
  isVisible: boolean;
  /**
   * Whether the effective Material is Glass. The caller already holds the
   * resolved overlay theme, pulling one on every show and listening for changes,
   * so the material comes from there rather than a second fetch here.
   */
  glassActive: boolean;
  state: OverlayState;
  streamText: Pick<StreamTextEvent, "committed" | "tentative">;
  phase: StreamPhase;
}

/**
 * Reports the overlay card's shape to the backend on every change.
 *
 * Under Glass the native window is the card (window slack is zero), and the Live panel's
 * open/collapsed morph is a webview decision from streamed text and phase the backend
 * cannot otherwise see. The report also says the card has painted, revealing the native
 * blur, so a session's first report matters even when it repeats the assumed shape.
 *
 * Under Flat, and off macOS where the effective Material is never Glass, this sends nothing.
 */
export function useCardShapeReporter({
  isVisible,
  glassActive,
  state,
  streamText,
  phase,
}: CardShapeReporterOptions): void {
  const shape = cardShape(state, streamText, phase);

  useEffect(() => {
    if (!isVisible || !glassActive) return;
    void commands.setOverlayCardShape(shape, cardMorphMs());
  }, [shape, isVisible, glassActive]);
}
