import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import ResetIcon from "@/components/icons/ResetIcon";

export interface OverlayThemeResetProps {
  /** Disabled once every overlay-theme token already equals the default
   *  (every token inherits). */
  disabled: boolean;
  /** True when the theme file owns at least one token — the confirmation
   *  wording changes because those values survive the reset. */
  hasThemeFileOwnership: boolean;
  onConfirm: () => void;
}

/**
 * The whole-theme reset: a small ghost button beneath the preview stage,
 * confirmed with a dialog before it restores every overlay color, size and
 * spacing token to inherit. Overlay style and position are untouched — this
 * only resets `overlay_theme`.
 */
export const OverlayThemeReset: React.FC<OverlayThemeResetProps> = ({
  disabled,
  hasThemeFileOwnership,
  onConfirm,
}) => {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        disabled={disabled}
        onClick={() => setOpen(true)}
        className="inline-flex items-center gap-1.5"
      >
        <ResetIcon width={14} height={14} />
        {t("settings.appearance.reset.button")}
      </Button>
      <Dialog
        open={open}
        onOpenChange={setOpen}
        title={t("settings.appearance.reset.title")}
        closeLabel={t("common.close")}
        footer={
          <>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setOpen(false)}
            >
              {t("settings.appearance.reset.cancel")}
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={() => {
                onConfirm();
                setOpen(false);
              }}
            >
              {t("settings.appearance.reset.confirm")}
            </Button>
          </>
        }
      >
        <p className="text-sm text-mid-gray">
          {hasThemeFileOwnership
            ? t("settings.appearance.reset.descriptionWithThemeFile")
            : t("settings.appearance.reset.description")}
        </p>
      </Dialog>
    </>
  );
};
