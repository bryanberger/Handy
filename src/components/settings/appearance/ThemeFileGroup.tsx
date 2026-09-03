import React from "react";
import { useTranslation } from "react-i18next";
import { openPath } from "@tauri-apps/plugin-opener";
import { Alert } from "@/components/ui/Alert";
import { Button } from "@/components/ui/Button";
import { PathDisplay } from "@/components/ui/PathDisplay";
import { SettingContainer } from "@/components/ui/SettingContainer";
import type {
  OverlayTheme,
  ResolvedOverlayTheme,
  ThemeFileDiagnosticCode,
} from "@/bindings";

/** The settings-window theme as the v1 document a theming tool would read:
 *  only set tokens are emitted, so an all-inherit theme copies as
 *  `{"version": 1}` (ticket 07 §3). Used by `OverlayPreview`'s "Copy theme as
 *  JSON" button; it lives here, beside the rest of the theme-file contract,
 *  rather than in a component file that imports CSS. */
export function themeAsJsonDocument(theme: OverlayTheme): string {
  const doc: Record<string, unknown> = { version: 1 };
  (Object.keys(theme) as (keyof OverlayTheme)[]).forEach((key) => {
    const value = theme[key];
    if (value !== null && value !== undefined) doc[key] = value;
  });
  return JSON.stringify(doc, null, 2);
}

const DIAGNOSTIC_I18N_KEYS: Record<ThemeFileDiagnosticCode, string> = {
  malformed_document:
    "settings.appearance.themeFile.diagnostics.malformedDocument",
  // Reuses the dedicated "newer version" copy rather than a near-duplicate
  // string — one distinct code, one distinct message.
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

/** The directory containing `filePath`. No Rust command reports whether it
 *  exists, so — like `AppDataDirectory`'s Open button — this only guards
 *  against an empty path and otherwise lets the OS report a missing folder. */
function containingDirectory(filePath: string): string {
  const trimmed = filePath.replace(/[\\/]+$/, "");
  const lastSlash = Math.max(
    trimmed.lastIndexOf("/"),
    trimmed.lastIndexOf("\\"),
  );
  return lastSlash > 0 ? trimmed.slice(0, lastSlash) : trimmed;
}

export interface ThemeFileGroupProps {
  file: ResolvedOverlayTheme["file"];
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
  onReload,
  isReloading,
  grouped = true,
}) => {
  const { t } = useTranslation();

  const handleOpen = async () => {
    if (!file.path) return;
    try {
      await openPath(containingDirectory(file.path));
    } catch (error) {
      console.error("Failed to open the theme file's directory:", error);
    }
  };

  const shown = file.diagnostics.slice(0, MAX_SHOWN_DIAGNOSTICS);
  const more = moreDiagnosticsCount(file.diagnostics_total, shown.length);

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
            onClick={onReload}
            disabled={isReloading}
          >
            {t("settings.appearance.themeFile.reload")}
          </Button>
        </div>
        <p className="mt-2 text-xs text-mid-gray">
          {file.present
            ? t("settings.appearance.themeFile.active", {
                count: file.owned_keys.length,
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
