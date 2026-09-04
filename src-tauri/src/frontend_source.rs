//! The frontend files this crate keeps constants in step with, read at compile
//! time, and the small parsers that read a number out of them.
//!
//! Several Rust constants are a second copy of a value the frontend owns: the
//! card geometry mirrors `RecordingOverlay.css`, the inherit surface mirrors the
//! app palette, the token bounds mirror the apply layer's own table. Each of
//! those pins lives in a test beside the constants it pins. They all need the
//! same handful of "find this declaration, take the number" readers, so the
//! readers live here rather than being copied into three test modules.
//!
//! Test-only. Nothing in a shipping build reads a stylesheet.

/// The overlay's stylesheet: every `--ov-*` length and timing the native
/// window is sized and animated from.
pub(crate) const OVERLAY_CSS: &str = include_str!("../../src/overlay/RecordingOverlay.css");
/// The overlay component: the geometry inputs that live in TypeScript rather
/// than in the stylesheet.
pub(crate) const OVERLAY_TSX: &str = include_str!("../../src/overlay/RecordingOverlay.tsx");
/// The app palette, which the overlay's inherited colours follow.
pub(crate) const THEME_CSS: &str = include_str!("../../src/styles/theme.css");
/// The apply layer: the token contract as TypeScript sees it. It carries the
/// bounds every slider is drawn from and every value is re-validated against.
pub(crate) const APPLY_LAYER_TS: &str = include_str!("../../src/lib/overlayTheme.ts");

/// The number a declaration is written with, in the unit it carries: the
/// digits immediately before the first `unit` after `<name>:`. The needle
/// carries the colon, so only the declaration matches and a `var(--ov-work-w)`
/// usage never does. Taking the digits from the right sees through a `calc(`
/// or a `minmax(` the value is wrapped in.
pub(crate) fn css_value(css: &str, name: &str, unit: &str) -> f64 {
    let needle = format!("{name}:");
    let start = css
        .find(&needle)
        .unwrap_or_else(|| panic!("{name} is not declared in the stylesheet"));
    let rest = &css[start + needle.len()..];
    let end = rest
        .find(unit)
        .unwrap_or_else(|| panic!("{name} is not declared in {unit}"));
    rest[..end]
        .trim_start_matches(|character: char| {
            !character.is_ascii_digit() && character != '-' && character != '.'
        })
        .parse()
        .unwrap_or_else(|_| panic!("{name} is not a number"))
}

pub(crate) fn css_px(css: &str, name: &str) -> f64 {
    css_value(css, name, "px")
}

pub(crate) fn css_ms(css: &str, name: &str) -> f64 {
    css_value(css, name, "ms")
}

/// A unitless declaration, such as `--ov-scale: 1;`.
pub(crate) fn css_number(css: &str, name: &str) -> f64 {
    css_value(css, name, ";")
}

/// The text a declaration is written with, verbatim: everything between
/// `<name>:` and its semicolon. For the values that are not a number at all —
/// a `calc()` of two other custom properties, say — the text itself is the
/// thing worth pinning.
pub(crate) fn css_declaration<'a>(css: &'a str, name: &str) -> &'a str {
    let start = declaration_start(css, name).unwrap_or_else(|| panic!("{name} is not declared"));
    let rest = &css[start + name.len() + 1..];
    let end = rest
        .find(';')
        .unwrap_or_else(|| panic!("{name} is unclosed"));
    rest[..end].trim()
}

/// Where `name`'s own declaration starts: the first `<name>:` that begins a
/// declaration rather than ending another property's name.
///
/// A declaration begins at the start of the text, or after the `{`, `;` or
/// `}` that closed the one before it, or after a comment, with only
/// whitespace in between. Matching the bare `<name>:` anywhere would let a
/// property latch onto a longer one the same rule happens to declare first:
/// `height` would read `min-height`'s value, and read it silently.
fn declaration_start(css: &str, name: &str) -> Option<usize> {
    let needle = format!("{name}:");
    let mut searched = 0;
    while let Some(offset) = css[searched..].find(&needle) {
        let at = searched + offset;
        let before = css[..at].trim_end();
        if before.is_empty() || before.ends_with(['{', ';', '}']) || before.ends_with("*/") {
            return Some(at);
        }
        searched = at + needle.len();
    }
    None
}

/// The colour a `--name: #rrggbb;` declaration is written with.
pub(crate) fn css_color(css: &str, name: &str) -> String {
    css_declaration(css, name).to_string()
}

/// One rule's body, so a declaration is read from the rule that carries it
/// rather than from the first match anywhere in the stylesheet. `selector`
/// includes the brace (`".swave {"`), which is what makes it unambiguous.
pub(crate) fn css_rule<'a>(css: &'a str, selector: &str) -> &'a str {
    let start = css
        .find(selector)
        .unwrap_or_else(|| panic!("{selector} is not a rule in the stylesheet"));
    let body = &css[start + selector.len()..];
    let end = body
        .find('}')
        .unwrap_or_else(|| panic!("{selector} is never closed"));
    &body[..end]
}

/// The number a `const NAME = <number>;` in a TypeScript source is declared
/// with. `declaration` is everything up to and including the `= `, so it names
/// the constant unambiguously.
pub(crate) fn tsx_const(tsx: &str, declaration: &str) -> f64 {
    let start = tsx
        .find(declaration)
        .unwrap_or_else(|| panic!("{declaration} is not in the TypeScript source"));
    let rest = &tsx[start + declaration.len()..];
    let end = rest
        .find(';')
        .unwrap_or_else(|| panic!("{declaration} is never terminated"));
    rest[..end]
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("{declaration} is not a number"))
}

/// The body of a `const NAME … = { … }` declaration, braces balanced, so a
/// nested object literal comes back whole.
///
/// Anchored on the `= {` rather than on the first brace after the name,
/// because a declaration's type often carries a block of its own
/// (`Record<K, { min: number }>`) and that is not the value.
pub(crate) fn ts_declaration_block<'a>(ts: &'a str, name: &str) -> &'a str {
    let start = ts
        .find(name)
        .unwrap_or_else(|| panic!("{name} is not declared in the TypeScript source"));
    let open = ts[start..]
        .find("= {")
        .unwrap_or_else(|| panic!("{name} is not assigned an object literal"))
        + start
        + 2;
    balanced_block(ts, open, name)
}

/// The body of a `key: { … }` entry of an object literal. `body` is another
/// block this module returned.
pub(crate) fn ts_entry_block<'a>(body: &'a str, key: &str) -> &'a str {
    let start = body
        .find(&format!("{key}:"))
        .unwrap_or_else(|| panic!("no {key} in this object literal"));
    let open = body[start..]
        .find('{')
        .unwrap_or_else(|| panic!("{key} is not an object literal"))
        + start;
    balanced_block(body, open, key)
}

/// The text between `open` (which must be a `{`) and its matching `}`.
fn balanced_block<'a>(ts: &'a str, open: usize, what: &str) -> &'a str {
    let mut depth = 0usize;
    for (offset, character) in ts[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &ts[open + 1..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("{what}'s block is never closed");
}

/// The number a `<key>: <number>` entry of a TypeScript object literal is
/// written with. `body` is a block this module returned.
pub(crate) fn ts_number_field(body: &str, key: &str) -> f64 {
    let needle = format!("{key}:");
    let start = body
        .find(&needle)
        .unwrap_or_else(|| panic!("no {key} in this object literal"));
    body[start + needle.len()..]
        .trim_start()
        .split([',', '\n', '}'])
        .next()
        .unwrap_or_default()
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("{key} is not a number literal"))
}
