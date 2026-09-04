import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./Button";

/** Padding for a button beside a path box. The box is `py-2` around `text-xs`
 *  inside a 1px border, so `size="sm"` alone (`py-1`) renders 8px shorter;
 *  `py-2` matches the heights. Exported so a caller adding a second button to
 *  the row matches Open instead of guessing at the number. */
export const PATH_ACTION_BUTTON_CLASS = "px-3 py-2";

interface PathDisplayProps {
  path: string;
  onOpen: () => void;
  disabled?: boolean;
}

export const PathDisplay: React.FC<PathDisplayProps> = ({
  path,
  onOpen,
  disabled = false,
}) => {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-2">
      <div className="flex-1 min-w-0 px-2 py-2 bg-mid-gray/10 border border-mid-gray/80 rounded-lg text-xs font-mono break-all select-text cursor-text">
        {path}
      </div>
      <Button
        onClick={onOpen}
        variant="secondary"
        size="sm"
        disabled={disabled}
        className={PATH_ACTION_BUTTON_CLASS}
      >
        {t("common.open")}
      </Button>
    </div>
  );
};
