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

  test("every other row is shown under both Materials", () => {
    for (const group of ["color", "material", "size"] as const) {
      const flat = keysOf(overlayTokenFieldsFor(group, "flat"));
      const glass = keysOf(overlayTokenFieldsFor(group, "glass"));
      const shared = flat.filter((key) => glass.includes(key));
      expect(shared).toEqual(flat.filter((key) => key !== "surface_opacity"));
      // The Material group is untouched by the rule.
      if (group === "material") expect(flat).toEqual(glass);
    }
  });

  test("a group only ever yields its own rows", () => {
    for (const material of ["flat", "glass"] as const) {
      for (const group of ["color", "material", "size"] as const) {
        for (const field of overlayTokenFieldsFor(group, material)) {
          expect(field.group).toBe(group);
        }
      }
    }
  });

  test("the three groups together are the whole table, on both Materials", () => {
    for (const material of ["flat", "glass"] as const) {
      const shown = (["color", "material", "size"] as const).flatMap((group) =>
        keysOf(overlayTokenFieldsFor(group, material)),
      );
      const hiddenAlpha =
        material === "glass" ? "surface_opacity" : "glass_tint";
      expect(shown).toEqual(
        keysOf(OVERLAY_TOKEN_FIELDS).filter((key) => key !== hiddenAlpha),
      );
    }
  });
});
