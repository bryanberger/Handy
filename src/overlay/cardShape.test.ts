import { describe, expect, test } from "bun:test";
import { cardShape, liveCardState } from "./cardShape";

/**
 * Unit tests for the Live card's shape derivation. It has two consumers:
 * `OverlayCard`, which renders the flags, and `cardShape`, which reports the
 * shape to the backend. The point of these tests is that both read one rule.
 */

const NO_TEXT = { committed: "", tentative: "" };

describe("liveCardState", () => {
  test("text opens the panel, working or not", () => {
    expect(liveCardState(true, false)).toEqual({
      open: true,
      collapsed: false,
    });
    expect(liveCardState(true, true)).toEqual({
      open: true,
      collapsed: false,
    });
  });

  test("working with nothing to preserve collapses to the pill", () => {
    expect(liveCardState(false, true)).toEqual({
      open: false,
      collapsed: true,
    });
  });

  test("an idle stream is neither open nor collapsed", () => {
    expect(liveCardState(false, false)).toEqual({
      open: false,
      collapsed: false,
    });
  });
});

describe("cardShape", () => {
  test("reports the shapes liveCardState implies", () => {
    expect(
      cardShape("streaming", { committed: "hi", tentative: "" }, "working"),
    ).toBe("live_open");
    expect(
      cardShape("streaming", { committed: "", tentative: "hi" }, "listening"),
    ).toBe("live_open");
    expect(cardShape("streaming", NO_TEXT, "working")).toBe("live_working");
    expect(cardShape("streaming", NO_TEXT, "listening")).toBe("live_pill");
  });

  test("the compact form has one working shape and one resting shape", () => {
    expect(cardShape("transcribing", NO_TEXT, "listening")).toBe(
      "compact_working",
    );
    expect(cardShape("processing", NO_TEXT, "listening")).toBe(
      "compact_working",
    );
    expect(cardShape("recording", NO_TEXT, "listening")).toBe("compact_rest");
  });
});
