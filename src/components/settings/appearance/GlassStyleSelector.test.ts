import { describe, expect, test } from "bun:test";
import type { GlassSupport } from "@/bindings";
import {
  GLASS_STYLE_OPTIONS,
  glassStyleControlState,
  glassStyleForKey,
} from "./GlassStyleSelector";
import { OVERLAY_TOKEN_FIELDS } from "./overlayTokenFields";

/**
 * The Glass style row's rule and the descriptor entry that puts it in the
 * Material group. Both are pure, so neither test renders anything.
 */

const support = (
  engine: GlassSupport["engine"],
  available = true,
): GlassSupport => ({ supported: engine !== "none", available, engine });

describe("glassStyleControlState", () => {
  test("is enabled on Liquid Glass while the Material is Glass", () => {
    expect(glassStyleControlState("glass", support("liquid"), false)).toBe(
      "enabled",
    );
  });

  test("is hidden wherever Liquid Glass is not the engine", () => {
    for (const engine of ["visual_effect", "none"] as const) {
      expect(glassStyleControlState("glass", support(engine), false)).toBe(
        "hidden",
      );
    }
  });

  test("hidden outranks everything: the row never appears off Liquid Glass", () => {
    expect(glassStyleControlState("flat", support("none"), true)).toBe(
      "hidden",
    );
  });

  test("is disabled, not hidden, while the Material is Flat", () => {
    expect(glassStyleControlState("flat", support("liquid"), false)).toBe(
      "disabled",
    );
  });

  test("is disabled while the theme file owns the token", () => {
    expect(glassStyleControlState("glass", support("liquid"), true)).toBe(
      "disabled",
    );
  });

  /** The same rule the Material row follows. A preference must survive Reduce
   *  Transparency going on and off again. */
  test("stays enabled on a Mac where Glass cannot render right now", () => {
    expect(
      glassStyleControlState("glass", support("liquid", false), false),
    ).toBe("enabled");
  });
});

describe("glassStyleForKey", () => {
  test("both arrow axes move the selection forward and back", () => {
    for (const key of ["ArrowRight", "ArrowDown"]) {
      expect(glassStyleForKey(key, "regular")).toBe("clear");
    }
    for (const key of ["ArrowLeft", "ArrowUp"]) {
      expect(glassStyleForKey(key, "clear")).toBe("regular");
    }
  });

  test("the ends wrap, the way a radiogroup's arrows do", () => {
    expect(glassStyleForKey("ArrowRight", "clear")).toBe("regular");
    expect(glassStyleForKey("ArrowLeft", "regular")).toBe("clear");
  });

  test("Space and Enter select the focused option", () => {
    for (const key of [" ", "Enter"]) {
      expect(glassStyleForKey(key, "clear")).toBe("clear");
      expect(glassStyleForKey(key, "regular")).toBe("regular");
    }
  });

  /** Anything else belongs to the browser. Tab has to leave the group, and a
   *  non-typing control must not swallow it. */
  test("every other key is left alone", () => {
    for (const key of ["Tab", "Escape", "a", "Home", "PageDown"]) {
      expect(glassStyleForKey(key, "regular")).toBeNull();
    }
  });

  test("every option answers every arrow, so no key is a dead end", () => {
    for (const option of GLASS_STYLE_OPTIONS) {
      for (const key of ["ArrowRight", "ArrowLeft", "ArrowUp", "ArrowDown"]) {
        expect(GLASS_STYLE_OPTIONS).toContain(glassStyleForKey(key, option)!);
      }
    }
  });
});

describe("the Glass style descriptor", () => {
  const field = OVERLAY_TOKEN_FIELDS.find(
    (candidate) => candidate.key === "glass_style",
  );

  test("sits in the Material group, right after Material", () => {
    expect(field === undefined).toBe(false);
    expect(field?.group).toBe("material");
    expect(field?.kind).toBe("glassStyle");

    const materialGroup = OVERLAY_TOKEN_FIELDS.filter(
      (candidate) => candidate.group === "material",
    ).map((candidate) => candidate.key);
    expect(materialGroup).toEqual(["material", "glass_style"]);
  });

  test("carries its own label and description keys", () => {
    expect(field?.labelKey).toBe("settings.appearance.glassStyle.title");
    expect(field?.descriptionKey).toBe(
      "settings.appearance.glassStyle.description",
    );
  });

  /** `glass_material` is a theme-file key only now. Its eight-option dropdown
   *  with subtitles left the tab when Liquid Glass arrived. */
  test("the Glass material has no row of its own", () => {
    // Widened to `string[]` deliberately. The key union no longer contains
    // `glass_material` at all, the stronger half of this guarantee; this only
    // keeps it true if the union ever widens.
    const keys: string[] = OVERLAY_TOKEN_FIELDS.map(
      (candidate) => candidate.key,
    );
    expect(keys).not.toContain("glass_material");
  });

  test("offers Regular first, then Clear", () => {
    expect(GLASS_STYLE_OPTIONS).toEqual(["regular", "clear"]);
  });
});
