import { useCallback, useEffect, useRef, useState } from "react";
import { commands } from "@/bindings";
import type { OverlayTheme } from "@/bindings";
import {
  commitOverlayTheme,
  persistedOverlayTheme,
} from "@/hooks/useResolvedOverlayTheme";
import { INHERIT_ALL, type OverlayThemeKey } from "@/lib/overlayTheme";

export const DRAFT_DEBOUNCE_MS = 120;

/**
 * Coalesces a burst of values to at most one delivery per animation frame,
 * leading edge first.
 *
 * That rule is why this is a class rather than three lines in the hook. The
 * first push after a quiet moment goes out at once, so the first pixel of a
 * drag is already on screen; later pushes in that frame are held, and only the
 * last goes out when the frame comes round. A frame with nothing new sends
 * nothing and goes quiet, so a settled control costs nothing.
 *
 * `schedule` is injected so the rule is testable without a browser, and
 * defaults to `requestAnimationFrame`, which makes the delivery rate follow
 * the display rather than the input device.
 *
 * A held value is one the caller has not seen delivered. [`flush`] sends it
 * now, as a commit does, because the last frame of a drag must not be dropped.
 * [`cancel`] throws it away, as a reset and unmount do, because the value is
 * abandoned; delivering it a frame later would repaint over what replaced it.
 */
export class FrameCoalescer<Value> {
  private pending: { value: Value } | null = null;
  private frameQueued = false;
  /** Bumped by [`cancel`], so a frame scheduled before it is ignored when it
   *  runs. `requestAnimationFrame` returns a handle, but the injected scheduler
   *  need not, so a cancelled frame is disarmed rather than un-scheduled. */
  private generation = 0;

  constructor(
    private readonly deliver: (value: Value) => void,
    private readonly schedule: (run: () => void) => void = (run) =>
      requestAnimationFrame(run),
  ) {}

  push(value: Value): void {
    if (this.frameQueued) {
      this.pending = { value };
      return;
    }
    this.frameQueued = true;
    this.deliver(value);
    const generation = this.generation;
    this.schedule(() => this.runFrame(generation));
  }

  /** Deliver the held value now, if there is one.
   *
   *  A commit calls this first. The debounce can fire between a push and its
   *  frame while holding the value being committed, so without this the last
   *  frame of a drag never arrives. The queued frame stays armed, so the
   *  one-per-frame rate is unchanged. */
  flush(): void {
    const pending = this.pending;
    this.pending = null;
    if (pending) this.deliver(pending.value);
  }

  /** Drop the held value; the next push is a leading edge again.
   *
   *  A reset and an unmount call this. The value is abandoned, and delivering
   *  it a frame later would repaint over whatever replaced it. */
  cancel(): void {
    this.pending = null;
    this.frameQueued = false;
    this.generation += 1;
  }

  private runFrame(generation: number): void {
    if (generation !== this.generation) return;
    this.frameQueued = false;
    const pending = this.pending;
    this.pending = null;
    if (pending) this.push(pending.value);
  }
}

/**
 * A framework-agnostic debounced-commit engine, split out of the
 * `useDraftSetting` hook so its one rule is testable without React or a
 * fake-timer library. The rule is "only the latest value scheduled inside a
 * debounce window is ever committed, exactly once".
 *
 * `schedule(key, value)` resets that key's timer, so a burst inside one window
 * collapses to one commit of the last value. A `flush` (explicit or the timer
 * firing) commits at once, resolving after `onSettled` runs. `onSettled` runs
 * only if no newer `schedule` landed for the key meanwhile, tracked by a
 * per-key generation counter so a slow, late-resolving commit cannot clear a
 * newer edit's draft.
 */
export class DraftDebouncer<Key extends string, Value> {
  private readonly timers = new Map<Key, ReturnType<typeof setTimeout>>();
  private readonly latest = new Map<Key, Value>();
  private readonly generation = new Map<Key, number>();

  constructor(
    private readonly onCommit: (key: Key, value: Value) => void | Promise<void>,
    private readonly onSettled: (key: Key) => void,
    private readonly debounceMs: number = DRAFT_DEBOUNCE_MS,
  ) {}

  /** Update the pending value and (re)start its debounce timer. */
  schedule(key: Key, value: Value): void {
    this.latest.set(key, value);
    this.generation.set(key, (this.generation.get(key) ?? 0) + 1);
    this.clearTimer(key);
    this.timers.set(
      key,
      setTimeout(() => void this.flush(key), this.debounceMs),
    );
  }

  /** Commit the pending value now, resolving once it has settled. A no-op
   *  (already resolved) if nothing is pending for `key`. */
  flush(key: Key): Promise<void> {
    if (!this.timers.has(key)) return Promise.resolve();
    this.clearTimer(key);
    const value = this.latest.get(key) as Value;
    const generation = this.generation.get(key) ?? 0;
    return Promise.resolve(this.onCommit(key, value)).then(() => {
      if (this.generation.get(key) === generation) this.onSettled(key);
    });
  }

  flushAll(): Promise<void> {
    const pending = [...this.timers.keys()].map((key) => this.flush(key));
    return Promise.all(pending).then(() => undefined);
  }

  /** Drop a pending debounce without committing it (a reset supersedes it). */
  cancel(key: Key): void {
    this.clearTimer(key);
    this.generation.set(key, (this.generation.get(key) ?? 0) + 1);
  }

  isPending(key: Key): boolean {
    return this.timers.has(key);
  }

  private clearTimer(key: Key): void {
    const timer = this.timers.get(key);
    if (timer !== undefined) clearTimeout(timer);
    this.timers.delete(key);
  }
}

/**
 * Commit one overlay-theme token. `value: null` resets that token to inherit,
 * which takes the key out of the theme file.
 *
 * Reads the persisted theme at call time, not a captured closure, so two edits
 * in flight compose. One `change_overlay_theme_setting` sends all twenty-two
 * tokens, writes the theme file and answers with the document it read back.
 */
export async function setOverlayThemeToken<K extends keyof OverlayTheme>(
  key: K,
  value: OverlayTheme[K],
): Promise<void> {
  await commitOverlayTheme({ ...persistedOverlayTheme(), [key]: value });
}

/** The theme file's tokens with every uncommitted draft laid over them. What
 *  the overlay would look like if the user stopped dragging now. */
function themeWithDrafts(stored: OverlayTheme, draft: Draft): OverlayTheme {
  return { ...INHERIT_ALL, ...stored, ...draft };
}

type Draft = Partial<OverlayTheme>;

/** Everything [`DraftEngine`] does to the world, injected so its ordering
 *  rules are testable without React, Tauri or a browser. */
export interface DraftEffects {
  /** Paint the overlay with a theme that is not (yet) stored. */
  paint: (theme: OverlayTheme) => void;
  /** Persist one token; `null` resets it to inherit. */
  commit: <K extends OverlayThemeKey>(
    key: K,
    value: OverlayTheme[K],
  ) => void | Promise<void>;
  /** The tokens as the theme file has them, read at call time, not captured.
   *  Two edits in flight must compose instead of clobbering each other. */
  storedTheme: () => OverlayTheme;
  /** The draft map changed; the tab re-renders from this. */
  onDraftChange: (draft: Draft) => void;
  /** Whether the overlay is the tab's to paint (preview running, nothing
   *  recording). Asked per push, because a recording can take it mid-drag. */
  canPaint: () => boolean;
}

/**
 * The two clocks of live editing, and the rules that keep them in step.
 *
 * A token edit goes two places at two rates, the overlay at frame rate and the
 * theme file on a 120 ms debounce, so every bug it prevents is an ordering bug
 * between them. The rules live in one place, testable as rules rather than
 * as React wiring:
 *
 *  1. The last frame is never dropped. A commit flushes the coalescer first,
 *     so the value persisted is also the last one painted.
 *  2. An abandoned draft never lands. A reset cancels the queued frame before
 *     it can repaint over what replaces it, and paints the corrected theme
 *     itself so the screen does not wait on the round trip.
 *  3. The screen ends on a stored value. The reset then commits `null`; when
 *     that commit finds nothing to write, Rust still re-delivers, because the
 *     draft left a mark (`OVERLAY_DRAFTED` in `commands/overlay_theme.rs`).
 *     Both halves are needed, one instant, one authoritative.
 *  4. Nothing is painted onto an overlay the tab does not own. `canPaint`
 *     gates every push, so no IPC per frame for a preview that is not running;
 *     the backend refuses it anyway (`draft_allowed`).
 */
export class DraftEngine {
  private draft: Draft = {};
  private readonly coalescer: FrameCoalescer<OverlayTheme>;
  private readonly debouncer: DraftDebouncer<
    OverlayThemeKey,
    OverlayTheme[OverlayThemeKey]
  >;

  constructor(
    private readonly effects: DraftEffects,
    schedule?: (run: () => void) => void,
    debounceMs: number = DRAFT_DEBOUNCE_MS,
  ) {
    this.coalescer = new FrameCoalescer<OverlayTheme>(
      (theme) => this.effects.paint(theme),
      schedule,
    );
    this.debouncer = new DraftDebouncer<
      OverlayThemeKey,
      OverlayTheme[OverlayThemeKey]
    >(
      (key, value) => {
        // Rule 1: the frame being held is the value about to be stored.
        this.coalescer.flush();
        return this.effects.commit(key, value);
      },
      (key) => this.clear(key),
      debounceMs,
    );
  }

  /** Edit a token. Paint it now, store it once the drag settles. */
  set<K extends OverlayThemeKey>(key: K, value: OverlayTheme[K]): void {
    this.draft = { ...this.draft, [key]: value };
    this.effects.onDraftChange(this.draft);
    this.push(this.themeWith(this.draft));
    this.debouncer.schedule(key, value);
  }

  /** Commit a still-pending token now (pointer up, focus out). */
  flush(key: OverlayThemeKey): Promise<void> {
    return this.debouncer.flush(key);
  }

  /** Commit everything still pending now, and wait for it. */
  flushAll(): Promise<void> {
    return this.debouncer.flushAll();
  }

  /** Abandon the draft and reset the token to inherit. */
  reset(key: OverlayThemeKey): void {
    this.debouncer.cancel(key);
    this.clear(key);
    // Rule 2, then rule 3. The queued frame carries the abandoned value, so
    // it goes, the corrected theme replaces it, the commit makes it official.
    this.coalescer.cancel();
    this.push(this.themeWith({ ...this.draft, [key]: null }));
    void this.effects.commit(key, null);
  }

  /** The tab is going away. Commit what is pending, paint nothing more. */
  dispose(): Promise<void> {
    const flushed = this.debouncer.flushAll();
    this.coalescer.cancel();
    return flushed;
  }

  private push(theme: OverlayTheme): void {
    if (this.effects.canPaint()) this.coalescer.push(theme);
  }

  private themeWith(draft: Draft): OverlayTheme {
    return themeWithDrafts(this.effects.storedTheme(), draft);
  }

  private clear(key: OverlayThemeKey): void {
    if (!(key in this.draft)) return;
    const next = { ...this.draft };
    delete next[key];
    this.draft = next;
    this.effects.onDraftChange(next);
  }
}

export interface UseDraftSettingResult {
  /** Values being edited but not yet committed. Read as `draft[key] ?? the
   *  theme file's own value`, never alone as the persisted value. */
  draft: Draft;
  /** Update the draft now, then write it to the theme file on a 120 ms
   *  trailing debounce. The tab's controls read the draft; the on-screen
   *  overlay gets it once per animation frame. */
  setDraft: <K extends OverlayThemeKey>(key: K, value: OverlayTheme[K]) => void;
  /** Commit a still-pending draft now. Wire to `onPointerUp` / `onFocusOut`
   *  so the debounce never outlives the drag or keystroke behind it, or "Show
   *  on screen" can fire before the end of a slider drag reaches Rust. */
  flush: (key: OverlayThemeKey) => Promise<void>;
  /** Commit every pending draft now and wait. "Show on screen" awaits this
   *  before its command, so the on-screen overlay shows no stale tokens. */
  flushAll: () => Promise<void>;
  /** Cancel any pending debounce and reset the token to inherit. */
  reset: (key: OverlayThemeKey) => void;
}

/**
 * Local draft state for the overlay-theme tokens, shared by ColorField
 * and the token sliders.
 *
 * `ui/Slider` fires `onChange` on every pixel of drag (`Slider.tsx`), and a
 * native `<input type="color">` fires `onInput` continuously inside the OS
 * picker. Those two rates need two treatments, and [`DraftEngine`] runs both:
 *
 *  - it writes the theme file on a 120 ms trailing debounce
 *    (`DraftDebouncer`), because persisting per pixel means a file read,
 *    write and broadcast per frame;
 *  - the on-screen overlay gets the draft at frame rate (`FrameCoalescer` ->
 *    `preview_overlay_theme_draft`), because a trailing debounce never fires
 *    during an unbroken drag, so the card only caught up when the user stopped.
 *
 * The hook adds only React, state for the draft map and one stable engine for
 * the tab's lifetime.
 *
 * @param overlayIsOurs whether a preview the tab started is on screen now
 * (`overlayAcceptsDrafts` in `previewMode.ts`). While it is false nothing is
 * painted and no IPC goes out. There is no overlay of the tab's to paint, and
 * the backend would refuse the draft anyway.
 */
export function useDraftSetting(overlayIsOurs: boolean): UseDraftSettingResult {
  const [draft, setDraftState] = useState<Draft>({});

  // Read at push time, not captured. The preview can be stopped, or pre-empted
  // by a recording, mid-drag.
  const overlayIsOursRef = useRef(overlayIsOurs);
  overlayIsOursRef.current = overlayIsOurs;

  const engineRef = useRef<DraftEngine | null>(null);
  if (engineRef.current === null) {
    engineRef.current = new DraftEngine({
      paint: (theme) => {
        void commands.previewOverlayThemeDraft(theme);
      },
      commit: (key, value) => setOverlayThemeToken(key, value),
      storedTheme: persistedOverlayTheme,
      onDraftChange: setDraftState,
      canPaint: () => overlayIsOursRef.current,
    });
  }
  const engine = engineRef.current;

  const setDraft = useCallback(
    <K extends OverlayThemeKey>(key: K, value: OverlayTheme[K]) =>
      engine.set(key, value),
    [engine],
  );

  const flush = useCallback(
    (key: OverlayThemeKey) => engine.flush(key),
    [engine],
  );

  const flushAll = useCallback(() => engine.flushAll(), [engine]);

  const reset = useCallback(
    (key: OverlayThemeKey) => engine.reset(key),
    [engine],
  );

  // Switching away from the tab mid-drag must not silently drop the edit, so
  // flush what is pending rather than lose it with the timer. `engine` is a
  // stable ref for the hook's lifetime, so this runs once, on unmount.
  useEffect(() => {
    return () => {
      void engine.dispose();
    };
  }, [engine]);

  return { draft, setDraft, flush, flushAll, reset };
}
