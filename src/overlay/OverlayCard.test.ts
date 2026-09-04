import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import i18n from "i18next";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { initReactI18next } from "react-i18next";
import OverlayCard, { type OverlayCardProps } from "./OverlayCard";
import {
  drawnWaveformStyle,
  isCanvasWaveformStyle,
} from "./waveform/waveformStyles";

/**
 * What the two visibility tokens and the waveform style do to the card's
 * markup.
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

  test("hiding the cancel button shrinks the pill to the row that is left", () => {
    const html = render({ showCancel: false });
    expect(html).not.toContain('class="sx"');
    expect(html).toContain('class="swave ready"');
    expect(html).toContain('class="sdot ready"');
    // `nocancel` is the width rule and the two-track row: the stylesheet hides
    // the empty right column, so no space is left where the button was.
    expect(cardClasses(html)).toEqual(["scard", "compact", "nocancel"]);
    // The markup is untouched; the column goes in CSS, because the open Live
    // panel still puts its timer in one.
    expect(rowChildren(html)).toBe(3);
    expect(html).toContain('class="sbase-r"');
  });

  test("hiding both leaves the dot alone in a square pill", () => {
    const html = render({ showWaveform: false, showCancel: false });
    expect(html).not.toContain("swave");
    expect(html).not.toContain('class="sx"');
    expect(html).toContain('class="sdot ready"');
    // The pair is what makes the pill a square as wide as the row is tall,
    // with the dot centred in it. Without `nocancel` the row keeps its three
    // tracks and the dot sits one padding in, with the whole empty right
    // column beside it.
    expect(cardClasses(html)).toEqual([
      "scard",
      "compact",
      "nowave",
      "nocancel",
    ]);
    expect(rowChildren(html)).toBe(3);
  });

  test("never carries the timer, whatever is hidden", () => {
    // What a resting pill shrinks to is the dot and the cancel button, the row
    // `--ov-bare-w` and `overlay_geometry.rs` both add up. A timer on that row
    // would be width neither of them knows about.
    for (const showWaveform of [true, false]) {
      for (const showCancel of [true, false]) {
        expect(render({ showWaveform, showCancel })).not.toContain("stimer");
      }
    }
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

describe("the waveform style", () => {
  // The canvas needs the levels ref the overlay owns; a static render has no
  // ref to give it, so the tests that want a canvas pass one.
  const levelsRef = { current: Array(16).fill(0) as number[] };

  test("bars keeps the nine capsules and adds no canvas", () => {
    const html = render({ waveformStyle: "bars", levelsRef });
    expect(html).not.toContain("canvas");
    // Nine `<i>` children, exactly today's markup.
    expect(html.split("<i ").length - 1).toBe(BASE.levels.length);
  });

  test("every other style is one canvas in the same lane", () => {
    for (const style of [
      "ribbon",
      "bloom",
      "motes",
      "matrix",
      "steps",
    ] as const) {
      const html = render({ waveformStyle: style, levelsRef });
      expect(html).toContain('class="swave-canvas"');
      expect(html).toContain('class="swave-probes"');
      // The lane itself is unchanged, so the card's footprint cannot move.
      expect(html).toContain('class="swave ready"');
      expect(html).not.toContain("<i ");
    }
  });

  test("without the levels ref a canvas style falls back to the bars", () => {
    const html = render({ waveformStyle: "motes" });
    expect(html).not.toContain("canvas");
    expect(html.split("<i ").length - 1).toBe(BASE.levels.length);
  });

  test("a browser with no 2D context falls back to the bars, levels and all", () => {
    // What the overlay passes down once the card has reported the failure.
    // The bars are DOM elements fed from React state, and that state is
    // skipped while a canvas is drawing, so the fallback has to leave the
    // canvas path entirely or the bars sit frozen at zero.
    const drawn = drawnWaveformStyle("motes", true);
    expect(drawn).toBe("bars");
    expect(isCanvasWaveformStyle(drawn)).toBe(false);
    const html = render({ waveformStyle: drawn, levelsRef });
    expect(html).not.toContain("canvas");
    expect(html.split("<i ").length - 1).toBe(BASE.levels.length);
    // And a canvas that was had is untouched by the same rule.
    expect(drawnWaveformStyle("motes", false)).toBe("motes");
  });

  test("a hidden waveform draws neither", () => {
    const html = render({
      waveformStyle: "matrix",
      levelsRef,
      showWaveform: false,
    });
    expect(html).not.toContain("swave");
  });
});

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
    // No `nocancel`: the label is centred by three columns, and the pill is
    // tuned to hold it, so the row keeps the column the button sat in.
    expect(cardClasses(html)).toEqual(["scard", "compact", "cworking"]);
    expect(rowChildren(html)).toBe(3);
  });
});

describe("the Live card", () => {
  const live: Partial<OverlayCardProps> = { state: "streaming" };

  test("the resting pill shrinks with either element, and only it", () => {
    expect(cardClasses(render(live))).toEqual(["scard"]);
    expect(cardClasses(render({ ...live, showWaveform: false }))).toEqual([
      "scard",
      "nowave",
    ]);
    expect(cardClasses(render({ ...live, showCancel: false }))).toEqual([
      "scard",
      "nocancel",
    ]);
    expect(
      cardClasses(render({ ...live, showWaveform: false, showCancel: false })),
    ).toEqual(["scard", "nowave", "nocancel"]);
    // Closed, so no timer: the pill is as wide as the row it is left with.
    expect(render({ ...live, showWaveform: false })).not.toContain("stimer");
  });

  test("the open panel keeps its width and its timer", () => {
    const open = {
      ...live,
      streamText: { committed: "hello", tentative: "" },
      showWaveform: false,
      showCancel: false,
    };
    const html = render(open);
    // Open, so neither resting class: the panel is tuned to the transcript,
    // and every morph out of the shrunken pill has to stay a grow. Without
    // `nocancel` the stylesheet leaves the right column alone, which is where
    // the timer is.
    expect(cardClasses(html)).toEqual(["scard", "open"]);
    expect(html).toContain('class="sbase-r"');
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
