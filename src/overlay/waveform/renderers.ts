import { drawBloom } from "./bloom";
import { drawMatrix } from "./matrix";
import { drawMotes } from "./motes";
import { drawRibbon } from "./ribbon";
import { drawSteps } from "./steps";
import type { CanvasWaveformStyle } from "./waveformStyles";
import type { WaveformDraw } from "./waveformLane";

/**
 * The one table of renderers: every canvas style, in the contract's order.
 *
 * `Record<CanvasWaveformStyle, …>` makes a style added in Rust a compile
 * error here rather than an empty lane: the generated `WaveformStyle` gains a
 * value, `CanvasWaveformStyle` gains it too, and this table stops type-checking
 * until it has a renderer.
 */
export const WAVEFORM_RENDERERS: Record<CanvasWaveformStyle, WaveformDraw> = {
  ribbon: drawRibbon,
  bloom: drawBloom,
  motes: drawMotes,
  matrix: drawMatrix,
  steps: drawSteps,
};
