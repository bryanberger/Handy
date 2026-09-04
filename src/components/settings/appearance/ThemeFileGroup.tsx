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
  Material,
  OverlayTheme,
  ResolvedOverlayTheme,
  ThemeFileDiagnosticCode,
} from "@/bindings";

/** The settings-window theme as the v1 document a theming tool would read.
 *  Only set tokens are emitted, so an all-inherit theme copies as
 *  `{"version": 1}`. Keys come out in the contract's own order
 *  (`INHERIT_ALL` declares it) rather than whatever order the runtime object
 *  happens to carry, so the copied document is byte-identical to the
 *  contract's examples. `OnScreenPreview`'s "Copy theme as JSON" button
 *  calls this. It lives here, beside the rest of the theme-file contract,
 *  rather than in a component file that imports CSS. */
export function themeAsJsonDocument(theme: OverlayTheme): string {
  const doc: Record<string, unknown> = { version: 1 };
  (Object.keys(INHERIT_ALL) as OverlayThemeKey[]).forEach((key) => {
    const value = theme[key];
    if (value !== null && value !== undefined) doc[key] = value;
  });
  return JSON.stringify(doc, null, 2);
}

/** The tokens the Appearance tab has a row for under one Material. It asks
 *  for every group's rows the same way the groups themselves are rendered, so
 *  a token that gains, loses or shares a row is counted without a second edit.
 *  Two of the sixteen are always absent: `glass_material`, which drives the
 *  pre-macOS-26 fallback engine and is set from the theme file alone, and
 *  whichever alpha belongs to the other Material. */
function keysWithARow(material: Material): Set<OverlayThemeKey> {
  return new Set<OverlayThemeKey>(
    OVERLAY_TOKEN_GROUPS.flatMap((group) =>
      overlayTokenFieldsFor(group, material).map((field) => field.key),
    ),
  );
}

/**
 * What the "this file sets N of M values" line counts: only the tokens the
 * tab can show as locked right now, on the Material being painted.
 *
 * Otherwise a file that sets a token with no row on screen would count as
 * owning a value the user cannot find any control for, and the total would
 * promise rows that are not there. `glass_material` has none at all, and the
 * two alphas share one slot, so fourteen of the sixteen tokens are showing at
 * any moment. The tokens are still honoured. This is a counting rule for one
 * sentence, not a filter on the file.
 */
export function lockedTokenCounts(
  ownedKeys: readonly string[],
  material: Material,
): {
  count: number;
  total: number;
} {
  const shown = keysWithARow(material);
  return {
    count: ownedKeys.filter((key) => shown.has(key as OverlayThemeKey)).length,
    total: shown.size,
  };
}

const DIAGNOSTIC_I18N_KEYS: Record<ThemeFileDiagnosticCode, string> = {
  malformed_document:
    "settings.appearance.themeFile.diagnostics.malformedDocument",
  // Reuses the dedicated "newer version" copy rather than a near-duplicate
  // string. One distinct code, one distinct message.
  unsupported_version: "settings.appearance.themeFile.newerVersion",
  unknown_key: "settings.appearance.themeFile.diagnostics.unknownKey",
  wrong_type: "settings.appearance.themeFile.diagnostics.wrongType",
  invalid_color: "settings.appearance.themeFile.diagnostics.invalidColor",
  out_of_bounds: "settings.appearance.themeFile.diagnostics.outOfBounds",
  unreadable: "settings.appearance.themeFile.diagnostics.unreadable",
};

/** The i18n key a diagnostic's stable `code` translates to; `key` (a token
 *  name, or a comma-joined list for `unknown_key`) is passed as `{{key}}`. */
export function diagnosticI18nKey(code: ThemeFileDiagnosticCode): string {
  return DIAGNOSTIC_I18N_KEYS[code];
}

/** How many diagnostics the payload's cap of 5 left out. */
export function moreDiagnosticsCount(total: number, shown: number): number {
  return Math.max(0, total - shown);
}

const MAX_SHOWN_DIAGNOSTICS = 5;

export interface ThemeFileGroupProps {
  file: ResolvedOverlayTheme["file"];
  /** The effective Material, which decides how many rows are on screen for
   *  the "sets N of M values" line. See [`lockedTokenCounts`]. */
  material: Material;
  onReload: () => void;
  isReloading: boolean;
  grouped?: boolean;
}

/**
 * The Theme File group: where the effective `overlay_theme.json` is, a
 * Reload button beside it, and a capped list of anything the reader had to
 * ignore or clamp. Each per-token lock note lives on that token's own
 * control (ColorField / Slider / MaterialSelector), not here.
 */
export const ThemeFileGroup: React.FC<ThemeFileGroupProps> = ({
  file,
  material,
  onReload,
  isReloading,
  grouped = true,
}) => {
  const { t } = useTranslation();

  // Two paths, because the app is only granted `opener:default`. That covers
  // `reveal_item_in_dir` but not `open_path`, so revealing is all the
  // frontend can do on its own. When the file exists, reveal it (Finder /
  // Explorer opens its folder with the file selected). When it does not,
  // there is no item to reveal and, more to the point, usually no folder
  // either: the path shown is `~/.config/handy/`, which most users have never
  // had a reason to create. So that case goes to Rust, which creates that one
  // folder and opens it. Under `HANDY_OVERLAY_THEME_FILE` it creates nothing
  // and opens the nearest folder that already exists, since that path is the
  // user's, not Handy's.
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
  const owned = lockedTokenCounts(file.owned_keys, material);

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
          <Button
            variant="secondary"
            size="sm"
            className={PATH_ACTION_BUTTON_CLASS}
            onClick={onReload}
            disabled={isReloading}
          >
            {t("settings.appearance.themeFile.reload")}
          </Button>
        </div>
        <p className="mt-2 text-xs text-mid-gray">
          {file.present
            ? t("settings.appearance.themeFile.active", {
                count: owned.count,
                total: owned.total,
              })
            : t("settings.appearance.themeFile.notFound")}
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
