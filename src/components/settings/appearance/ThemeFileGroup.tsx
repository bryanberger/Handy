import React from "react";
import { useTranslation } from "react-i18next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Alert } from "@/components/ui/Alert";
import { Button } from "@/components/ui/Button";
import {
  PATH_ACTION_BUTTON_CLASS,
  PathDisplay,
} from "@/components/ui/PathDisplay";
import { SettingContainer } from "@/components/ui/SettingContainer";
import { INHERIT_ALL, type OverlayThemeKey } from "@/lib/overlayTheme";
import {
  OVERLAY_TOKEN_GROUPS,
  overlayTokenFieldsFor,
} from "./overlayTokenFields";
import { commands } from "@/bindings";
import type {
  ManagedReason,
  Material,
  OverlayTheme,
  ResolvedOverlayTheme,
  ThemeFileDiagnosticCode,
} from "@/bindings";

/** The settings-window theme as the v1 document a theming tool reads. Only
 *  set tokens are emitted, so an all-inherit theme copies as `{"version": 1}`.
 *  Keys come out in the contract's order (`INHERIT_ALL` declares it), not the
 *  runtime object's, so the copy is byte-identical to the contract's examples.
 *  `OnScreenPreview`'s "Copy theme as JSON" button calls this. It sits with
 *  the theme-file contract, away from CSS-importing components. */
export function themeAsJsonDocument(theme: OverlayTheme): string {
  const doc: Record<string, unknown> = { version: 1 };
  (Object.keys(INHERIT_ALL) as OverlayThemeKey[]).forEach((key) => {
    const value = theme[key];
    if (value !== null && value !== undefined) doc[key] = value;
  });
  return JSON.stringify(doc, null, 2);
}

/** The tokens with a row in the Appearance tab under one Material. It asks
 *  for rows exactly as the groups render them, so a token that gains, loses
 *  or shares a row needs no second edit. Never shown: `glass_material`, which
 *  drives only the pre-macOS-26 fallback engine and has no control, the other
 *  Material's alpha, under Glass the shadow offset macOS owns, and with the
 *  waveform hidden the whole Waveform group the tab drops with it. */
function keysWithARow(
  material: Material,
  showWaveform: boolean,
): Set<OverlayThemeKey> {
  const groups = showWaveform
    ? OVERLAY_TOKEN_GROUPS
    : OVERLAY_TOKEN_GROUPS.filter((group) => group !== "waveform");
  return new Set<OverlayThemeKey>(
    groups.flatMap((group) =>
      overlayTokenFieldsFor(group, material).map((field) => field.key),
    ),
  );
}

/**
 * What the "this file sets N of M values" line counts. Only the tokens with a
 * row on screen right now, on the Material being painted.
 *
 * Otherwise a file setting a row-less token would count as a value with no
 * control, promising rows that are not there. So the total follows what is on
 * screen: twenty of the twenty-two under Flat, nineteen under Glass, with no
 * shadow offset, and three fewer either way with the waveform hidden, whose
 * group leaves the tab. The tokens still apply; this rule counts one sentence,
 * not the file.
 */
export function setTokenCounts(
  ownedKeys: readonly string[],
  material: Material,
  showWaveform: boolean,
): {
  count: number;
  total: number;
} {
  const shown = keysWithARow(material, showWaveform);
  return {
    count: ownedKeys.filter((key) => shown.has(key as OverlayThemeKey)).length,
    total: shown.size,
  };
}

const DIAGNOSTIC_I18N_KEYS: Record<ThemeFileDiagnosticCode, string> = {
  malformed_document:
    "settings.appearance.themeFile.diagnostics.malformedDocument",
  // Reuses the dedicated "newer version" copy, not a near-duplicate string.
  // One distinct code, one distinct message.
  unsupported_version: "settings.appearance.themeFile.newerVersion",
  unknown_key: "settings.appearance.themeFile.diagnostics.unknownKey",
  wrong_type: "settings.appearance.themeFile.diagnostics.wrongType",
  invalid_color: "settings.appearance.themeFile.diagnostics.invalidColor",
  out_of_bounds: "settings.appearance.themeFile.diagnostics.outOfBounds",
  unreadable: "settings.appearance.themeFile.diagnostics.unreadable",
};

/** The i18n key a diagnostic's stable `code` maps to. `key` (a token name, or
 *  a comma-joined list for `unknown_key`) is passed as `{{key}}`. */
export function diagnosticI18nKey(code: ThemeFileDiagnosticCode): string {
  return DIAGNOSTIC_I18N_KEYS[code];
}

/** How many diagnostics the payload's cap of 5 left out. */
export function moreDiagnosticsCount(total: number, shown: number): number {
  return Math.max(0, total - shown);
}

const MANAGED_I18N_KEYS: Record<ManagedReason, string> = {
  symlink: "settings.appearance.themeFile.managed.symlink",
  read_only: "settings.appearance.themeFile.managed.readOnly",
  not_creatable: "settings.appearance.themeFile.managed.notCreatable",
  unknown: "settings.appearance.themeFile.managed.unknown",
};

/** One line of copy: the i18n key to show, and what to interpolate into it. */
export interface ThemeFileStatus {
  key: string;
  params: Record<string, string | number>;
}

/**
 * Which of the group's three states the theme file is in.
 *
 * Managed first, because it is the one that changes what the tab can do: a
 * symlinked or read-only document belongs to whoever made it, Handy reads it
 * and every token row is locked. Otherwise the file is Handy's, and the only
 * question left is whether it exists yet.
 *
 * Pure, so the three states are a unit test rather than a screenshot.
 */
export function themeFileStatus(
  file: ResolvedOverlayTheme["file"],
  counts: { count: number; total: number },
): ThemeFileStatus {
  const { writable, reason, target } = file.ownership;
  if (!writable) {
    return {
      key: MANAGED_I18N_KEYS[reason ?? "unknown"],
      params: { target: target ?? file.path },
    };
  }

  return file.present
    ? { key: "settings.appearance.themeFile.owned", params: counts }
    : { key: "settings.appearance.themeFile.notFound", params: {} };
}

const MAX_SHOWN_DIAGNOSTICS = 5;

export interface ThemeFileGroupProps {
  file: ResolvedOverlayTheme["file"];
  /** The effective Material, which sets how many rows are on screen for the
   *  "sets N of M values" line. See [`setTokenCounts`]. */
  material: Material;
  /** `show_waveform`, which takes the Waveform group off the tab with it, so
   *  its three rows leave the same count. */
  showWaveform: boolean;
  /** Whether the file watcher is running. It is, on nearly every machine, and
   *  then a hand edit arrives on its own and Reload has nothing to do. */
  watching: boolean;
  onReload: () => void;
  isReloading: boolean;
  grouped?: boolean;
}

/**
 * The Theme File group: where `overlay_theme.json` is, a button that opens it,
 * one line saying which of its three states it is in, and a capped list of
 * what the reader ignored or clamped. The Reload button appears only where the
 * watcher could not start, since everywhere else a hand edit is already live.
 */
export const ThemeFileGroup: React.FC<ThemeFileGroupProps> = ({
  file,
  material,
  showWaveform,
  watching,
  onReload,
  isReloading,
  grouped = true,
}) => {
  const { t } = useTranslation();

  // Two paths, because the app only has `opener:default`. That covers
  // `reveal_item_in_dir` but not `open_path`, so the frontend can only reveal.
  // If the file exists, reveal it (Finder / Explorer opens its folder with the
  // file selected). If not, there is nothing to reveal and usually no folder
  // either, since `~/.config/handy/` is one most users never had reason to
  // create. That case goes to Rust, which creates that one folder and opens
  // it. Under `HANDY_OVERLAY_THEME_FILE` it creates nothing and opens the
  // nearest existing folder, since that path is the user's, not Handy's.
  const handleOpen = async () => {
    if (!file.path) return;
    try {
      if (file.present) {
        await revealItemInDir(file.path);
        return;
      }
      const result = await commands.revealOverlayThemeLocation();
      if (result.status === "error") {
        console.error(
          "Failed to open the theme file's directory:",
          result.error,
        );
      }
    } catch (error) {
      console.error("Failed to show the theme file's directory:", error);
    }
  };

  const shown = file.diagnostics.slice(0, MAX_SHOWN_DIAGNOSTICS);
  const more = moreDiagnosticsCount(file.diagnostics_total, shown.length);
  const status = themeFileStatus(
    file,
    setTokenCounts(file.owned_keys, material, showWaveform),
  );

  return (
    <>
      <SettingContainer
        title={t("settings.appearance.themeFile.title")}
        description={t("settings.appearance.themeFile.description")}
        grouped={grouped}
        layout="stacked"
      >
        <div className="flex items-center gap-2">
          <div className="min-w-0 flex-1">
            <PathDisplay
              path={file.path}
              onOpen={handleOpen}
              disabled={!file.path}
            />
          </div>
          {!watching && (
            <Button
              variant="secondary"
              size="sm"
              className={PATH_ACTION_BUTTON_CLASS}
              onClick={onReload}
              disabled={isReloading}
            >
              {t("settings.appearance.themeFile.reload")}
            </Button>
          )}
        </div>
        <p className="mt-2 text-xs text-mid-gray">
          {t(status.key, status.params)}
        </p>
      </SettingContainer>
      {shown.length > 0 && (
        <div className="p-4">
          <Alert variant="warning">
            <span className="font-medium">
              {t("settings.appearance.themeFile.problemsTitle")}
            </span>
            <ul className="mt-1 list-disc space-y-0.5 pl-4">
              {shown.map((diagnostic, index) => (
                <li key={index}>
                  {t(diagnosticI18nKey(diagnostic.code), {
                    key: diagnostic.key ?? "",
                  })}
                </li>
              ))}
            </ul>
            {more > 0 && (
              <p className="mt-1">
                {t("settings.appearance.themeFile.moreDiagnostics", {
                  count: more,
                })}
              </p>
            )}
          </Alert>
        </div>
      )}
    </>
  );
};
