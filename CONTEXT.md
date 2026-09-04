# Handy

Handy is a local, offline speech-to-text desktop app. A global shortcut records the microphone, a local model transcribes it, and Handy pastes the text into the active application. This glossary covers the app's appearance and its recording overlay.

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
The visible rounded rectangle the overlay draws inside its window, the pill in Minimal and the panel in Live. How much of the window it fills depends on the material. Flat leaves transparent slack around it; Glass makes the window the card exactly.
_Avoid_: overlay window, box, container, bubble

**Card shape**:
Which of the five rectangles the card is drawing right now: the resting pill, the working pill, the Live pill, the Live panel collapsed to a pill, or the Live panel open. Only the webview knows it, so it reports it. Under Flat that is bookkeeping; under Glass it is what the window is sized to.
_Avoid_: card state, overlay state (that is what the card is showing, not what shape it takes), variant, form

**Card metrics**:
The three tokens a card's rectangle is computed from, clamped and carried together. Size scale and border width decide how much room the card needs; size scale and radius decide how round its corners are. They travel as one because the window and the blur are sized and rounded from the same set.
_Avoid_: dimensions, sizing options, geometry (which is the computation, not its inputs)

**Overlay style**:
Which overlay is shown: None (hidden), Minimal (a compact pill), or Live (a pill that opens into a panel showing the transcript as it arrives).
_Avoid_: overlay mode, overlay type, overlay form

**Overlay position**:
Which screen edge the overlay sits at: Top or Bottom.
_Avoid_: placement, anchor, alignment

**Overlay theme**:
The tokens that decide how the overlay looks: accent, surface, text, border, material and the macOS material Glass is drawn with, size, radius, and spacing. Every token is optional, so a theme may set one value or all of them.
_Avoid_: overlay skin, overlay colors, overlay CSS, customization

**Token**:
One named, user-settable value in the overlay theme, such as the accent color or the corner radius. Every token is optional, and setting one in the settings window or in the theme file is the same act: the tab writes the file.
_Avoid_: variable, property, knob, option

**Inherit**:
What an unset token does. The overlay uses Handy's built-in value for it, which follows the app theme. A theme that inherits everything is today's overlay.
_Avoid_: default (as a value), fallback, empty, null

**Derived value**:
A value the overlay computes from a token rather than reading it from the theme. These are the accent's soft tint and the neutrals for hairlines and secondary text. Never settable.
_Avoid_: computed token, sub-token, implicit color

**Accent**:
The overlay theme's highlight color, painting the waveform, the recording dot, the text caret, and the spinner. It applies to the overlay only; the settings window keeps the brand pink.
_Avoid_: brand color, pink, primary color, highlight color

**Surface**:
The overlay card's background color. Its translucency is a separate token, surface opacity, so a theme can dim the card without naming a color.
_Avoid_: background, fill, card color

**Text**:
The overlay theme's foreground color. It paints the transcript and is the base every derived neutral is mixed from. Set it whenever the surface is set.
_Avoid_: foreground, fg, font color, transcript color

**Tint strength**:
How much of the surface colour covers the glass under Glass, from untinted to fully covered. Separate from Flat's surface opacity, so switching material never carries the other material's value across.
_Avoid_: glass opacity, tint alpha

**Material**:
How the surface is rendered: Flat (an opaque surface) or Glass (a translucent surface that blurs whatever is behind it).
_Avoid_: effect, blur mode, vibrancy, acrylic, frosted (implementation terms)

**Size scale**:
The single factor that zooms every length in the card at once, including widths, heights, paddings, and type. With border width and padding, it is one of the tokens that change how much room the overlay needs on screen.
_Avoid_: zoom, size, scale factor (bare), DPI, density

**Element gap**:
The extra space between the elements of the card's control row. The row carries one at each boundary between its columns, so a card with the full row is twice the gap wider and its middle keeps the room it had. A resting pill that lost the cancel button, and with it the right column, carries one.
_Avoid_: spacing, item gap, column gap, padding (which insets the card)

**Waveform style**:
How the waveform is drawn: Bars (Handy's own meter, and what an unset theme draws), Ribbon, Bloom, Motes, Matrix or Steps. A look, not a layout: the lane it draws into is the same width whichever is chosen.
_Avoid_: waveform type, visualizer, visualization, animation

**Waveform lane**:
The fixed slot the waveform occupies in the control row, sized from the waveform's width and gap and from nothing else. Bars fills it with capsules; every other style fills it with one canvas.
_Avoid_: waveform area, canvas, slot, container

**Resolved overlay theme**:
The theme file's tokens, clamped, with every unset one left to inherit. It also carries the material actually rendered, whether Glass is available, the theme file's state and whether the watcher is running. The single thing both windows read.
_Avoid_: merged theme, effective theme, computed theme

**Apply layer**:
The one module that turns a resolved overlay theme into the overlay's custom properties and material attribute, used identically by the overlay and the preview.
_Avoid_: theme applier, style engine, renderer

**Window slack**:
The extra native window area around the card, per overlay state, that keeps the card's animations inside the window. Zero under Glass so the blur never paints outside the card.
_Avoid_: padding, margin, window bleed

**Shadow slack**:
The window margin that the card is inset by so its own shadow falls inside the window. Zero under Glass, where macOS draws the shadow outside the card, and zero when the shadow strength is zero. The window keeps the card's own distance from the screen edge, growing by the full slack away from that edge and by only the room the card already had towards it, so the card does not move and the shadow is clipped at the usable edge.
_Avoid_: shadow padding, shadow margin, window slack (which is the room a morph needs)

**Glass style**:
Which Liquid Glass recipe the surface is drawn with under Glass on macOS 26 and later: Regular or Clear.
_Avoid_: glass variant, glass mode

**Engine**:
Which native view draws Glass on the running system: Liquid Glass, the older visual-effect view, or none.
_Avoid_: backend, renderer, implementation (for this concept)

**Theme file**:
`overlay_theme.json` on disk. It is the overlay theme, not a second opinion about it: the Appearance tab writes it, a text editor or a theming tool writes the same document, and a watcher makes either one live. Every token it does not set inherits, and no file at all is today's overlay.
_Avoid_: config file, user stylesheet, custom CSS, override file

**Managed**:
What a theme file is when Handy reads it but will not write it: a symlink, a read-only file, or a path `HANDY_OVERLAY_THEME_FILE` named that does not exist. The Appearance tab locks every token row and says which of those it is.
_Avoid_: locked, read-only theme, external theme, owned by a tool

**Preview**:
The real overlay held on screen with synthetic activity while the user edits the overlay theme, so the user judges every change on the actual card. Started and stopped from the Appearance tab.
_Avoid_: mock, sample, demo, thumbnail, in-page preview
