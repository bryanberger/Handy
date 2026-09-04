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
 * The Live card's two shape flags, from the two facts that decide them.
 *
 * The single derivation `OverlayCard` renders from and `cardShape` reports
 * from, so the card on screen and the window behind it can never disagree:
 * text always wins over the working spinner, so the panel never squishes a
 * transcript while finalizing, and only a working stream with nothing to
 * preserve collapses to the small pill.
 */
export function liveCardState(
  hasText: boolean,
  working: boolean,
): { open: boolean; collapsed: boolean } {
  return { open: hasText, collapsed: working && !hasText };
}

/**
 * Which of the five card shapes the overlay is currently drawing, from the
 * same primitive state `OverlayCard` renders from.
 *
 * Must agree with `OverlayCardShape` and `OverlayCardShape::initial_for` in
 * `src-tauri/src/overlay_geometry.rs`; pinned there by
 * `initial_card_shape_matches_card_shape_ts`.
 *
 * An idle stream — nothing captured yet, no work running — is the resting
 * Live pill.
 */
export function cardShape(
  state: OverlayState,
  streamText: Pick<StreamTextEvent, "committed" | "tentative">,
  phase: StreamPhase,
): OverlayCardShape {
  if (state === "streaming") {
    const hasText =
      streamText.committed.length > 0 || streamText.tentative.length > 0;
    const { open, collapsed } = liveCardState(hasText, phase === "working");
    if (open) return "live_open";
    if (collapsed) return "live_working";
    return "live_pill";
  }
  if (state === "transcribing" || state === "processing") {
    return "compact_working";
  }
  return "compact_rest"; // "recording"
}
