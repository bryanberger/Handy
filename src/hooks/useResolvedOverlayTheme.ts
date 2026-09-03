import { useEffect } from "react";
import { create } from "zustand";
import { commands, events } from "@/bindings";
import type { ResolvedOverlayTheme } from "@/bindings";

interface ResolvedOverlayThemeState {
  resolved: ResolvedOverlayTheme | null;
  isReloading: boolean;
  subscribed: boolean;
  load: () => Promise<void>;
  subscribe: () => void;
}

// Module-level singleton (mirrors settingsStore.ts): the settings window has
// exactly one Appearance tab at a time, so one store is all that's needed, and
// the `resolved-overlay-theme` listener below is registered once for the life
// of the window rather than per mount/unmount of the tab.
const useResolvedOverlayThemeStore = create<ResolvedOverlayThemeState>(
  (set, get) => ({
    resolved: null,
    isReloading: false,
    subscribed: false,

    // Re-reads the theme file from disk, resolves and returns the merged
    // theme (commands.reloadOverlayThemeFile) — the "Appearance tab mount"
    // and "Reload button" rows of the theme file's reload contract.
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

    // Keeps `resolved` current between reloads: a token committed from this
    // tab, a theme-file change picked up by the overlay's next show, or
    // another window's Reload all deliver the same event.
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
 * The resolved overlay theme for the Appearance tab: the merged tokens
 * (`file ?? settings ?? inherit`), the Material actually rendered, whether
 * Glass is available, and the theme file's state — the same payload the
 * overlay window itself paints from, so the preview and the on-screen overlay
 * can never disagree.
 */
export function useResolvedOverlayTheme() {
  const { resolved, isReloading, load, subscribe } =
    useResolvedOverlayThemeStore();

  useEffect(() => {
    subscribe();
    load();
  }, [load, subscribe]);

  return { resolved, isReloading, reload: load };
}
