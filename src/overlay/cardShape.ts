import type {
  OverlayCardShape,
  StreamPhase,
  StreamTextEvent,
} from "@/bindings";

import type { OverlayState } from "./OverlayCard";

export type { OverlayState };

/**
 * The card morph duration to fall back on when the stylesheet cannot be read
 * (a detached document, a test renderer). Kept equal to `--ov-morph-ms`.
 */
const CARD_MORPH_MS_FALLBACK = 460;

/**
 * How long the card's morph between two shapes takes, in milliseconds.
 *
 * Read from `--ov-morph-ms` on the overlay document rather than duplicated
 * here: the same custom property drives `.scard`'s width and border-radius
 * transitions, so the native window morph the backend runs under Glass lasts
 * exactly as long as the CSS morph it replaces. The property is declared in
 * ms, so the number in front of the unit is the value.
 */
export function cardMorphMs(): number {
  const declared = getComputedStyle(document.documentElement).getPropertyValue(
    "--ov-morph-ms",
  );
  const parsed = Number.parseFloat(declared);
  return Number.isFinite(parsed) && parsed >= 0
    ? parsed
    : CARD_MORPH_MS_FALLBACK;
}

/**
 * Which of the five card shapes the overlay is currently drawing, from the
 * same primitive state `RecordingOverlay.tsx` renders from.
 *
 * Must agree with `initial_card_shape` / `OverlayCardShape` in
 * `src-tauri/src/overlay.rs`; pinned there by
 * `initial_card_shape_matches_card_shape_ts`.
 *
 * Streaming mirrors `RecordingOverlay.tsx`'s own `open`/`collapsed`
 * derivation exactly (`open = hasText`, `collapsed = working && !hasText`):
 * text always wins over the working spinner, so the panel never squishes
 * while finalizing; an empty working stream is the collapsed working pill;
 * an idle stream (nothing captured yet) is the resting Live pill.
 */
export function cardShape(
  state: OverlayState,
  streamText: Pick<StreamTextEvent, "committed" | "tentative">,
  phase: StreamPhase,
): OverlayCardShape {
  if (state === "streaming") {
    const hasText =
      streamText.committed.length > 0 || streamText.tentative.length > 0;
    if (hasText) return "live_open";
    if (phase === "working") return "live_working";
    return "live_pill";
  }
  if (state === "transcribing" || state === "processing") {
    return "compact_working";
  }
  return "compact_rest"; // "recording"
}
