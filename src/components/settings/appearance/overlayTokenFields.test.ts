import { describe, expect, test } from "bun:test";
import { INHERIT_ALL, OVERLAY_TOKEN_BOUNDS } from "@/lib/overlayTheme";
import {
  OVERLAY_TOKEN_FIELDS,
  overlayTokenFieldsFor,
} from "./overlayTokenFields";

/**
 * The descriptor table's own rules. Pure data and one filter, no rendering.
 */

const keysOf = (fields: readonly { key: string }[]) =>
  fields.map((field) => field.key);

describe("the token descriptors", () => {
  test("every token but the theme-file-only one has a row", () => {
    const rows = new Set(keysOf(OVERLAY_TOKEN_FIELDS));
    const missing = Object.keys(INHERIT_ALL).filter((key) => !rows.has(key));
    expect(missing).toEqual(["glass_material"]);
  });

  test("the table is in contract order, and every group's rows are together", () => {
    expect(keysOf(OVERLAY_TOKEN_FIELDS)).toEqual([
      "accent",
      "surface",
      "surface_opacity",
      "glass_tint",
      "text",
      "border",
      "border_opacity",
      "material",
      "glass_style",
      // Two rows for one token: a slider under Flat, a switch under Glass.
      "shadow_strength",
      "shadow_strength",
      "shadow_offset_y",
      "show_waveform",
      "show_cancel",
      "size_scale",
      "radius",
      "border_width",
      "padding",
      "element_gap",
      "waveform_gap",
      "waveform_width",
    ]);
    // Display order is table order, so each group's rows must occupy one
    // unbroken run or a group would render rows out of contract sequence.
    const groups = OVERLAY_TOKEN_FIELDS.map((field) => field.group);
    for (const group of new Set(groups)) {
      const rows = groups.flatMap((row, index) =>
        row === group ? [index] : [],
      );
      const run = rows.map((_, offset) => rows[0] + offset);
      expect(rows).toEqual(run);
    }
    expect([...new Set(groups)]).toEqual([
      "color",
      "material",
      "elements",
      "size",
    ]);
  });

  test("the shadow is a slider under Flat and a switch under Glass", () => {
    const rows = OVERLAY_TOKEN_FIELDS.filter(
      (field) => field.key === "shadow_strength",
    );
    expect(rows.map((row) => [row.kind, row.onlyUnder])).toEqual([
      ["factor", "flat"],
      ["glassShadow", "glass"],
    ]);
    // macOS places its own window shadow and takes no offset, so that row is
    // Flat's alone.
    const offset = OVERLAY_TOKEN_FIELDS.find(
      (field) => field.key === "shadow_offset_y",
    );
    expect(offset?.onlyUnder).toBe("flat");
  });

  test("the two switches sit in their own group, not among the sizes", () => {
    expect(
      keysOf(
        OVERLAY_TOKEN_FIELDS.filter((field) => field.group === "elements"),
      ),
    ).toEqual(["show_waveform", "show_cancel"]);
    for (const field of OVERLAY_TOKEN_FIELDS) {
      if (field.group === "elements") expect(field.kind).toBe("toggle");
    }
  });

  test("the two alphas sit together in the Colour group, in contract order", () => {
    const colour = keysOf(
      OVERLAY_TOKEN_FIELDS.filter((field) => field.group === "color"),
    );
    expect(colour).toEqual([
      "accent",
      "surface",
      "surface_opacity",
      "glass_tint",
      "text",
      "border",
      "border_opacity",
    ]);
  });

  test("the Glass tint carries its own labels and the contract's bounds", () => {
    const tint = OVERLAY_TOKEN_FIELDS.find(
      (field) => field.key === "glass_tint",
    );
    expect(tint?.kind).toBe("factor");
    expect(tint?.labelKey).toBe("settings.appearance.tokens.glassTint.title");
    expect(tint?.descriptionKey).toBe(
      "settings.appearance.tokens.glassTint.description",
    );
    // Bounds come from the apply layer's table, never a second copy.
    const bounds = OVERLAY_TOKEN_BOUNDS.glass_tint;
    expect(tint && "min" in tint ? tint.min : null).toBe(bounds.min);
    expect(tint && "max" in tint ? tint.max : null).toBe(bounds.max);
    expect(tint && "step" in tint ? tint.step : null).toBe(bounds.step);
  });
});

describe("overlayTokenFieldsFor", () => {
  /** The point of the split. One control for the card's alpha, and it is the
   *  one painting the Material on screen. */
  test("Flat shows the surface opacity and Glass the tint, never both", () => {
    const flat = keysOf(overlayTokenFieldsFor("color", "flat"));
    expect(flat).toContain("surface_opacity");
    expect(flat).not.toContain("glass_tint");

    const glass = keysOf(overlayTokenFieldsFor("color", "glass"));
    expect(glass).toContain("glass_tint");
    expect(glass).not.toContain("surface_opacity");
  });

  test("the tint takes the opacity's place, so the group keeps its shape", () => {
    const flat = keysOf(overlayTokenFieldsFor("color", "flat"));
    const glass = keysOf(overlayTokenFieldsFor("color", "glass"));
    expect(glass.length).toBe(flat.length);
    expect(glass.indexOf("glass_tint")).toBe(flat.indexOf("surface_opacity"));
  });

  test("the Material group swaps the shadow control and drops the offset", () => {
    expect(keysOf(overlayTokenFieldsFor("material", "flat"))).toEqual([
      "material",
      "glass_style",
      "shadow_strength",
      "shadow_offset_y",
    ]);
    expect(keysOf(overlayTokenFieldsFor("material", "glass"))).toEqual([
      "material",
      "glass_style",
      "shadow_strength",
    ]);
    expect(overlayTokenFieldsFor("material", "flat")[2].kind).toBe("factor");
    expect(overlayTokenFieldsFor("material", "glass")[2].kind).toBe(
      "glassShadow",
    );
  });

  test("the Elements and Size groups are the same under both Materials", () => {
    for (const group of ["elements", "size"] as const) {
      expect(keysOf(overlayTokenFieldsFor(group, "flat"))).toEqual(
        keysOf(overlayTokenFieldsFor(group, "glass")),
      );
    }
    expect(keysOf(overlayTokenFieldsFor("size", "flat"))).toEqual([
      "size_scale",
      "radius",
      "border_width",
      "padding",
      // The gap follows the padding it is a sibling of, not the waveform.
      "element_gap",
      "waveform_gap",
      "waveform_width",
    ]);
  });

  test("a group only ever yields its own rows", () => {
    for (const material of ["flat", "glass"] as const) {
      for (const group of ["color", "material", "elements", "size"] as const) {
        for (const field of overlayTokenFieldsFor(group, material)) {
          expect(field.group).toBe(group);
        }
      }
    }
  });

  test("the four groups together are every row the Material has", () => {
    const shown = (material: "flat" | "glass") =>
      (["color", "material", "elements", "size"] as const).flatMap((group) =>
        keysOf(overlayTokenFieldsFor(group, material)),
      );

    expect(shown("flat")).toEqual([
      "accent",
      "surface",
      "surface_opacity",
      "text",
      "border",
      "border_opacity",
      "material",
      "glass_style",
      "shadow_strength",
      "shadow_offset_y",
      "show_waveform",
      "show_cancel",
      "size_scale",
      "radius",
      "border_width",
      "padding",
      "element_gap",
      "waveform_gap",
      "waveform_width",
    ]);
    expect(shown("glass")).toEqual([
      "accent",
      "surface",
      "glass_tint",
      "text",
      "border",
      "border_opacity",
      "material",
      "glass_style",
      "shadow_strength",
      "show_waveform",
      "show_cancel",
      "size_scale",
      "radius",
      "border_width",
      "padding",
      "element_gap",
      "waveform_gap",
      "waveform_width",
    ]);
    // Nineteen rows under Flat, eighteen under Glass: `glass_material` never
    // has one, the two alphas share a slot, the shadow's two rows share one,
    // and Glass has no shadow offset to show.
    expect(shown("flat").length).toBe(19);
    expect(shown("glass").length).toBe(18);
  });
});
