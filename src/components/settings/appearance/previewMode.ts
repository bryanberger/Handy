/**
 * Preview mode's decisions, as pure functions.
 *
 * Preview mode keeps the *real* overlay on screen while the overlay theme is
 * edited: the tab asks the backend to start a driver, pins it to a state or
 * lets it cycle, and stops it again. Everything the tab decides — whether the
 * button may be pressed, which chips exist, and which backend call an action
 * turns into — lives here so it can be tested without a webview.
 */

import type { OverlayStyle, PreviewState } from "@/bindings";

/** The two overlay styles that have something to preview; `none` has nothing
 *  to show. Derived from the binding rather than respelled, so the tab and the
 *  chip table narrow the same setting and cannot drift from it. */
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
 * refusal order (`preview_refusal` in `commands/overlay_preview.rs`):
 * recording outranks everything, because refusing for the right reason
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

export type PreviewAction =
  | { kind: "start" }
  | { kind: "stop" }
  | { kind: "pin"; state: PreviewState }
  /** The overlay style changed under a running preview; its chip may no
   *  longer exist. */
  | { kind: "restyle"; style: OverlayStyle }
  /** A real recording took the overlay — the backend has already ended the
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
 * The tab's state machine. The one rule worth stating: every call the tab
 * makes is derived from the transition rather than from the click handler, so
 * "stop when you leave" and "stop when the button says Stop" cannot drift
 * apart, and a click that changes nothing sends nothing.
 */
export function reducePreview(
  mode: PreviewMode,
  action: PreviewAction,
): PreviewTransition {
  switch (action.kind) {
    case "start":
      if (mode.running) return { mode, call: "none" };
      return { mode: { ...mode, running: true }, call: "start" };

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
        // The overlay was turned off under a running preview: the backend
        // still holds it, so it has to be told to let go.
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
      // Nothing to send: the backend stopped itself and deliberately left the
      // overlay to the recording that took it.
      return { mode: { ...mode, running: false }, call: "none" };

    case "leave":
      return {
        mode: { ...mode, running: false },
        call: mode.running ? "stop" : "none",
      };
  }
}
