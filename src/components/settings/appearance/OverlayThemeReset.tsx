import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import ResetIcon from "@/components/icons/ResetIcon";

export interface OverlayThemeResetProps {
  /** Disabled once every overlay-theme token already inherits the default, and
   *  while the theme file is managed by something other than Handy. */
  disabled: boolean;
  onConfirm: () => void;
}

/**
 * The whole-theme reset. A small ghost button in the On-Screen Preview card
 * that, after a confirming dialog, restores every overlay color, size and
 * spacing token to inherit, leaving the theme file only its version row.
 * Overlay style and position survive, being settings, not theme tokens.
 */
export const OverlayThemeReset: React.FC<OverlayThemeResetProps> = ({
  disabled,
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
          {t("settings.appearance.reset.description")}
        </p>
      </Dialog>
    </>
  );
};
