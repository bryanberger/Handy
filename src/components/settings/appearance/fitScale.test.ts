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
    const atOne = cardFootprint("minimal", 1, BASE);
    const atHalf = cardFootprint("minimal", 0.5, BASE);
    // Content halves exactly; the border (2 * scale) halves too, so the
    // footprint is not simply half of the scale-1 footprint plus a constant.
    expect(atHalf.width).toBeCloseTo(atOne.width / 2, 5);
    expect(atHalf.height).toBeCloseTo(atOne.height / 2, 5);
  });
});

describe("computeFit", () => {
  test("is 1 when the card already fits the stage", () => {
    expect(computeFit(456, 148, 394, 118)).toBe(1);
  });

  test("shrinks to the tighter of width/height when the card overflows", () => {
    // A 1.5x Live card (591x177) in a 456x148 stage.
    const fit = computeFit(456, 148, 591, 177);
    expect(fit).toBeCloseTo(Math.min(456 / 591, 148 / 177), 10);
    expect(fit).toBeLessThan(1);
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
