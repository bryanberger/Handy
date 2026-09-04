//! The frontend files this crate pins constants against, read at compile time,
//! and the small parsers that read a number out of them.
//!
//! Several Rust constants copy a frontend value: card geometry mirrors
//! `RecordingOverlay.css`, the inherit surface the app palette, token bounds
//! the apply layer's table. Each pin lives in a test beside its constants, and
//! all need the same "find the declaration, take the number" readers, kept
//! here rather than copied three times.
//!
//! Test-only. Nothing in a shipping build reads a stylesheet.

/// The overlay's stylesheet: every `--ov-*` length and timing the native
/// window is sized and animated from.
pub(crate) const OVERLAY_CSS: &str = include_str!("../../src/overlay/RecordingOverlay.css");
/// The overlay component: geometry inputs in TypeScript, not the stylesheet.
pub(crate) const OVERLAY_TSX: &str = include_str!("../../src/overlay/RecordingOverlay.tsx");
/// The app palette the overlay's inherited colours follow.
pub(crate) const THEME_CSS: &str = include_str!("../../src/styles/theme.css");
/// The apply layer, the token contract as TypeScript sees it. It carries the
/// bounds every slider is drawn from and every value re-validated against.
pub(crate) const APPLY_LAYER_TS: &str = include_str!("../../src/lib/overlayTheme.ts");
/// The waveform styles' own table: which of the two waveform lengths each
/// style reads, and so which rows the Appearance tab shows for it.
pub(crate) const WAVEFORM_STYLES_TS: &str =
    include_str!("../../src/overlay/waveform/waveformStyles.ts");

/// The number a declaration is written with, in the unit it carries, the
/// digits immediately before the first `unit` after `<name>:`. The needle
/// carries the colon, so a `var(--ov-work-w)` usage never matches. Taking the
/// digits from the right sees through a `calc(` or `minmax(` wrapper.
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

/// The text a declaration is written with, verbatim, everything between
/// `<name>:` and its semicolon. When a value is not a number at all, say a
/// `calc()` of two other custom properties, the text itself is worth pinning.
pub(crate) fn css_declaration<'a>(css: &'a str, name: &str) -> &'a str {
    let start = declaration_start(css, name).unwrap_or_else(|| panic!("{name} is not declared"));
    let rest = &css[start + name.len() + 1..];
    let end = rest
        .find(';')
        .unwrap_or_else(|| panic!("{name} is unclosed"));
    rest[..end].trim()
}

/// Where `name`'s own declaration starts, the first `<name>:` that begins a
/// declaration rather than ending another property's name.
///
/// A declaration begins at the start of the text, or after the `{`, `;` or `}`
/// that closed the one before, or after a comment, with only whitespace
/// between. Otherwise `height` would silently read `min-height`'s value.
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

/// One rule's body, so a declaration comes from the rule that carries it, not
/// the first match anywhere. `selector` includes the brace, as in `".swave {"`.
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

/// The number a TypeScript `const NAME = <number>;` declares. `declaration`
/// runs up to and including the `= `, so it names the constant unambiguously.
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
/// Anchored on the `= {`, not the first brace, because a type often carries
/// its own block (`Record<K, { min: number }>`) that is not the value.
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

/// The body of a `key: { … }` entry. `body` is a block this module returned.
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
