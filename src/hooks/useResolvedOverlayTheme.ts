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

// Module-level singleton, mirroring settingsStore.ts. One Appearance tab at a
// time means one store, and the `resolved-overlay-theme` listener below
// registers once for the window's life, not per tab mount.
const useResolvedOverlayThemeStore = create<ResolvedOverlayThemeState>(
  (set, get) => ({
    resolved: null,
    isReloading: false,
    subscribed: false,

    // Re-reads the theme file from disk, resolves and returns the merged
    // theme (commands.reloadOverlayThemeFile), covering the "Appearance tab
    // mount" and "Reload button" rows of the theme file's reload contract.
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

    // Keeps `resolved` current between reloads. A commit from this tab, a
    // theme-file change seen at the overlay's next show, and another window's
    // Reload all send the same event.
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
 * The resolved overlay theme for the Appearance tab. Merged tokens
 * (`file ?? settings ?? inherit`), the Material rendered, whether Glass is
 * available, and the theme file's state. The overlay paints from this same
 * payload, so preview and overlay cannot disagree.
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
