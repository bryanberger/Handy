import { describe, expect, test } from "bun:test";
import { cardFootprint, computeFit, type CardBaseMetrics } from "./fitScale";

/**
 * Unit tests for the preview's fit-to-stage math (orchestrator reconciliation
 * to ticket 03 §4). `cardFootprint`'s base numbers are the contract's card
 * dimensions at scale 1 (ticket 02 §7 / 06 §2): compact 216×40 content
 * (`--ov-work-w` / `--ov-base-h`), Live open 392×(40+64+12) content
 * (`--ov-open-w` / `--ov-base-h` + `--ov-cap-max-h` + `--ov-cap-pad-y`), each
 * plus a scaled 1px border per side.
 */

const BASE: CardBaseMetrics = {
  openW: 392,
  workW: 216,
  baseH: 40,
  capMaxH: 64,
  capPadY: 12,
};

describe("cardFootprint", () => {
  test("Live at scale 1 matches the contract's 394x118 footprint", () => {
    const { width, height } = cardFootprint("live", 1, BASE);
    expect(width).toBe(394);
    expect(height).toBe(118);
  });

  test("Minimal (compact) at scale 1 matches the contract's 218x42 footprint", () => {
    const { width, height } = cardFootprint("minimal", 1, BASE);
    expect(width).toBe(218);
    expect(height).toBe(42);
  });

  test("Live at scale 1.5 matches 591x177 (02 §7's worked bound)", () => {
    const { width, height } = cardFootprint("live", 1.5, BASE);
    expect(width).toBeCloseTo(591, 5);
    expect(height).toBeCloseTo(177, 5);
  });

  test("the border scales with the card, not a flat 2px", () => {
    // 216 * 0.5 + (2 * 0.5) = 109, not 216 * 0.5 + 2 = 110; likewise
    // 40 * 0.5 + 1 = 21, not 22. A flat 2 px border would give the larger
    // pair, and the preview would then shrink a card that already fits.
    const { width, height } = cardFootprint("minimal", 0.5, BASE);
    expect(width).toBe(109);
    expect(height).toBe(21);
  });
});

describe("computeFit", () => {
  test("is 1 when the card already fits the stage", () => {
    expect(computeFit(456, 148, 394, 118)).toBe(1);
  });

  test("shrinks to the tighter of width/height when the card overflows", () => {
    // A 1.5x Live card (591x177) in a 456x148 stage: width needs 0.7716,
    // height only 0.8362, so width is the binding axis.
    expect(computeFit(456, 148, 591, 177)).toBeCloseTo(0.7715736, 6);
  });

  test("takes the height axis when that is the tighter one", () => {
    // The same card in a stage that is wide but short: 300/177 = 1.695 would
    // upscale, so height's 0.5650 binds instead.
    expect(computeFit(900, 100, 591, 177)).toBeCloseTo(0.564972, 6);
  });

  test("never upscales past 1 for a small card in a big stage", () => {
    expect(computeFit(1000, 500, 218, 42)).toBe(1);
  });

  test("falls back to 1 for non-positive input instead of NaN or Infinity", () => {
    expect(computeFit(0, 148, 394, 118)).toBe(1);
    expect(computeFit(456, 148, 0, 118)).toBe(1);
    expect(computeFit(456, 148, 394, 0)).toBe(1);
  });
});
