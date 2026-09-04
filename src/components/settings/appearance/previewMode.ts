/**
 * Preview mode's decisions, as pure functions.
 *
 * Preview mode keeps the real overlay on screen while the theme is edited. The
 * tab starts a backend driver, pins it to a state or lets it cycle, and stops
 * it. Every decision lives here, testable without a webview: may the button be
 * pressed, which chips exist, which backend call an action makes.
 */

import type { Material, OverlayStyle, PreviewState } from "@/bindings";

/** The two previewable overlay styles; `none` has nothing to show. Taken from
 *  the binding, not respelled, so the tab and the chip table narrow the same
 *  setting and cannot drift. */
export type PreviewableStyle = Exclude<OverlayStyle, "none">;

/** The chips for a style: `cycle` first, then the states its sequence visits.
 *  Live and Minimal name capture differently (`listening` vs `recording`), and
 *  Live's spinner covers Minimal's separate processing pill. */
export function previewChipsFor(
  style: PreviewableStyle,
): readonly PreviewState[] {
  return style === "live"
    ? (["cycle", "arming", "listening", "transcribing"] as const)
    : (["cycle", "arming", "recording", "transcribing", "processing"] as const);
}

/** Why the Start button is disabled, or `null` when it may be pressed. Each
 *  reason names the i18n key explaining it. */
export type PreviewBlocker = "recording" | "overlayOff";

/**
 * Whether preview mode can start at all. Mirrors the backend's refusal order
 * (`preview_refusal` in `src-tauri/src/overlay_preview.rs`). Recording outranks
 * everything, so the reason given is the right one, not the first found.
 */
export function previewBlocker(
  isRecording: boolean,
  style: OverlayStyle,
): PreviewBlocker | null {
  if (isRecording) return "recording";
  if (style === "none") return "overlayOff";
  return null;
}

/** What the tab is showing on screen right now. */
export interface PreviewMode {
  /** Whether a preview is believed to be running in the backend. */
  running: boolean;
  /** The state it is pinned to, or `cycle` to loop. Remembered while stopped,
   *  so Start resumes the chip the user last chose. */
  state: PreviewState;
}

export const IDLE_PREVIEW: PreviewMode = { running: false, state: "cycle" };

/**
 * Whether the on-screen overlay is the tab's to repaint with a draft.
 *
 * The backend's rule (`draft_allowed` in `src-tauri/src/overlay_preview.rs`),
 * asked here too so a drag does not send an IPC message per animation frame for
 * a command that will refuse it. The backend re-checks every draft and stays
 * the authority.
 *
 * `isRecording` is in the rule, not assumed away. Pre-emption reaches the tab
 * only by poll, so it can believe its preview runs while a real session owns
 * the card.
 */
export function overlayAcceptsDrafts(
  mode: PreviewMode,
  isRecording: boolean,
): boolean {
  return mode.running && !isRecording;
}

export type PreviewAction =
  | { kind: "start" }
  /** Start because a Material or Glass style change wants to be seen, not
   *  because the button was pressed. Only [`autoStartFor`] issues it. */
  | { kind: "autoStart" }
  | { kind: "stop" }
  | { kind: "pin"; state: PreviewState }
  /** The overlay style changed under a running preview; its chip may vanish. */
  | { kind: "restyle"; style: OverlayStyle }
  /** A real recording took the overlay. The backend already ended the preview,
   *  so the tab only catches up. */
  | { kind: "preempted" }
  /** The tab is going away (another section, or the window closing). */
  | { kind: "leave" };

/** The backend call an action turns into. `none` means the tab's own state
 *  moved and nothing is sent. */
export type PreviewCall = "none" | "start" | "setState" | "stop";

export interface PreviewTransition {
  mode: PreviewMode;
  call: PreviewCall;
}

/**
 * The tab's state machine. Every call comes from the transition, not the click
 * handler, so "stop when you leave" and "stop when the button says Stop" cannot
 * drift apart, and a click that changes nothing sends nothing.
 */
export function reducePreview(
  mode: PreviewMode,
  action: PreviewAction,
): PreviewTransition {
  switch (action.kind) {
    case "start":
      if (mode.running) return { mode, call: "none" };
      return { mode: { ...mode, running: true }, call: "start" };

    case "autoStart":
      // The loop, whatever chip was remembered. A Material or Glass change must
      // show every state, and the user changed a token, not asked for a
      // preview.
      if (mode.running) return { mode, call: "none" };
      return { mode: { running: true, state: "cycle" }, call: "start" };

    case "stop":
      if (!mode.running) return { mode, call: "none" };
      return { mode: { ...mode, running: false }, call: "stop" };

    case "pin": {
      if (mode.state === action.state) return { mode, call: "none" };
      const next = { ...mode, state: action.state };
      // Pinning while stopped only remembers the choice for the next start.
      return { mode: next, call: mode.running ? "setState" : "none" };
    }

    case "restyle": {
      if (action.style === "none") {
        // The user turned the overlay off under a running preview. The backend
        // still holds it, so it must be told to let go.
        return { mode: IDLE_PREVIEW, call: mode.running ? "stop" : "none" };
      }
      const chips = previewChipsFor(action.style);
      if (chips.includes(mode.state)) return { mode, call: "none" };
      // The pinned chip is gone in the new style (Live's `listening` vs
      // Minimal's `recording`). Fall back to the loop, which every style has.
      const next: PreviewMode = { ...mode, state: "cycle" };
      return { mode: next, call: mode.running ? "setState" : "none" };
    }

    case "preempted":
      // Nothing to send. The backend stopped itself and deliberately left the
      // overlay to the recording that took it.
      return { mode: { ...mode, running: false }, call: "none" };

    case "leave":
      return {
        mode: { ...mode, running: false },
        call: mode.running ? "stop" : "none",
      };
  }
}

/**
 * A change the user just made that is worth seeing on the real overlay.
 *
 * Deliberately only the two that change what the surface is made of. A colour
 * or a length changes a card the user can already see when a preview runs, and
 * starting one for each would put an overlay on screen on every slider move.
 */
export type PreviewChange =
  | { kind: "material"; to: Material }
  | { kind: "glassStyle" };

/** One such change as the tab reports it, tagged with a sequence number. Two
 *  identical changes in a row stay two requests, and an answered one is told
 *  from one still waiting. See [`answerPreviewRequest`]. */
export interface PreviewChangeRequest {
  change: PreviewChange;
  seq: number;
}

/** What the tab knows when it decides whether to start showing. */
export interface PreviewAutoStartState {
  /** Whether a preview is already on screen. */
  running: boolean;
  style: OverlayStyle;
  isRecording: boolean;
  /** `glass_support.available`. Whether Glass is what would be painted right
   *  now, not merely a Material this build supports. */
  glassAvailable: boolean;
}

/**
 * Whether a token change should put the overlay on screen by itself, as the
 * action to dispatch, or `null` for "leave the screen alone".
 *
 * Picking Glass with no preview running used to change nothing visible until
 * the user found Start. Selecting Glass, or changing the Glass style, therefore
 * starts the preview itself. Stop is right there and is the only way out, so
 * nothing is left on screen the user cannot take off.
 *
 * Choosing Flat starts nothing; it is what the overlay already looks like
 * everywhere else. Glass starts nothing either on a machine that cannot draw it
 * now, the supported but unavailable case macOS Reduce Transparency leaves. It
 * would come up Flat, answering "show me glass" with the card already there.
 *
 * The refusals are [`previewBlocker`]'s, not a second copy. A preview that may
 * not be started by hand may not be started on the user's behalf.
 */
export function autoStartFor(
  change: PreviewChange,
  state: PreviewAutoStartState,
): PreviewAction | null {
  if (change.kind === "material" && change.to !== "glass") return null;
  // Both changes are about glass, so both need glass to be what renders.
  if (!state.glassAvailable) return null;
  if (state.running) return null;
  if (previewBlocker(state.isRecording, state.style) !== null) return null;
  return { kind: "autoStart" };
}

/** The answer to one [`PreviewChangeRequest`]: the sequence number now dealt
 *  with, and what to dispatch for it. `null` means "leave the screen alone",
 *  which is still an answer. */
export interface PreviewRequestAnswer {
  seq: number;
  action: PreviewAction | null;
}

/**
 * Answer a change request, at most once.
 *
 * The tab reports the change; this decides what the preview does, so "once per
 * request" is testable rather than an effect's shape. `answeredSeq` is the last
 * sequence number answered; a request carrying it is done. That keeps a later
 * `style`, `isRecording` or Glass-availability change from re-running it, and
 * an asynchronous first payload from faking a change the user never made.
 * `null` means nothing to answer and the caller keeps its mark.
 *
 * A request answered with no action still counts as answered. The pick was
 * considered and the screen deliberately left alone.
 */
export function answerPreviewRequest(
  request: PreviewChangeRequest | null | undefined,
  answeredSeq: number,
  state: PreviewAutoStartState,
): PreviewRequestAnswer | null {
  if (!request || request.seq === answeredSeq) return null;
  return { seq: request.seq, action: autoStartFor(request.change, state) };
}
