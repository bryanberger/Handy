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
// `data-theme` on its own document: last-known theme before render (shared
// localStorage) to avoid a flash, reconcile with the persisted setting in case
// the overlay booted first, then follow live changes.
applyTheme(getStoredTheme());
syncThemeFromSettings();
listen<Theme>("theme-changed", (event) => applyTheme(event.payload));

// Same reasoning for the overlay theme, with one difference: the card renders
// nothing until it is shown, so this is not about a flash. It makes the pull in
// the show handler failure-tolerant — if it throws, the root already carries the
// user's last-known theme instead of Handy's built-in pink. Reconciled by that
// handler and by `resolved-overlay-theme`.
applyOverlayTheme(document.documentElement, getStoredOverlayTheme());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RecordingOverlay />
  </React.StrictMode>,
);
