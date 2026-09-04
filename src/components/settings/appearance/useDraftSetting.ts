import { useCallback, useEffect, useRef, useState } from "react";
import type { OverlayTheme } from "@/bindings";
import { INHERIT_ALL, type OverlayThemeKey } from "@/lib/overlayTheme";
import { useSettingsStore } from "@/stores/settingsStore";

export const DRAFT_DEBOUNCE_MS = 120;

/**
 * A small, framework-agnostic debounced-commit engine, factored out of the
 * `useDraftSetting` hook so its one rule — "only the latest value scheduled
 * inside a debounce window is ever committed, exactly once" — is directly
 * testable without React or a fake-timer library.
 *
 * `schedule(key, value)` resets that key's timer, so a burst of calls inside
 * one debounce window collapses to a single commit of the last value. A
 * `flush` (explicit, or the timer firing) commits immediately and resolves
 * once `onSettled` has run for that commit — but only calls `onSettled` if no
 * *newer* `schedule` has landed for the key in the meantime, tracked with a
 * per-key generation counter so a slow, late-resolving commit can never
 * clear a newer edit's draft.
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

  /**
   * Commit the pending value immediately, resolving once it has settled. A
   * no-op (resolved immediately) if nothing is pending for `key`.
   */
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

  /** Cancel a pending debounce without committing it (a reset supersedes it). */
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
 * Commit one overlay-theme token. `value: null` resets that token to inherit.
 *
 * Reads the store's current `overlay_theme` at call time — not from a
 * captured closure — so two edits in flight compose instead of one
 * clobbering the other; then sends the whole sixteen-token object through the
 * one `change_overlay_theme_setting` command, which is what keeps the
 * store's optimistic write and rollback (keyed on the single `overlay_theme`
 * `AppSettings` field) working unchanged.
 */
export async function setOverlayThemeToken<K extends keyof OverlayTheme>(
  key: K,
  value: OverlayTheme[K],
): Promise<void> {
  const store = useSettingsStore.getState();
  const current = store.settings?.overlay_theme ?? INHERIT_ALL;
  await store.updateSetting("overlay_theme", { ...current, [key]: value });
}

type Draft = Partial<OverlayTheme>;

export interface UseDraftSettingResult {
  /** Values being edited but not yet committed. Read as `draft[key] ??
   *  settings.overlay_theme[key]` — never as a substitute for the persisted
   *  value on its own. */
  draft: Draft;
  /** Update the draft immediately (so the preview follows every frame of a
   *  slider drag or a native color picker's `onInput`) and commit on a
   *  120 ms trailing debounce. */
  setDraft: <K extends OverlayThemeKey>(key: K, value: OverlayTheme[K]) => void;
  /** Commit a still-pending draft immediately. Wire to `onPointerUp` /
   *  `onFocusOut` on the control so the debounce never outlives the drag or
   *  keystroke that produced it — without this, "Show on screen" could fire
   *  before the last few milliseconds of a slider drag ever reached Rust. */
  flush: (key: OverlayThemeKey) => Promise<void>;
  /** Commit every still-pending draft immediately and wait for all of them —
   *  "Show on screen" awaits this before invoking the command, so the
   *  on-screen overlay never renders stale tokens. */
  flushAll: () => Promise<void>;
  /** Cancel any pending debounce and reset the token to inherit. */
  reset: (key: OverlayThemeKey) => void;
}

/**
 * Local draft state for the sixteen overlay-theme tokens, shared by ColorField
 * and the token sliders.
 *
 * `ui/Slider` fires `onChange` on every pixel of drag (`Slider.tsx`), and a
 * native `<input type="color">` fires `onInput` continuously while dragging
 * inside the OS picker; committing straight to `change_overlay_theme_setting`
 * on every one of those would issue a Tauri command per frame. This hook
 * holds the in-flight value for the preview to read immediately and commits
 * to the backend on a trailing debounce (`DraftDebouncer`, above).
 */
export function useDraftSetting(): UseDraftSettingResult {
  const [draft, setDraftState] = useState<Draft>({});

  const clearDraftKey = useCallback((key: OverlayThemeKey) => {
    setDraftState((current) => {
      if (!(key in current)) return current;
      const next = { ...current };
      delete next[key];
      return next;
    });
  }, []);

  // One debouncer instance for the lifetime of the hook. Commits go through
  // `setOverlayThemeToken`; the draft entry clears once a commit settles,
  // unless a newer edit has already superseded it (the generation guard).
  const debouncerRef = useRef<DraftDebouncer<
    OverlayThemeKey,
    OverlayTheme[OverlayThemeKey]
  > | null>(null);
  if (debouncerRef.current === null) {
    debouncerRef.current = new DraftDebouncer(
      (key, value) => setOverlayThemeToken(key, value),
      (key) => clearDraftKey(key),
    );
  }
  const debouncer = debouncerRef.current;

  const setDraft = useCallback(
    <K extends OverlayThemeKey>(key: K, value: OverlayTheme[K]) => {
      setDraftState((current) => ({ ...current, [key]: value }));
      debouncer.schedule(key, value);
    },
    [debouncer],
  );

  const flush = useCallback(
    (key: OverlayThemeKey) => debouncer.flush(key),
    [debouncer],
  );

  const flushAll = useCallback(() => debouncer.flushAll(), [debouncer]);

  const reset = useCallback(
    (key: OverlayThemeKey) => {
      debouncer.cancel(key);
      clearDraftKey(key);
      void setOverlayThemeToken(key, null);
    },
    [debouncer, clearDraftKey],
  );

  // Switching away from the tab mid-drag should not silently drop the edit:
  // flush whatever is still pending rather than losing it with the timer.
  // Intentionally runs once: `debouncer` is a stable ref for the hook's
  // lifetime, so this only needs to fire on unmount.
  useEffect(() => {
    return () => {
      void debouncer.flushAll();
    };
  }, [debouncer]);

  return { draft, setDraft, flush, flushAll, reset };
}
