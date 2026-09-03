# Handy

Handy is a local, offline speech-to-text desktop app: a global shortcut records the microphone, a local model transcribes it, and the text is pasted into the active application. This glossary covers the terms in play for the app's appearance and its recording overlay.

## Language

### Windows and appearance

**Settings window**:
Handy's main window, organised into sidebar tabs (General, History, Models, Advanced, About, and so on).
_Avoid_: main window, app window, dashboard

**Appearance tab**:
The settings tab that holds the app theme and the overlay theme with its preview.
_Avoid_: theme tab, customization tab, look-and-feel tab

**App theme**:
The choice of System, Light, or Dark palette for Handy's windows.
_Avoid_: appearance mode, color scheme, dark mode setting, theme (when the overlay theme is meant)

**Brand pink**:
Handy's own accent colors as used in the settings window; not user-adjustable.
_Avoid_: accent (reserved for the overlay), primary color

### Overlay

**Overlay**:
The small floating window shown on screen while Handy records, transcribes, or processes.
_Avoid_: HUD, widget, popup, indicator

**Card**:
The visible rounded rectangle the overlay draws inside its window — the pill in Minimal, the panel in Live. It is always smaller than the overlay window, which is transparent around it.
_Avoid_: overlay window, box, container, bubble

**Overlay style**:
Which overlay form is shown: None (hidden), Minimal (a compact pill), or Live (a pill that opens into a panel showing the transcript as it arrives).
_Avoid_: overlay mode, overlay type, overlay form

**Overlay position**:
Which screen edge the overlay sits at: Top or Bottom.
_Avoid_: placement, anchor, alignment

**Overlay theme**:
The complete set of tokens that determine how the overlay looks: accent, surface, text, material, size, radius, and spacing. Every token is optional, so a theme may set one value or all of them.
_Avoid_: overlay skin, overlay colors, overlay CSS, customization

**Token**:
One named, user-settable value in the overlay theme, such as the accent color or the corner radius. Every token is optional and every token can be set from the settings window or the theme file.
_Avoid_: variable, property, knob, option

**Inherit**:
What an unset token does: the overlay uses Handy's built-in value for it, which follows the app theme. A theme that inherits everything is today's overlay.
_Avoid_: default (as a value), fallback, empty, null

**Derived value**:
A value the overlay computes from a token rather than reading from the theme — the accent's soft tint and the neutrals for hairlines and secondary text. Never settable.
_Avoid_: computed token, sub-token, implicit color

**Accent**:
The overlay theme's highlight color, painting the waveform, the recording dot, the text caret, and the spinner. It applies to the overlay only; the settings window keeps the brand pink.
_Avoid_: brand color, pink, primary color, highlight color

**Surface**:
The overlay card's background color. Its translucency is a separate token, surface opacity, so a theme can dim the card without naming a color.
_Avoid_: background, fill, card color

**Text**:
The overlay theme's foreground color: the transcript, and the base every derived neutral is mixed from. Set it whenever the surface is set.
_Avoid_: foreground, fg, font color, transcript color

**Material**:
How the surface is rendered: Flat (an opaque surface) or Glass (a translucent surface that blurs whatever is behind it).
_Avoid_: effect, blur mode, vibrancy, acrylic, frosted (implementation terms)

**Size scale**:
The single factor that zooms every length in the card at once — widths, heights, paddings, and type. It is the only token that changes how much room the overlay needs on screen.
_Avoid_: zoom, size, scale factor (bare), DPI, density

**Resolved overlay theme**:
The overlay theme after the per-token merge of theme file, then settings, then inherit, clamped, together with the material actually rendered, whether Glass is available, and the theme file's state. The single thing both windows read.
_Avoid_: merged theme, effective theme, computed theme

**Apply layer**:
The one module that turns a resolved overlay theme into the overlay's custom properties and material attribute, used identically by the overlay and by the preview.
_Avoid_: theme applier, style engine, renderer

**Window slack**:
The extra native window area around the card, per overlay state, that keeps the card's animations inside the window. Zero under Glass so the blur never paints outside the card.
_Avoid_: padding, margin, window bleed

**Theme file**:
A file on disk that supplies overlay-theme tokens, so external theming tools can drive the overlay without the settings window.
_Avoid_: config file, user stylesheet, custom CSS

**Preview**:
A rendering of the overlay inside the settings window that reflects the current overlay theme before it is shown for real.
_Avoid_: mock, sample, demo, thumbnail
