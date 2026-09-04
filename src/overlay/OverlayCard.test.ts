import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import i18n from "i18next";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { initReactI18next } from "react-i18next";
import OverlayCard, { type OverlayCardProps } from "./OverlayCard";

/**
 * What the two visibility tokens do to the card's markup.
 *
 * Rendered to static markup rather than into a DOM, which this repository has
 * no test renderer for. The card's own effects (the live-text scroll pin) do
 * not run, and nothing here depends on them: the question is which elements
 * and which class names the row is built from.
 */

// Without an instance `useTranslation` warns on every render. The labels are
// not what this file is about, so the keys come back as themselves.
void i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: {} } } });

// `useLayoutEffect` cannot run in a static render and React says so once per
// component. The warning is about hydration, which nothing here does. Patched
// for this file's own run and put back after it, so no later test file inherits
// a filtered console.
const passThrough = console.error;
beforeAll(() => {
  console.error = (...args: unknown[]) => {
    if (typeof args[0] === "string" && args[0].includes("useLayoutEffect"))
      return;
    passThrough(...(args as []));
  };
});
afterAll(() => {
  console.error = passThrough;
});

const BASE: OverlayCardProps = {
  state: "recording",
  captureReady: true,
  levels: Array(9).fill(0.5),
  streamText: { committed: "", tentative: "" },
  phase: "listening",
  workKind: "transcribing",
  elapsed: 12,
  position: "bottom",
  session: 0,
  direction: "ltr",
};

const render = (props: Partial<OverlayCardProps>) =>
  renderToStaticMarkup(React.createElement(OverlayCard, { ...BASE, ...props }));

/** The `class` of the `.scard` element, split into its class names. */
const cardClasses = (html: string): string[] =>
  (/<div class="(scard[^"]*)"/.exec(html)?.[1] ?? "")
    .split(/\s+/)
    .filter(Boolean);

describe("the resting Minimal pill", () => {
  test("shows the waveform, the dot and the cancel button while unset", () => {
    const html = render({});
    expect(html).toContain('class="swave ready"');
    expect(html).toContain('class="sdot ready"');
    expect(html).toContain('class="sx"');
    expect(cardClasses(html)).toEqual(["scard", "compact"]);
  });

  test("hiding the waveform drops it and shrinks the pill", () => {
    const html = render({ showWaveform: false });
    expect(html).not.toContain("swave");
    // The dot and the cancel button stay; only the centre column empties.
    expect(html).toContain('class="sdot ready"');
    expect(html).toContain('class="sx"');
    expect(cardClasses(html)).toEqual(["scard", "compact", "nowave"]);
    // Still three grid children, so the row's centring is untouched.
    expect(rowChildren(html)).toBe(3);
  });

  test("hiding the cancel button drops it and nothing else", () => {
    const html = render({ showCancel: false });
    expect(html).not.toContain('class="sx"');
    expect(html).toContain('class="swave ready"');
    expect(html).toContain('class="sdot ready"');
    // The width rule is the waveform's, so the pill keeps its tuned width.
    expect(cardClasses(html)).toEqual(["scard", "compact"]);
    expect(rowChildren(html)).toBe(3);
  });

  test("hiding both leaves the dot alone on the row", () => {
    const html = render({ showWaveform: false, showCancel: false });
    expect(html).not.toContain("swave");
    expect(html).not.toContain('class="sx"');
    expect(html).toContain('class="sdot ready"');
    expect(cardClasses(html)).toEqual(["scard", "compact", "nowave"]);
    expect(rowChildren(html)).toBe(3);
  });
});

/** How many direct children the control row has. Three, always: the grid is
 *  what centres the waveform and the working label. */
function rowChildren(html: string): number {
  const row = /<div class="sbase">(.*)$/s.exec(html)?.[1] ?? "";
  let depth = 0;
  let children = 0;
  for (const tag of row.matchAll(/<(\/?)(div|span|button)\b/g)) {
    if (tag[1]) {
      depth -= 1;
      if (depth < 0) break;
    } else {
      if (depth === 0) children += 1;
      depth += 1;
    }
  }
  return children;
}

describe("the working pill", () => {
  test("never shows a waveform, and keeps its width whatever is hidden", () => {
    for (const showWaveform of [true, false]) {
      const html = render({ state: "transcribing", showWaveform });
      expect(html).not.toContain("swave");
      expect(html).toContain('class="sspinner"');
      expect(html).toContain('class="swork-label"');
      // `nowave` would shrink it past the width its label is tuned to, so the
      // working pill must never carry it.
      expect(cardClasses(html)).toEqual(["scard", "compact", "cworking"]);
      expect(rowChildren(html)).toBe(3);
    }
  });

  test("loses only the cancel button", () => {
    const html = render({ state: "transcribing", showCancel: false });
    expect(html).not.toContain('class="sx"');
    expect(html).toContain('class="sspinner"');
    expect(rowChildren(html)).toBe(3);
  });
});

describe("the Live card", () => {
  const live: Partial<OverlayCardProps> = { state: "streaming" };

  test("the resting pill shrinks with the waveform, and only it", () => {
    expect(cardClasses(render(live))).toEqual(["scard"]);
    expect(cardClasses(render({ ...live, showWaveform: false }))).toEqual([
      "scard",
      "nowave",
    ]);
  });

  test("the open panel keeps its width and its timer", () => {
    const open = {
      ...live,
      streamText: { committed: "hello", tentative: "" },
      showWaveform: false,
    };
    const html = render(open);
    // Open, so no `nowave`: the panel is tuned to the transcript, and every
    // morph out of the shrunken pill has to stay a grow.
    expect(cardClasses(html)).toEqual(["scard", "open"]);
    expect(html).toContain('class="stimer"');
    expect(html).not.toContain("swave");
  });

  test("the collapsed working pill keeps its width too", () => {
    const html = render({
      ...live,
      phase: "working",
      showWaveform: false,
      showCancel: false,
    });
    expect(cardClasses(html)).toEqual(["scard", "working"]);
    expect(html).toContain('class="sspinner"');
    expect(html).not.toContain('class="sx"');
  });
});
