import React, {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { useTranslation } from "react-i18next";
import { Pause, Play } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { commands } from "@/bindings";
import type { OverlayTheme, ResolvedOverlayTheme } from "@/bindings";
import type { OverlayColorKey } from "@/lib/overlayTheme";
import OverlayCard from "@/overlay/OverlayCard";
import { cardFootprint, computeFit, type CardBaseMetrics } from "./fitScale";
import { OverlayThemeReset } from "./OverlayThemeReset";
import { themeAsJsonDocument } from "./ThemeFileGroup";
import {
  useOverlayPreviewDriver,
  type OverlayPreviewStyle,
  type PreviewStateName,
} from "./useOverlayPreviewDriver";
import "./OverlayPreview.css";

const PROBE_STYLE = (cssVar: string): CSSProperties => ({
  position: "absolute",
  width: 0,
  height: 0,
  overflow: "hidden",
  color: `var(${cssVar})`,
});

export interface OverlayPreviewProps {
  style: OverlayPreviewStyle;
  position: "top" | "bottom";
  direction: "ltr" | "rtl";
  previewTheme: ResolvedOverlayTheme;
  /** The settings-window theme on its own — what "Copy theme as JSON" puts on
   *  the clipboard, which is deliberately *not* `previewTheme`: that one has
   *  the theme file's own values folded in, and copying those back out would
   *  hand a tool author a document echoing its own input. */
  settingsTheme: OverlayTheme;
  previewVars: CSSProperties;
  colorProbeRefs: Record<OverlayColorKey, React.RefObject<HTMLSpanElement>>;
  resetDisabled: boolean;
  hasThemeFileOwnership: boolean;
  onResetConfirm: () => void;
  /** Awaited before "Show on screen" invokes the command, so the on-screen
   *  overlay never renders tokens a pending debounce hasn't sent yet. */
  onFlushDrafts: () => Promise<void>;
}

/**
 * The preview card: a chip row that doubles as a scrubber, the play/pause and
 * "Show on screen" controls, the in-page stage the overlay is drawn on, and
 * the footer actions (whole-theme reset, copy-as-JSON).
 */
export const OverlayPreview: React.FC<OverlayPreviewProps> = ({
  style,
  position,
  direction,
  previewTheme,
  settingsTheme,
  previewVars,
  colorProbeRefs,
  resetDisabled,
  hasThemeFileOwnership,
  onResetConfirm,
  onFlushDrafts,
}) => {
  const { t } = useTranslation();
  const sampleText = t("settings.appearance.preview.sampleText");
  const driver = useOverlayPreviewDriver(style, sampleText);
  const effectiveMaterial = previewTheme.effective_material;

  const stageRef = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState(1);
  // Mirrors `fit` so the measurement below can skip an unchanged update
  // without listing `fit` itself as a dependency (which would give
  // `recomputeFit` — and so the layout effect and the ResizeObserver — a new
  // identity on every fit change).
  const fitRef = useRef(1);

  const recomputeFit = useCallback(() => {
    const stageEl = stageRef.current;
    if (!stageEl) return;
    const computed = getComputedStyle(stageEl);
    const readPx = (name: string) =>
      parseFloat(computed.getPropertyValue(name)) || 0;
    const scale = readPx("--ov-scale") || 1;
    const base: CardBaseMetrics = {
      openW: readPx("--ov-open-w"),
      workW: readPx("--ov-work-w"),
      baseH: readPx("--ov-base-h"),
      capMaxH: readPx("--ov-cap-max-h"),
      capPadY: readPx("--ov-cap-pad-y"),
    };
    const footprint = cardFootprint(style, scale, base);
    const rect = stageEl.getBoundingClientRect();
    const next = computeFit(
      rect.width,
      rect.height,
      footprint.width,
      footprint.height,
    );
    // Skipping an unchanged update matters here, not just for tidiness: this
    // runs from a layout effect, and an update scheduled during a commit
    // counts towards React's nested-update limit even when it changes nothing.
    if (next === fitRef.current) return;
    fitRef.current = next;
    setFit(next);
    // `previewVars` is not read directly; its reference changes exactly when a
    // custom property does (`useStableMap` in `useOverlayThemeVars`), which is
    // the "token or scale changed" signal this needs to re-measure on.
  }, [style, previewVars]);

  useLayoutEffect(() => {
    recomputeFit();
  }, [recomputeFit]);

  useEffect(() => {
    const stageEl = stageRef.current;
    if (!stageEl || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => recomputeFit());
    observer.observe(stageEl);
    return () => observer.disconnect();
  }, [recomputeFit]);

  const [isRecording, setIsRecording] = useState(false);
  useEffect(() => {
    let cancelled = false;
    const poll = () => {
      commands
        .isRecording()
        .then((recording) => {
          if (!cancelled) setIsRecording(recording);
        })
        .catch(() => {
          // Leave the last known state; the button's own attempt surfaces
          // any real failure when clicked.
        });
    };
    poll();
    const id = setInterval(poll, 1500);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  const [showBusy, setShowBusy] = useState(false);
  const handleShowOnScreen = async () => {
    setShowBusy(true);
    try {
      await onFlushDrafts();
      const result = await commands.previewOverlayOnScreen(sampleText);
      if (result.status === "error") {
        console.error(
          "Failed to show the overlay preview on screen:",
          result.error,
        );
      }
    } catch (error) {
      console.error("Failed to show the overlay preview on screen:", error);
    } finally {
      setShowBusy(false);
    }
  };

  const [copied, setCopied] = useState(false);
  // Cleared on unmount (and before a second copy restarts it) so the timer
  // can never call setState on a tab the user has already navigated away from.
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (copiedTimerRef.current !== null) clearTimeout(copiedTimerRef.current);
    },
    [],
  );
  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(themeAsJsonDocument(settingsTheme));
      setCopied(true);
      if (copiedTimerRef.current !== null) clearTimeout(copiedTimerRef.current);
      copiedTimerRef.current = setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error("Failed to copy the overlay theme as JSON:", error);
    }
  };

  // Both captions key off `glass_support`, never off what was requested:
  // `supported === false` is the platform fact ("macOS only"), and
  // `supported && !available` is a Mac that cannot render Glass right now.
  // The payload carries no reason for the latter, so the copy names none.
  const glassSupport = previewTheme.glass_support;
  const showGlassNote = effectiveMaterial === "glass";
  const showGlassUnsupported = !glassSupport.supported;
  const showGlassUnavailable =
    glassSupport.supported && !glassSupport.available;

  return (
    <div className="space-y-2 p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-1">
          {driver.availableStates.map((name: PreviewStateName) => (
            <button
              key={name}
              type="button"
              onClick={() => driver.pinState(name)}
              className={`rounded-full border px-2 py-0.5 text-xs transition-colors ${
                driver.activeState === name
                  ? "border-logo-primary bg-logo-primary/20"
                  : "border-transparent text-mid-gray hover:bg-mid-gray/10"
              }`}
            >
              {t(`settings.appearance.preview.states.${name}`)}
            </button>
          ))}
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={driver.togglePlay}
            aria-label={
              driver.playing
                ? t("settings.appearance.preview.pause")
                : t("settings.appearance.preview.play")
            }
            className="cursor-pointer rounded-md border border-transparent p-1.5 text-mid-gray transition-colors hover:border-logo-primary hover:bg-mid-gray/10"
          >
            {driver.playing ? (
              <Pause className="h-4 w-4" />
            ) : (
              <Play className="h-4 w-4" />
            )}
          </button>
          <Button
            variant={showGlassNote ? "primary" : "secondary"}
            size="sm"
            onClick={handleShowOnScreen}
            disabled={isRecording || showBusy}
            title={
              isRecording
                ? t("settings.appearance.preview.showOnScreenBlocked")
                : undefined
            }
          >
            {showBusy
              ? t("settings.appearance.preview.showOnScreenBusy")
              : t("settings.appearance.preview.showOnScreen")}
          </Button>
        </div>
      </div>

      <div
        className="ov-preview relative"
        data-material={effectiveMaterial}
        style={previewVars}
        dir={direction}
      >
        <span
          aria-hidden="true"
          style={PROBE_STYLE("--s-accent")}
          ref={colorProbeRefs.accent}
        />
        <span
          aria-hidden="true"
          style={PROBE_STYLE("--s-surface")}
          ref={colorProbeRefs.surface}
        />
        <span
          aria-hidden="true"
          style={PROBE_STYLE("--s-text")}
          ref={colorProbeRefs.text}
        />
        <span
          aria-hidden="true"
          style={PROBE_STYLE("--s-border")}
          ref={colorProbeRefs.border}
        />
        <div
          ref={stageRef}
          className="ov-preview-stage relative h-[148px] w-full overflow-hidden rounded-lg"
          data-material={effectiveMaterial}
        >
          {/* A hairline at the screen edge the overlay is anchored to. The card
              itself is centred in the stage (see OverlayPreview.css), so this is
              the only thing left saying "Top" or "Bottom" for the compact pill;
              it is kept deliberately faint so it reads as an annotation rather
              than as an edge the card should be resting on. */}
          <div
            aria-hidden="true"
            className={`pointer-events-none absolute inset-x-0 border-mid-gray/20 ${
              position === "top" ? "top-0 border-t" : "bottom-0 border-b"
            }`}
          />
          <div
            className="h-full w-full"
            // The card is centred, so the shrink pulls towards the middle
            // rather than towards one edge.
            style={
              fit < 1
                ? { transform: `scale(${fit})`, transformOrigin: "50% 50%" }
                : undefined
            }
          >
            {driver.mounted && (
              <OverlayCard
                state={driver.cardProps.state}
                captureReady={driver.cardProps.captureReady}
                levels={driver.cardProps.levels}
                streamText={driver.cardProps.streamText}
                phase={driver.cardProps.phase}
                workKind={driver.cardProps.workKind}
                elapsed={driver.cardProps.elapsed}
                position={position}
                session={driver.session}
                direction={direction}
                inert
              />
            )}
          </div>
        </div>
      </div>

      {fit < 1 && (
        <p className="text-xs text-mid-gray">
          {t("settings.appearance.preview.scaledNote", {
            percent: Math.round(fit * 100),
          })}
        </p>
      )}
      {showGlassNote && (
        <p className="text-xs text-mid-gray">
          {t("settings.appearance.preview.glassNote")}
        </p>
      )}
      {showGlassUnsupported && (
        <p className="text-xs text-mid-gray">
          {t("settings.appearance.preview.glassUnavailable")}
        </p>
      )}
      {showGlassUnavailable && (
        <p className="text-xs text-mid-gray">
          {t("settings.appearance.material.unavailableNote")}
        </p>
      )}

      <div className="flex items-center gap-2 pt-1">
        <OverlayThemeReset
          disabled={resetDisabled}
          hasThemeFileOwnership={hasThemeFileOwnership}
          onConfirm={onResetConfirm}
        />
        <Button variant="ghost" size="sm" onClick={handleCopy}>
          {copied
            ? t("settings.appearance.themeFile.copied")
            : t("settings.appearance.themeFile.copy")}
        </Button>
      </div>
    </div>
  );
};
