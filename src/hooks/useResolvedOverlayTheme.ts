import { useEffect } from "react";
import { create } from "zustand";
import { commands, events } from "@/bindings";
import { INHERIT_ALL } from "@/lib/overlayTheme";
import type { OverlayTheme, ResolvedOverlayTheme } from "@/bindings";

interface ResolvedOverlayThemeState {
  resolved: ResolvedOverlayTheme | null;
  isReloading: boolean;
  isCommitting: boolean;
  subscribed: boolean;
  load: () => Promise<void>;
  commit: (theme: OverlayTheme) => Promise<void>;
  subscribe: () => void;
}

// Module-level singleton, mirroring settingsStore.ts. One Appearance tab at a
// time means one store, and the `resolved-overlay-theme` listener below
// registers once for the window's life, not per tab mount.
const useResolvedOverlayThemeStore = create<ResolvedOverlayThemeState>(
  (set, get) => ({
    resolved: null,
    isReloading: false,
    isCommitting: false,
    subscribed: false,

    // Re-reads the theme file from disk, resolves and returns it
    // (commands.reloadOverlayThemeFile). The tab calls it on mount; its Reload
    // button calls it where the watcher could not start.
    load: async () => {
      set({ isReloading: true });
      try {
        const result = await commands.reloadOverlayThemeFile();
        if (result.status === "ok") {
          set({ resolved: result.data });
        } else {
          console.error("Failed to reload overlay theme file:", result.error);
        }
      } catch (error) {
        console.error("Failed to reload overlay theme file:", error);
      } finally {
        set({ isReloading: false });
      }
    },

    // Writes the theme file, which is the overlay theme. Rust answers with the
    // document it read back, so this store holds what is on disk, not what was
    // asked for, and a clamped value corrects itself, no second round trip.
    commit: async (theme) => {
      set({ isCommitting: true });
      try {
        const result = await commands.changeOverlayThemeSetting(theme);
        if (result.status === "ok") {
          set({ resolved: result.data });
        } else {
          console.error(
            "Failed to write the overlay theme file:",
            result.error,
          );
        }
      } catch (error) {
        console.error("Failed to write the overlay theme file:", error);
      } finally {
        set({ isCommitting: false });
      }
    },

    // Keeps `resolved` current between commits. A commit from this tab, a hand
    // edit the watcher saw, a change seen at the overlay's next show and
    // another window's Reload all send this event.
    subscribe: () => {
      if (get().subscribed) return;
      set({ subscribed: true });
      events.resolvedOverlayTheme.listen((event) => {
        set({ resolved: event.payload });
      });
    },
  }),
);

/**
 * The overlay theme as persisted, read at call time rather than captured, so
 * two edits in flight compose instead of clobbering each other.
 *
 * `resolved.theme` is the theme file's own tokens, clamped: what the next
 * write starts from. Before the first payload, everything inherits.
 */
export function persistedOverlayTheme(): OverlayTheme {
  return useResolvedOverlayThemeStore.getState().resolved?.theme ?? INHERIT_ALL;
}

/** Write the overlay theme file, from outside React. */
export function commitOverlayTheme(theme: OverlayTheme): Promise<void> {
  return useResolvedOverlayThemeStore.getState().commit(theme);
}

/**
 * The resolved overlay theme for the Appearance tab: the file's tokens
 * clamped, the Material rendered, whether Glass is available, and the file's
 * state, where it is, whether Handy writes it, what the reader ignored. The
 * overlay paints from it too, so preview and overlay cannot disagree.
 */
export function useResolvedOverlayTheme() {
  const { resolved, isReloading, isCommitting, load, subscribe } =
    useResolvedOverlayThemeStore();

  useEffect(() => {
    subscribe();
    load();
  }, [load, subscribe]);

  return { resolved, isReloading, isCommitting, reload: load };
}
