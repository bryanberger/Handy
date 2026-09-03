import { useCallback, useEffect, useRef, useState } from "react";
import type {
  OverlayStyle,
  StreamPhase,
  StreamTextEvent,
  StreamWorkKind,
} from "@/bindings";
import type { OverlayCardProps, OverlayState } from "@/overlay/OverlayCard";

/** The two overlay styles the preview can render; `none` has nothing to show.
 *  Derived from the binding rather than respelled, so the preview, the fit
 *  math and the tab all narrow the same setting and cannot drift from it. */
export type OverlayPreviewStyle = Exclude<OverlayStyle, "none">;

/** The named states the preview cycles through — the chip row's labels. */
export type PreviewStateName =
  | "arming"
  | "recording"
  | "listening"
  | "transcribing"
  | "processing";

type PreviewStepName = PreviewStateName | "gap";

export interface PreviewStep {
  name: PreviewStepName;
  durationMs: number;
}

/** Minimal: arming 0.8s → recording 3.7s → transcribing 1.6s → processing
 *  1.4s → 0.6s gap (card unmounted, so the pop-in replays) → loop. ~8.1s. */
export const MINIMAL_SEQUENCE: readonly PreviewStep[] = [
  { name: "arming", durationMs: 800 },
  { name: "recording", durationMs: 3700 },
  { name: "transcribing", durationMs: 1600 },
  { name: "processing", durationMs: 1400 },
  { name: "gap", durationMs: 600 },
];

/** Live: arming 0.8s → listening 4.4s (text streams in) → working 1.9s (text
 *  held) → 0.6s gap → loop. ~7.7s. */
export const LIVE_SEQUENCE: readonly PreviewStep[] = [
  { name: "arming", durationMs: 800 },
  { name: "listening", durationMs: 4400 },
  { name: "transcribing", durationMs: 1900 },
  { name: "gap", durationMs: 600 },
];

export function sequenceFor(
  style: OverlayPreviewStyle,
): readonly PreviewStep[] {
  return style === "live" ? LIVE_SEQUENCE : MINIMAL_SEQUENCE;
}

/** The chips shown for a style — only the states that style ever visits. */
export function pinnableStatesFor(
  style: OverlayPreviewStyle,
): readonly PreviewStateName[] {
  return style === "live"
    ? (["arming", "listening", "transcribing"] as const)
    : (["arming", "recording", "transcribing", "processing"] as const);
}

export function totalDurationMs(sequence: readonly PreviewStep[]): number {
  return sequence.reduce((sum, step) => sum + step.durationMs, 0);
}

export interface ActiveStep {
  index: number;
  name: PreviewStepName;
  elapsedInStepMs: number;
  /** Full loops completed. Feeds `OverlayCard`'s `session` prop, so the card
   *  remounts — and its pop-in replays — once per cycle. */
  cycle: number;
}

/**
 * Pure: which step of `sequence` is active `elapsedMs` after the driver
 * started, wrapping modulo the sequence's total duration.
 */
export function stepAt(
  sequence: readonly PreviewStep[],
  elapsedMs: number,
): ActiveStep {
  const total = totalDurationMs(sequence);
  if (total <= 0 || sequence.length === 0) {
    return { index: 0, name: "arming", elapsedInStepMs: 0, cycle: 0 };
  }
  const safeElapsed = Math.max(0, elapsedMs);
  const cycle = Math.floor(safeElapsed / total);
  let remainder = safeElapsed - cycle * total;
  for (let i = 0; i < sequence.length; i++) {
    const step = sequence[i];
    if (remainder < step.durationMs) {
      return { index: i, name: step.name, elapsedInStepMs: remainder, cycle };
    }
    remainder -= step.durationMs;
  }
  // Floating-point edge case landing exactly on the total: the last step.
  const lastIndex = sequence.length - 1;
  return {
    index: lastIndex,
    name: sequence[lastIndex].name,
    elapsedInStepMs: sequence[lastIndex].durationMs,
    cycle,
  };
}

/** The elapsed-ms offset of the midpoint of the first step named `name` —
 *  used to show a settled, representative frame when a chip is pinned. */
export function offsetFor(
  sequence: readonly PreviewStep[],
  name: PreviewStateName,
): number {
  let offset = 0;
  for (const step of sequence) {
    if (step.name === name) return offset + step.durationMs / 2;
    offset += step.durationMs;
  }
  return 0;
}

const MID_HEIGHT_LEVELS: readonly number[] = Array(16).fill(0.5);

/**
 * 16 deterministic buckets in [0, 1] — the sum of two sines plus a third,
 * faster and quieter one standing in for jitter (never `Math.random()`, so
 * the driver stays pure and its output reproducible in a test).
 */
export function syntheticLevels(elapsedMs: number, bucketCount = 16): number[] {
  const t = elapsedMs / 1000;
  return Array.from({ length: bucketCount }, (_, i) => {
    const phase = i * 0.4;
    const slow = Math.sin(t * 3.1 + phase) * 0.5 + 0.5;
    const fast = Math.sin(t * 5.7 + phase * 1.7) * 0.5 + 0.5;
    const jitter = Math.sin(t * 13.3 + i * 2.1) * 0.08;
    const value = slow * 0.6 + fast * 0.4 + jitter;
    return Math.max(0, Math.min(1, value));
  });
}

const EMPTY_STREAM_TEXT: StreamTextEvent = { committed: "", tentative: "" };

/**
 * The sample sentence revealed word by word, the next word held as
 * `tentative` — exercises the same committed/tentative split and caret a real
 * stream does.
 */
export function revealedText(
  sampleText: string,
  elapsedMs: number,
  msPerWord = 260,
): StreamTextEvent {
  const words =
    sampleText.trim().length > 0 ? sampleText.trim().split(/\s+/) : [];
  if (words.length === 0) return EMPTY_STREAM_TEXT;
  const revealed = Math.max(
    0,
    Math.min(words.length, Math.floor(elapsedMs / msPerWord)),
  );
  return {
    committed: words.slice(0, revealed).join(" "),
    tentative: words[revealed] ?? "",
  };
}

type DriverCardProps = Pick<
  OverlayCardProps,
  | "state"
  | "captureReady"
  | "levels"
  | "streamText"
  | "phase"
  | "workKind"
  | "elapsed"
>;

export interface CardPropsResult {
  /** False during the loop's gap — the caller should not render OverlayCard
   *  at all, so the pop-in animation replays at the top of the next cycle. */
  mounted: boolean;
  session: number;
  activeState: PreviewStateName;
  props: DriverCardProps;
}

const WORKING_PHASE: StreamPhase = "working";
const LISTENING_PHASE: StreamPhase = "listening";
const TRANSCRIBING_KIND: StreamWorkKind = "transcribing";

/**
 * Every frame the preview can show differs in at most five fields; the rest
 * (the work kind, the empty transcript, a zero timer) is the same every time.
 * One builder with those defaults keeps each state below a single line and
 * stops a seventh near-identical object literal from drifting from the other
 * six.
 */
function frame(
  active: ActiveStep,
  activeState: PreviewStateName,
  levels: number[],
  props: {
    state: OverlayState;
    captureReady: boolean;
    streamText?: StreamTextEvent;
    phase?: StreamPhase;
    elapsed?: number;
    mounted?: boolean;
  },
): CardPropsResult {
  return {
    mounted: props.mounted ?? true,
    session: active.cycle,
    activeState,
    props: {
      state: props.state,
      captureReady: props.captureReady,
      levels,
      streamText: props.streamText ?? EMPTY_STREAM_TEXT,
      phase: props.phase ?? LISTENING_PHASE,
      workKind: TRANSCRIBING_KIND,
      elapsed: props.elapsed ?? 0,
    },
  };
}

/**
 * Pure: the OverlayCard props for `style` at `cycleElapsedMs` into the loop.
 * `animated` selects between the synthetic waveform (playing) and a static
 * mid-height one (paused, pinned, or `prefers-reduced-motion: reduce`).
 */
export function cardPropsAt(
  style: OverlayPreviewStyle,
  cycleElapsedMs: number,
  sampleText: string,
  animated: boolean,
): CardPropsResult {
  const sequence = sequenceFor(style);
  const active = stepAt(sequence, cycleElapsedMs);
  const levels = animated
    ? syntheticLevels(cycleElapsedMs)
    : [...MID_HEIGHT_LEVELS];
  const elapsed = Math.floor(active.elapsedInStepMs / 1000);
  const cardState: OverlayState = style === "live" ? "streaming" : "recording";

  if (active.name === "gap") {
    return frame(active, "arming", levels, {
      state: cardState,
      captureReady: false,
      mounted: false,
    });
  }

  if (style === "live") {
    if (active.name === "arming") {
      return frame(active, "arming", levels, {
        state: "streaming",
        captureReady: false,
      });
    }
    if (active.name === "listening") {
      return frame(active, "listening", levels, {
        state: "streaming",
        captureReady: true,
        streamText: revealedText(sampleText, active.elapsedInStepMs),
        elapsed,
      });
    }
    // "transcribing" (working): hold the text exactly where listening left
    // it, rather than restarting the reveal from this step's own clock.
    const listeningDuration = sequence[active.index - 1]?.durationMs ?? 0;
    return frame(active, "transcribing", levels, {
      state: "streaming",
      captureReady: true,
      streamText: revealedText(sampleText, listeningDuration),
      phase: WORKING_PHASE,
      elapsed,
    });
  }

  // Minimal.
  switch (active.name) {
    case "arming":
      return frame(active, "arming", levels, {
        state: "recording",
        captureReady: false,
      });
    case "transcribing":
      return frame(active, "transcribing", levels, {
        state: "transcribing",
        captureReady: true,
        phase: WORKING_PHASE,
      });
    case "processing":
      return frame(active, "processing", levels, {
        state: "processing",
        captureReady: true,
        phase: WORKING_PHASE,
      });
    default: // "recording"
      return frame(active, "recording", levels, {
        state: "recording",
        captureReady: true,
        elapsed,
      });
  }
}

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

function usePrefersReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () =>
      typeof window !== "undefined" &&
      window.matchMedia(REDUCED_MOTION_QUERY).matches,
  );
  useEffect(() => {
    if (typeof window === "undefined") return;
    const mql = window.matchMedia(REDUCED_MOTION_QUERY);
    const handler = () => setReduced(mql.matches);
    mql.addEventListener("change", handler);
    return () => mql.removeEventListener("change", handler);
  }, []);
  return reduced;
}

export interface UseOverlayPreviewDriverResult {
  mounted: boolean;
  session: number;
  activeState: PreviewStateName;
  availableStates: readonly PreviewStateName[];
  playing: boolean;
  togglePlay: () => void;
  pinState: (name: PreviewStateName) => void;
  cardProps: DriverCardProps;
}

/**
 * Drives the in-page preview: an auto-advancing, looping fake state machine
 * with synthetic audio levels and a word-by-word transcript, so the tab shows
 * every control's effect without opening the real overlay per slider drag.
 */
export function useOverlayPreviewDriver(
  style: OverlayPreviewStyle,
  sampleText: string,
): UseOverlayPreviewDriverResult {
  const prefersReducedMotion = usePrefersReducedMotion();
  const [playing, setPlaying] = useState(!prefersReducedMotion);
  const [pinnedState, setPinnedState] = useState<PreviewStateName | null>(
    prefersReducedMotion ? "arming" : null,
  );
  const [elapsedMs, setElapsedMs] = useState(0);
  const rafRef = useRef<number | null>(null);
  const lastFrameRef = useRef<number | null>(null);

  // A style switch (Minimal <-> Live) has a different sequence and total
  // duration; start its cycle fresh rather than reinterpreting the old clock.
  useEffect(() => {
    setElapsedMs(0);
    lastFrameRef.current = null;
  }, [style]);

  useEffect(() => {
    if (!playing) {
      lastFrameRef.current = null;
      return;
    }
    let cancelled = false;
    const tick = (now: number) => {
      if (cancelled) return;
      const last = lastFrameRef.current ?? now;
      lastFrameRef.current = now;
      setElapsedMs((prev) => prev + (now - last));
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => {
      cancelled = true;
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
    };
  }, [playing]);

  // Reduce Motion is a live setting, not just a starting condition: turning it
  // on mid-session parks the cycle on a settled frame, turning it off resumes
  // it. Compared against the previous value so this only ever reacts to an
  // actual transition and never overrides the user's own play/pause.
  const wasReducedRef = useRef(prefersReducedMotion);
  useEffect(() => {
    if (wasReducedRef.current === prefersReducedMotion) return;
    wasReducedRef.current = prefersReducedMotion;
    if (prefersReducedMotion) {
      setPlaying(false);
      setPinnedState((pinned) => pinned ?? "arming");
    } else {
      setPinnedState(null);
      setPlaying(true);
    }
  }, [prefersReducedMotion]);

  const togglePlay = useCallback(() => {
    setPinnedState(null);
    setPlaying((p) => !p);
  }, []);

  const pinState = useCallback((name: PreviewStateName) => {
    setPlaying(false);
    setPinnedState(name);
  }, []);

  const sequence = sequenceFor(style);
  const cycleElapsedMs =
    pinnedState !== null ? offsetFor(sequence, pinnedState) : elapsedMs;
  const result = cardPropsAt(style, cycleElapsedMs, sampleText, playing);

  return {
    mounted: result.mounted,
    session: result.session,
    activeState: pinnedState ?? result.activeState,
    availableStates: pinnableStatesFor(style),
    playing,
    togglePlay,
    pinState,
    cardProps: result.props,
  };
}
