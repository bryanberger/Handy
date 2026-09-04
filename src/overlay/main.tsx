import React from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import RecordingOverlay from "./RecordingOverlay";
import {
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "@/lib/utils/theme";
import { applyOverlayTheme, getStoredOverlayTheme } from "@/lib/overlayTheme";
import type { Theme } from "@/bindings";
import "@/i18n";

// A separate webview from the settings window, so the overlay has to set
// `data-theme` itself. Apply the last-known theme (shared localStorage)
// before render to avoid a flash, reconcile with the persisted setting in case
// the overlay booted first, then follow live changes.
applyTheme(getStoredTheme());
syncThemeFromSettings();
listen<Theme>("theme-changed", (event) => applyTheme(event.payload));

// Same reasoning for the overlay theme, minus the flash. The card renders
// nothing until shown. This makes the show handler's pull failure-tolerant.
// If it throws, the root already carries the last-known theme, not Handy's
// built-in pink, and the show handler and `resolved-overlay-theme` reconcile it.
applyOverlayTheme(document.documentElement, getStoredOverlayTheme());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RecordingOverlay />
  </React.StrictMode>,
);
