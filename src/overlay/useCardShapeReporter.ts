import { useEffect } from "react";
import { commands } from "@/bindings";
import type { StreamPhase, StreamTextEvent } from "@/bindings";
import { cardMorphMs, cardShape, type OverlayState } from "./cardShape";

interface CardShapeReporterOptions {
  /** Whether the card is on screen right now. Nothing is reported while hidden. */
  isVisible: boolean;
  /**
   * Whether the effective Material is Glass. The caller already holds the
   * resolved overlay theme, pulling one on every show and listening for
   * changes, so the material arrives from there rather than being fetched
   * a second time here.
   */
  glassActive: boolean;
  state: OverlayState;
  streamText: Pick<StreamTextEvent, "committed" | "tentative">;
  phase: StreamPhase;
}

/**
 * Reports the overlay card's shape to the backend on every change.
 *
 * Under Glass the native window is the card (window slack is zero), and the
 * Live panel's open/collapsed morph is a pure webview decision driven by
 * streamed text and phase, which the backend cannot see any other way. The
 * report also tells the backend that the card has painted, which is what
 * reveals the native blur, so the first report of a session matters even
 * when it repeats the shape the backend already assumed.
 *
 * Under Flat, and off macOS where the effective Material is never Glass,
 * this sends no message at all.
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
