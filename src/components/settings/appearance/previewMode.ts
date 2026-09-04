/**
 * Preview mode's decisions, as pure functions.
 *
 * Preview mode keeps the real overlay on screen while the overlay theme is
 * edited. The tab asks the backend to start a driver, pins it to a state or
 * lets it cycle, and stops it again. Everything the tab decides lives here so
 * it can be tested without a webview: whether the button may be pressed,
 * which chips exist, and which backend call an action turns into.
 */

import type { Material, OverlayStyle, PreviewState } from "@/bindings";

/** The two overlay styles that have something to preview; `none` has nothing
 *  to show. Taken from the binding instead of respelled here, so the tab and
 *  the chip table narrow the same setting and cannot drift from it. */
export type PreviewableStyle = Exclude<OverlayStyle, "none">;

/** The chips shown for a style: `cycle` first, then only the states that
 *  style's own sequence visits. Live and Minimal name capture differently
 *  (`listening` vs `recording`), and Live's working spinner covers what
 *  Minimal shows as a separate processing pill. */
export function previewChipsFor(
  style: PreviewableStyle,
): readonly PreviewState[] {
  return style === "live"
    ? (["cycle", "arming", "listening", "transcribing"] as const)
    : (["cycle", "arming", "recording", "transcribing", "processing"] as const);
}

/** Why the Start button is disabled, or `null` when it may be pressed. Each
 *  reason names the i18n key that explains it. */
export type PreviewBlocker = "recording" | "overlayOff";

/**
 * Whether preview mode can be started at all. Mirrors the backend's own
 * refusal order (`preview_refusal` in `src-tauri/src/overlay_preview.rs`).
 * Recording outranks everything, because refusing for the right reason
 * matters more than reporting the first problem found.
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
   *  so pressing Start again resumes the chip the user last chose. */
  state: PreviewState;
}

export const IDLE_PREVIEW: PreviewMode = { running: false, state: "cycle" };

/**
 * Whether the overlay on screen is the tab's to repaint with an uncommitted
 * draft.
 *
 * The same rule the backend enforces (`draft_allowed` in
 * `src-tauri/src/overlay_preview.rs`), asked here as well so a drag does not
 * send an IPC message per animation frame for a command that will refuse it.
 * The backend is the authority and re-checks every draft; the tab is only the
 * one paying for the call.
 *
 * `isRecording` is in the rule rather than assumed away. The tab learns about
 * a pre-emption from its own poll, so there is a window in which it still
 * believes its preview is running while a real session already owns the card.
 */
export function overlayAcceptsDrafts(
  mode: PreviewMode,
  isRecording: boolean,
): boolean {
  return mode.running && !isRecording;
}

export type PreviewAction =
  | { kind: "start" }
  /** Start because a Material or Glass style change wants to be seen, rather
   *  than because the button was pressed. Only [`autoStartFor`] issues it. */
  | { kind: "autoStart" }
  | { kind: "stop" }
  | { kind: "pin"; state: PreviewState }
  /** The overlay style changed under a running preview; its chip may no
   *  longer exist. */
  | { kind: "restyle"; style: OverlayStyle }
  /** A real recording took the overlay. The backend has already ended the
   *  preview, so the tab only has to catch up. */
  | { kind: "preempted" }
  /** The tab is going away (navigating to another section, or the window
   *  closing). */
  | { kind: "leave" };

/** The backend call an action turns into. `none` means the tab's own state
 *  moved and nothing has to be sent. */
export type PreviewCall = "none" | "start" | "setState" | "stop";

export interface PreviewTransition {
  mode: PreviewMode;
  call: PreviewCall;
}

/**
 * The tab's state machine. Every call the tab makes comes from the transition
 * rather than from the click handler, so "stop when you leave" and "stop when
 * the button says Stop" cannot drift apart, and a click that changes nothing
 * sends nothing.
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
      // The loop, whatever chip was remembered. A Material or Glass style
      // change has to show the card in every state, and the user did not ask
      // for a preview at all; they changed a token.
      if (mode.running) return { mode, call: "none" };
      return { mode: { running: true, state: "cycle" }, call: "start" };

    case "stop":
      if (!mode.running) return { mode, call: "none" };
      return { mode: { ...mode, running: false }, call: "stop" };

    case "pin": {
      if (mode.state === action.state) return { mode, call: "none" };
      const next = { ...mode, state: action.state };
      // Pinning while stopped only remembers the choice; the state travels
      // with the next start.
      return { mode: next, call: mode.running ? "setState" : "none" };
    }

    case "restyle": {
      if (action.style === "none") {
        // The user turned the overlay off under a running preview. The
        // backend still holds it, so it has to be told to let go.
        return { mode: IDLE_PREVIEW, call: mode.running ? "stop" : "none" };
      }
      const chips = previewChipsFor(action.style);
      if (chips.includes(mode.state)) return { mode, call: "none" };
      // The pinned chip does not exist in the new style (Live's `listening`
      // vs Minimal's `recording`, say). Fall back to the loop, which every
      // style has.
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
 * or a length is a change to a card the user can already see if a preview is
 * running, and starting one for each of those would put an overlay on screen
 * every time a slider moves.
 */
export type PreviewChange =
  | { kind: "material"; to: Material }
  | { kind: "glassStyle" };

/** One such change as the tab reports it, tagged with a sequence number. Two
 *  identical changes in a row are still two requests, and an answered request
 *  can be told from one still waiting. See [`answerPreviewRequest`]. */
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
  /** `glass_support.available`: whether Glass is what would actually be
   *  painted right now, rather than merely a Material this build supports. */
  glassAvailable: boolean;
}

/**
 * Whether a token change should put the overlay on screen by itself, as the
 * action to dispatch, or `null` for "leave the screen alone".
 *
 * Picking Glass with no preview running used to change nothing visible. The
 * user chose a Material, the card behind the settings window changed, and
 * they had to find the Start button to see it. Selecting Glass, or changing
 * the Glass style, therefore starts the preview itself; the Stop button is
 * right there, and it is the only way out, so nothing can be left on screen
 * without the user being told how to take it off.
 *
 * Choosing Flat starts nothing. Flat is what the overlay already looks like
 * everywhere else, so there is nothing new to show. Glass starts nothing
 * either on a machine that cannot draw it right now, the supported but
 * unavailable case macOS Reduce Transparency leaves. The overlay would come
 * up Flat, answering "show me glass" with the card the user already has.
 *
 * The refusals are [`previewBlocker`]'s rather than a second copy of them. A
 * preview that may not be started by hand may not be started on the user's
 * behalf either.
 */
export function autoStartFor(
  change: PreviewChange,
  state: PreviewAutoStartState,
): PreviewAction | null {
  if (change.kind === "material" && change.to !== "glass") return null;
  // Both changes are about the glass, so both need glass to be what renders.
  if (!state.glassAvailable) return null;
  if (state.running) return null;
  if (previewBlocker(state.isRecording, state.style) !== null) return null;
  return { kind: "autoStart" };
}

/** The answer to one [`PreviewChangeRequest`]: the sequence number that has
 *  now been dealt with, and what to dispatch for it. `null` means "leave the
 *  screen alone", which is still an answer. */
export interface PreviewRequestAnswer {
  seq: number;
  action: PreviewAction | null;
}

/**
 * Answer a change request, at most once.
 *
 * The tab reports what the user did and this says what the preview does about
 * it, so the "once per request" rule is testable rather than a shape an effect
 * happens to have. `answeredSeq` is the last sequence number that got an
 * answer, so a request carrying it is already dealt with. That is what keeps
 * a later `style`, `isRecording` or Glass-availability change from re-running
 * an answered request, and what keeps an asynchronous first resolved payload
 * from faking a change the user never made. `null` means there is nothing to
 * answer and the caller leaves its mark where it is.
 *
 * A request answered with no action still counts as answered. The user's pick
 * was considered and the screen was deliberately left alone.
 */
export function answerPreviewRequest(
  request: PreviewChangeRequest | null | undefined,
  answeredSeq: number,
  state: PreviewAutoStartState,
): PreviewRequestAnswer | null {
  if (!request || request.seq === answeredSeq) return null;
  return { seq: request.seq, action: autoStartFor(request.change, state) };
}
