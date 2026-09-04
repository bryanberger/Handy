# Handy

[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?style=for-the-badge&logo=discord&logoColor=white)](https://discord.com/invite/WVBeWsNXK4)

**A free, open source, and extensible speech-to-text application that works completely offline.**

Handy is a cross-platform desktop application that provides simple, privacy-focused speech transcription. Press a shortcut, speak, and have your words appear in any text field. This happens on your own computer without sending any information to the cloud.

## Why Handy?

Handy was created to fill the gap for a truly open source, extensible speech-to-text tool. As stated on [handy.computer](https://handy.computer):

- **Free**: Accessibility tooling belongs in everyone's hands, not behind a paywall
- **Open Source**: Together we can build further. Extend Handy for yourself and contribute to something bigger
- **Private**: Your voice stays on your computer. Get transcriptions without sending audio to the cloud
- **Simple**: One tool, one job. Transcribe what you say and put it into a text box

Handy isn't trying to be the best speech-to-text app—it's trying to be the most forkable one.

## How It Works

1. **Press** a configurable keyboard shortcut: hold it to record and release to stop, or tap it to toggle recording on and off (Hold-only and Toggle-only modes are also available)
2. **Speak** your words while the shortcut is active
3. **Release** and Handy processes your speech using Whisper
4. **Get** your transcribed text pasted directly into whatever app you're using

The process is entirely local:

- Silence is filtered using VAD (Voice Activity Detection) with Silero
- Transcription uses your choice of models:
  - **Whisper models** (Small/Medium/Turbo/Large) with GPU acceleration when available
  - **Parakeet V3** - CPU-optimized model with excellent performance and automatic language detection
- Works on Windows, macOS, and Linux

## Quick Start

### Installation

1. Download the latest release from the [releases page](https://github.com/cjpais/Handy/releases) or the [website](https://handy.computer)
   - **macOS**: Also available via [Homebrew cask](https://formulae.brew.sh/cask/handy): `brew install --cask handy`
   - **Windows**: Also available via [winget](https://github.com/microsoft/winget-pkgs): `winget install cjpais.Handy` \
     **Note:** The Homebrew cask and winget package are not maintained by the Handy developers.
2. Install the application
3. Launch Handy and grant necessary system permissions (microphone, accessibility)
4. Configure your preferred keyboard shortcuts in Settings
5. Start transcribing!

### Development Setup

For detailed build instructions including platform-specific requirements, see [BUILD.md](BUILD.md).

## Integrations

<a href="https://www.raycast.com/mattiacolombomc/handy" title="Install Handy Raycast Extension"><img src="https://www.raycast.com/mattiacolombomc/handy/install_button@2x.png?v=1.1" height="64" style="height: 64px;" alt="Install handy Raycast Extension" /></a>

Control Handy from [Raycast](https://www.raycast.com) — start/stop recording, browse transcript history, manage dictionary, switch models and languages.

[Source](https://github.com/mattiacolombomc/raycast-handy) · by [@mattiacolombomc](https://github.com/mattiacolombomc)

## Overlay Theme File

`overlay_theme.json` **is** the overlay theme. The Appearance tab is an editor for it, and a text editor or an external theming tool (Omarchy and the like) edits the same document; whichever writes it, the change is live at once. There is no second copy in Handy's settings, so the tab and the file cannot disagree.

**Where Handy looks.** The file is always named `overlay_theme.json`. The first candidate that resolves to a readable file wins, and that document is the only one used. Locations are never merged.

| Priority | Location                                                                                       |
| -------- | ---------------------------------------------------------------------------------------------- |
| 1        | The exact path in `HANDY_OVERLAY_THEME_FILE`, when it is set. Nothing else is tried.           |
| 2        | `Data/` beside the executable, for a portable install.                                         |
| 3        | `~/.config/handy/`, on every platform, or `$XDG_CONFIG_HOME/handy/` when that variable is set. |
| 4        | Handy's app data directory (the path the About tab prints).                                    |

`~/.config/handy/overlay_theme.json` is where to put a new file, on macOS and Windows as much as on Linux; on Windows that is `%USERPROFILE%\.config\handy\overlay_theme.json`. Priority 4 is a fallback. A file in the app data directory, where earlier builds pointed, still loads as before and is written in place; Handy never moves it.

The app data directory is `~/Library/Application Support/com.pais.handy/` on macOS, `%APPDATA%\com.pais.handy\` on Windows, and `$XDG_DATA_HOME/com.pais.handy/` (default `~/.local/share/com.pais.handy/`) on Linux.

The Appearance tab shows the path in effect, or `~/.config/handy/overlay_theme.json` when there is no file anywhere, with a button that opens its folder, creating `~/.config/handy/` first if it does not exist. Under `HANDY_OVERLAY_THEME_FILE` nothing is created, and the button opens the nearest folder along that path that already exists.

**When Handy writes it.** Every change committed in the Appearance tab writes the file in effect: dragging a slider writes once when you let go, not once per pixel, and resetting a token removes its key. The write is atomic (a temp file, then a rename), so nothing ever reads a half-written document, and Handy re-reads what it wrote before applying it, so a document it could not load never becomes the theme. `version` and keys Handy does not recognise are kept, and the tokens are written in this page's table order, two-space indented with a trailing newline. `~/.config/handy/` is created on the first write.

**When it will not.** Handy writes the file only when it owns it: when nothing is there yet in one of Handy's own locations, or when it is a regular, writable file. A **symlinked** or read-only `overlay_theme.json` belongs to whoever made it, which is exactly how a dotfile manager or a theming tool claims the document. Handy then reads it, never writes it, and the Appearance tab says so and turns every token row read-only; "Copy theme as JSON" still works, and the on-screen preview still previews. A path named by `HANDY_OVERLAY_THEME_FILE` is written like any other once it is a regular writable file, since you chose it; Handy will not create one there.

**Upgrading.** A theme set in the Appearance tab before this file became the theme is copied into `~/.config/handy/overlay_theme.json` once, at the first launch that finds no theme file anywhere, and logged. A file already in place wins and nothing is migrated.

**What the file may contain.** A JSON object with an optional `version` plus any of the twenty-three overlay-theme tokens:

| Key               | Type       | Range                                                                                                                           |
| ----------------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `version`         | integer    | `1` (absent means 1)                                                                                                            |
| `accent`          | string     | `"#RRGGBB"`                                                                                                                     |
| `surface`         | string     | `"#RRGGBB"`                                                                                                                     |
| `surface_opacity` | number     | 0.30 to 1.00                                                                                                                    |
| `glass_tint`      | number     | 0.00 to 1.00                                                                                                                    |
| `text`            | string     | `"#RRGGBB"`                                                                                                                     |
| `border`          | string     | `"#RRGGBB"`                                                                                                                     |
| `border_opacity`  | number     | 0.00 to 1.00                                                                                                                    |
| `material`        | string     | `"flat"` or `"glass"`                                                                                                           |
| `glass_material`  | string     | `"hud_window"`, `"popover"`, `"menu"`, `"sidebar"`, `"under_window_background"`, `"sheet"`, `"tooltip"`, `"content_background"` |
| `glass_style`     | string     | `"regular"` or `"clear"`                                                                                                        |
| `shadow_strength` | number     | 0.00 to 1.00                                                                                                                    |
| `shadow_offset_y` | integer px | 0 to 16                                                                                                                         |
| `show_waveform`   | boolean    | `true` or `false`                                                                                                               |
| `show_cancel`     | boolean    | `true` or `false`                                                                                                               |
| `size_scale`      | number     | 0.80 to 1.50                                                                                                                    |
| `radius`          | integer px | 0 to 32                                                                                                                         |
| `border_width`    | integer px | 0 to 4                                                                                                                          |
| `padding`         | integer px | 0 to 20                                                                                                                         |
| `element_gap`     | integer px | 0 to 40                                                                                                                         |
| `waveform_style`  | string     | `"bars"`, `"ribbon"`, `"bloom"`, `"motes"`, `"matrix"` or `"steps"`                                                             |
| `waveform_gap`    | integer px | 0 to 5                                                                                                                          |
| `waveform_width`  | integer px | 2 to 6                                                                                                                          |
| `edge_margin`     | integer px | 0 to 200                                                                                                                        |

`surface_opacity` and `glass_tint` are the card's two alphas, one per material. `surface_opacity` is how opaque the Flat card is, and `glass_tint` is how much of the same `surface` colour covers the glass. Each is ignored under the other material, so one theme can hold an opaque Flat card and see-through Glass, and picking Glass shows glass straight away. The Appearance tab shows whichever applies to the material in effect. `glass_style` picks which Liquid Glass draws the Glass surface on macOS 26 and later, and is the one the Appearance tab shows; `glass_material` picks the `NSVisualEffectMaterial` blur on older macOS. `glass_material` is **theme-file only**. It drives the fallback engine, has no row in the Appearance tab, and Handy reads it from this file and nowhere else. Each is read by one engine and ignored by the other, so a file can carry both, and both do nothing while `material` is `"flat"` or off macOS. `border`, `border_opacity` and `border_width` are the card's edge. An unset edge is a foreground hairline everywhere except Clear glass, where it is white at 35 %, the highlight Spotlight's own capsule carries. Clear is the one surface dark enough in both app themes for it to read. `shadow_strength` and `shadow_offset_y` are the card's drop shadow, and they mean something different on each material, because each draws its shadow somewhere else. Under Flat the card draws its own, so the strength shapes it and the offset pushes it further below the card; unset, the strength is 0 and the offset 4, today's shadowless card. Under Glass the shadow is macOS's own, drawn outside a window the card fills exactly, and `NSWindow` offers no strength, radius or offset at all, so any value above zero is that shadow and zero turns it off; unset it is 1, the Glass overlay that has always shipped, and `shadow_offset_y` is ignored. A Flat shadow grows the overlay window on every side to fall into, and the card is inset from the window's edges by the same amount, so **the card never moves**: switching a shadow on, or dragging its offset, leaves it exactly where it was. Towards the screen edge the overlay is anchored to, the window grows only as far as `edge_margin`, the gap the card already has to the usable edge, so it never reaches over the Dock, the taskbar or the menu bar and never swallows a click meant for them; the faint tail of the shadow is clipped at that edge instead.

`waveform_style` picks how the waveform is drawn: `bars` is today's nine capsules, and `ribbon`, `bloom`, `motes`, `matrix` and `steps` are drawn on a canvas in the same lane. All but `bloom` read `waveform_width` (the ribbon's thinnest point, a mote's diameter, a matrix dot, a step's width), and `matrix` alone also reads `waveform_gap`; the Appearance tab hides the rows a style ignores. The lane is the same width whatever draws in it, so the style never changes how much room the overlay needs.

`show_waveform` and `show_cancel` take those two elements off the overlay, for a more minimal card. Both are `true` unset, so a theme that says nothing draws today's row. The recording dot and the Live timer stay. Hiding either one also shrinks the resting pill to what is left of the row, and only that pill: the working pill and the Live panel keep their widths. Without the waveform the row is the dot and the cancel button, one padding from either edge. Without the cancel button it is the dot and the waveform, the column the button sat in gone rather than left empty, so no space is stranded at the right. With both hidden the dot is alone on the row, and the resting pill is a square as wide as the row is tall, with that dot centred in it and the radius token rounding it. Hiding the cancel button does not take cancelling away: the keyboard shortcut and `--cancel` still work.

`padding` insets the card on all four sides, so it makes the card taller as well as roomier. `element_gap` puts extra space between the row's elements, once on each side of the row's middle, so the card is twice the gap wider; a resting pill that lost the cancel button has one boundary left and pays for one. Unset it is 0. With `size_scale` and `border_width` those two are the four tokens that change how much room the overlay needs on screen under either material. Under Flat the two shadow tokens add to them, because the window grows on every side to hold the shadow. Under Glass, where the window is the card exactly, `waveform_gap`, `waveform_width`, `show_waveform` and `show_cancel` do instead, because they decide how wide a resting pill is.

`edge_margin` is how far the overlay sits from the screen edge it is anchored to, in points, `0` meaning flush with the **usable** edge: below the menu bar and above the Dock on macOS, inside the work area on Windows, and inside the area panels leave free on Wayland under layer shell. It is the one token measured against the screen rather than the card, so `size_scale` does not multiply it — the gap you set is the gap you get at every size. Which edge it applies to follows Overlay Position; leaving it unset keeps Handy's own gap on both edges, which differs per platform. It is also the room a Flat shadow has towards that edge: the window grows by the margin at most, so at `0` a shadowed card is flush and its tail is clipped, and the card still does not move. One exception to "flush with the usable edge": on X11, or on Wayland with `HANDY_NO_GTK_LAYER_SHELL=1` or a compositor without `zwlr_layer_shell_v1`, the desktop may report no work area, and the margin is then measured from the raw screen edge, so `0` can put the overlay behind a panel. That is what Handy already does there today.

**Inherit.** Every token is optional, and an absent key does exactly what an explicit `null` does. Both inherit Handy's own theme-aware value for that token, so a file that sets only `surface` leaves the other twenty-two looking exactly as they do today. `{ "version": 1 }` and `{}` are valid documents that change nothing, and deleting the file puts the overlay back to its built-in look. Handy writes inherit as an absent key rather than a `null`, so resetting a token shortens the document.

**A full theme:**

```json
{
  "version": 1,
  "accent": "#7aa2f7",
  "surface": "#1a1b26",
  "surface_opacity": 0.92,
  "glass_tint": 0.45,
  "text": "#c0caf5",
  "border": "#ffffff",
  "border_opacity": 0.3,
  "material": "glass",
  "glass_material": "popover",
  "glass_style": "clear",
  "shadow_strength": 0.35,
  "shadow_offset_y": 6,
  "show_waveform": true,
  "show_cancel": false,
  "size_scale": 1.1,
  "radius": 12,
  "border_width": 1,
  "padding": 14,
  "element_gap": 8,
  "waveform_style": "ribbon",
  "waveform_gap": 2,
  "waveform_width": 4,
  "edge_margin": 24
}
```

**What a theming tool might emit.** Color parsing is lenient. It accepts `#RGB` shorthand, a missing `#`, any case, surrounding whitespace, and a UTF-8 BOM. The four enums are lenient too. `material` ignores case and surrounding whitespace; `glass_material`, `glass_style` and `waveform_style` also drop everything that is not a letter or a digit, so `"HUD Window"`, `"hud-window"` and `"hud_window"` all read as `"hud_window"`, and `"Clear"` and `"clear "` both read as `"clear"`. Everything else must be a correctly typed JSON value, including the two switches, which take `true` and `false` and not `"true"` or `1`. Unknown keys are ignored, so `"_comment"` is the supported way to annotate a document:

```json
{
  "version": 1,
  "_comment": "generated by omarchy-theme-set; do not edit",
  "accent": "#8AADF4",
  "surface": "24273a",
  "text": "#cad",
  "surface_opacity": 1,
  "material": "Flat",
  "app_theme": "dark"
}
```

That resolves to accent `#8aadf4`, surface `#24273a`, text `#ccaadd`, surface opacity 1.0 and material Flat; `app_theme` is ignored with a warning, and the seventeen unmentioned tokens inherit. Both unknown keys survive the next write from the Appearance tab, so an annotation stays where its author put it.

**When it is re-read.** A file watcher on its folder applies a hand edit or a tool's write as it happens, to the overlay and to an open Appearance tab, with no restart and no button. It is debounced, so one save is one update, and a write Handy made itself changes nothing twice. Handy also re-reads at launch, every time the overlay is shown, and when the Appearance tab is opened, so a missed event heals itself by the next dictation. Where the watcher cannot start, the tab shows a Reload button; where it can, there is nothing for it to do and it is not shown.

**When something is wrong.** A malformed or unreadable document keeps the last one that parsed, so a file caught half-written never blanks the overlay. A single bad key costs only that key, which inherits, and Handy clamps a number outside its range. Everything is logged, and the Appearance tab lists the problems. Changing a value in the tab while the file is broken replaces it with the values on screen, which is the point of changing it.

**Forward compatibility.** Color values are `"#RRGGBB"` strings today. A future schema version may also accept `{ "light": "#RRGGBB", "dark": "#RRGGBB" }` for the same keys. The key names will not change, writers that emit a single string stay valid, and readers should tolerate either shape.

## Architecture

Handy is built as a Tauri application combining:

- **Frontend**: React + TypeScript with Tailwind CSS for the settings UI
- **Backend**: Rust for system integration, audio processing, and ML inference
- **Core Libraries**:
  - `transcribe-cpp`: Local speech recognition with Whisper-family models (GGML/GGUF)
  - `transcribe-rs`: CPU-optimized speech recognition with Parakeet models
  - `cpal`: Cross-platform audio I/O
  - `vad-rs`: Voice Activity Detection
  - `rdev`: Global keyboard shortcuts and system events
  - `rubato`: Audio resampling

### Debug Mode

Handy includes an advanced debug mode for development and troubleshooting. Access it by pressing:

- **macOS**: `Cmd+Shift+D`
- **Windows/Linux**: `Ctrl+Shift+D`

### CLI Parameters

Handy supports command-line flags for controlling a running instance and customizing startup behavior. These work on all platforms (macOS, Windows, Linux).

**Remote control flags** (sent to an already-running instance via the single-instance plugin):

```bash
handy --toggle-transcription    # Toggle recording on/off
handy --toggle-post-process     # Toggle recording with post-processing on/off
handy --cancel                  # Cancel the current operation
handy --preview-overlay         # Show the overlay for a few seconds (theme preview)
```

**Startup flags:**

```bash
handy --start-hidden            # Start without showing the main window
handy --no-tray                 # Start without the system tray icon
handy --debug                   # Enable debug mode with verbose logging
handy --help                    # Show all available flags
```

Flags can be combined for autostart scenarios:

```bash
handy --start-hidden --no-tray
```

> **macOS tip:** When Handy is installed as an app bundle, invoke the binary directly:
>
> ```bash
> /Applications/Handy.app/Contents/MacOS/Handy --toggle-transcription
> ```

## Known Issues & Current Limitations

This project is actively being developed and has some [known issues](https://github.com/cjpais/Handy/issues). We believe in transparency about the current state:

### Bluetooth Headset Microphones (macOS)

Using a Bluetooth headset microphone on macOS may temporarily reduce playback quality or volume while recording because Bluetooth switches to bidirectional audio. Keep your headphones as the output device and select your Mac's built-in or an external microphone in Handy to avoid this.

### fn and Globe Key Shortcuts (macOS)

Shortcuts that include the `fn` (Globe) key **only work on Apple keyboards** — your Mac's built-in keyboard or an Apple external keyboard. They will never trigger on a third-party keyboard, even while it is connected to the same Mac.

This is a hardware limitation rather than a Handy bug. `fn` is not part of the standard USB HID keyboard specification: Apple reports it through a vendor-specific usage that macOS honors only from Apple devices, while third-party keyboards handle their `Fn` key entirely in firmware and send nothing to the computer. There is no event for Handy to listen for.

If you switch between a MacBook keyboard and an external one, pick a shortcut built from standard modifiers (`ctrl`, `option`, `shift`, `command`) or a regular key instead.

### Major Issues (Help Wanted)

**Whisper Model Crashes:**

- Whisper models crash on certain system configurations (Windows and Linux)
- Does not affect all systems - issue is configuration-dependent
  - If you experience crashes and are a developer, please help to fix and provide debug logs!

**Wayland Support (Linux):**

- Limited support for Wayland display server
- Requires [`wtype`](https://github.com/atx/wtype) or [`dotool`](https://sr.ht/~geb/dotool/) for text input to work correctly (see [Linux Notes](#linux-notes) below for installation)

### Linux Notes

**Text Input Tools:**

For reliable text input on Linux, install the appropriate tool for your display server:

| Display Server | Recommended Tool | Install Command                                    |
| -------------- | ---------------- | -------------------------------------------------- |
| X11            | `xdotool`        | `sudo apt install xdotool`                         |
| Wayland        | `wtype`          | `sudo apt install wtype`                           |
| Both           | `dotool`         | `sudo apt install dotool` (requires `input` group) |

- **X11**: Install `xdotool` for both direct typing and clipboard paste shortcuts
- **Ubuntu 26.04**: Has Wayland display server by default. `wtype` does not work, you need to install `ydotool` and configure systemd as described [here](https://github.com/cjpais/Handy/pull/557#issuecomment-3781249267).
- **Wayland**: Install `wtype` (preferred) or `dotool` for text input to work correctly
- **dotool setup**: Requires adding your user to the `input` group: `sudo usermod -aG input $USER` (then log out and back in)

Without these tools, Handy falls back to enigo which may have limited compatibility, especially on Wayland.

**Other Notes:**

- **Runtime library dependency (`libgtk-layer-shell.so.0`)**:
  - Handy links `gtk-layer-shell` on Linux. If startup fails with `error while loading shared libraries: libgtk-layer-shell.so.0`, install the runtime package for your distro:

    | Distro        | Package to install    | Example command                        |
    | ------------- | --------------------- | -------------------------------------- |
    | Ubuntu/Debian | `libgtk-layer-shell0` | `sudo apt install libgtk-layer-shell0` |
    | Fedora/RHEL   | `gtk-layer-shell`     | `sudo dnf install gtk-layer-shell`     |
    | Arch Linux    | `gtk-layer-shell`     | `sudo pacman -S gtk-layer-shell`       |

  - For building from source on Ubuntu/Debian, you may also need `libgtk-layer-shell-dev`.

- The recording overlay is disabled by default on Linux (`Overlay Position: None`) because certain compositors treat it as the active window. When the overlay is visible it can steal focus, which prevents Handy from pasting back into the application that triggered transcription. If you enable the overlay anyway, be aware that clipboard-based pasting might fail or end up in the wrong window.
- If you are having trouble with the app, running with the environment variable `WEBKIT_DISABLE_DMABUF_RENDERER=1` may help
- If Handy fails to start reliably on Linux, see [Troubleshooting → Linux Startup Crashes or Instability](#linux-startup-crashes-or-instability).
- **Global keyboard shortcuts (Wayland):** On Wayland, system-level shortcuts must be configured through your desktop environment or window manager. Use the [CLI flags](#cli-parameters) as the command for your custom shortcut.

  **GNOME:**
  1. Open **Settings > Keyboard > Keyboard Shortcuts > Custom Shortcuts**
  2. Click the **+** button to add a new shortcut
  3. Set the **Name** to `Toggle Handy Transcription`
  4. Set the **Command** to `handy --toggle-transcription`
  5. Click **Set Shortcut** and press your desired key combination (e.g., `Super+O`)

  **KDE Plasma:**
  1. Open **System Settings > Shortcuts > Custom Shortcuts**
  2. Click **Edit > New > Global Shortcut > Command/URL**
  3. Name it `Toggle Handy Transcription`
  4. In the **Trigger** tab, set your desired key combination
  5. In the **Action** tab, set the command to `handy --toggle-transcription`

  **Sway / i3:**

  Add to your config file (`~/.config/sway/config` or `~/.config/i3/config`):

  ```ini
  bindsym $mod+o exec handy --toggle-transcription
  ```

  **Hyprland:**

  Add to your config file (`~/.config/hypr/hyprland.conf`):

  ```ini
  bind = $mainMod, O, exec, handy --toggle-transcription
  ```

- You can also trigger Handy externally via Unix signals or the CLI flags, which lets Wayland window managers or other hotkey daemons keep ownership of keybindings:

  | Action                                    | Trigger                                                  |
  | ----------------------------------------- | -------------------------------------------------------- |
  | Toggle transcription                      | `pkill -USR2 -n handy` or `handy --toggle-transcription` |
  | Toggle transcription with post-processing | `handy --toggle-post-process`                            |

  Example Sway config:

  ```ini
  bindsym $mod+o exec pkill -USR2 -n handy
  bindsym $mod+p exec handy --toggle-post-process
  ```

  `pkill` here simply delivers the signal—it does not terminate the process.

  > **Behavior change:** older releases also accepted `SIGUSR1` for toggling transcription with post-processing. WebKitGTK — the webview engine embedded in Handy on Linux — uses SIGUSR1 internally to coordinate JavaScript garbage collection, so listening for it caused phantom recordings and interrupted dictations every few minutes ([#1660](https://github.com/cjpais/Handy/issues/1660)). Handy no longer listens for SIGUSR1 on Linux; the post-processing toggle is still available via `handy --toggle-post-process`. **Remove any `pkill -USR1` bindings**: the signal is now delivered straight to WebKit's internal handler and can crash the app.

**Overlay & Pasting Issues (Linux):**

- The recording overlay window can interfere with pasting transcribed text into target applications on Linux (X11)
- **Solution:** Open **Settings > Appearance** and, under **Overlay**, set the overlay style to **"None"** to disable the overlay
- Enable **"Audio Feedback"** (in **Settings > General**) if you still want audible confirmation of recording state
- Users who upgrade from older versions or import settings from other platforms may need to manually apply this change

### Platform Support

- **macOS (both Intel and Apple Silicon)**
- **x64 Windows**
- **x64 Linux**

### System Requirements/Recommendations

The following are recommendations for running Handy on your own machine. If you don't meet the system requirements, the performance of the application may be degraded. We are working on improving the performance across all kinds of computers and hardware.

**For Whisper Models:**

- **macOS**: M series Mac, Intel Mac
- **Windows**: Intel, AMD, or NVIDIA GPU
- **Linux**: Intel, AMD, or NVIDIA GPU
  - Ubuntu 22.04, 24.04

**For Parakeet V3 Model:**

- **CPU-only operation** - runs on a wide variety of hardware
- **Minimum**: Intel Skylake (6th gen) or equivalent AMD processors
- **Performance**: ~5x real-time speed on mid-range hardware (tested on i5)
- **Automatic language detection** - no manual language selection required

## Roadmap & Active Development

We're actively working on several features and improvements. Contributions and feedback are welcome!

### In Progress

**Debug Logging:**

- Adding debug logging to a file to help diagnose issues

**macOS Keyboard Improvements:**

- Support for Globe key as transcription trigger
- A rewrite of global shortcut handling for MacOS, and potentially other OS's too.

**Opt-in Analytics:**

- Collect anonymous usage data to help improve Handy
- Privacy-first approach with clear opt-in

**Settings Refactoring:**

- Cleanup and refactor settings system which is becoming bloated and messy
- Implement better abstractions for settings management

**Tauri Commands Cleanup:**

- Abstract and organize Tauri command patterns
- Investigate tauri-specta for improved type safety and organization

## Verify Release Signatures

Handy release artifacts are signed with Tauri's updater signature format. The public key is stored in [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) under `plugins.updater.pubkey`.

To verify a release manually, set `ARTIFACT` to the filename you downloaded, save the `pubkey` value from `src-tauri/tauri.conf.json` to `handy.pub.b64`, then decode the public key and matching `.sig` file from base64 and verify the artifact with `minisign`:

```bash
# Replace with the file you downloaded
ARTIFACT="Handy_0.8.1_amd64.AppImage"

python3 - "$ARTIFACT" <<'PY'
import base64, pathlib, sys

artifact = sys.argv[1]

pub = pathlib.Path("handy.pub.b64").read_text().strip()
pathlib.Path("handy.pub").write_bytes(base64.b64decode(pub))

sig = pathlib.Path(f"{artifact}.sig").read_text().strip()
pathlib.Path(f"{artifact}.minisig").write_bytes(base64.b64decode(sig))
PY

minisign -Vm "$ARTIFACT" \
  -p handy.pub \
  -x "$ARTIFACT.minisig"
```

On success, `minisign` prints:

```text
Signature and comment signature verified
```

Do not use `gpg` for these `.sig` files.

## Troubleshooting

### Manual Model Installation (For Proxy Users or Network Restrictions)

If you're behind a proxy, firewall, or in a restricted network environment where Handy cannot download models automatically, you can manually download and install them. The URLs are publicly accessible from any browser.

#### Step 1: Find Your App Data Directory

1. Open Handy settings
2. Navigate to the **About** section
3. Copy the "App Data Directory" path shown there, or use the shortcuts:
   - **macOS**: `Cmd+Shift+D` to open debug menu
   - **Windows/Linux**: `Ctrl+Shift+D` to open debug menu

The typical paths are:

- **macOS**: `~/Library/Application Support/com.pais.handy/`
- **Windows**: `C:\Users\{username}\AppData\Roaming\com.pais.handy\`
- **Linux**: `~/.config/com.pais.handy/`

#### Step 2: Create Models Directory

Inside your app data directory, create a `models` folder if it doesn't already exist:

```bash
# macOS/Linux
mkdir -p ~/Library/Application\ Support/com.pais.handy/models

# Windows (PowerShell)
New-Item -ItemType Directory -Force -Path "$env:APPDATA\com.pais.handy\models"
```

#### Step 3: Download Model Files

Download the models you want from below

**Whisper Models (single .bin files):**

- Small (487 MB): `https://blob.handy.computer/ggml-small.bin`
- Medium (492 MB): `https://blob.handy.computer/whisper-medium-q4_1.bin`
- Turbo (1600 MB): `https://blob.handy.computer/ggml-large-v3-turbo.bin`
- Large (1100 MB): `https://blob.handy.computer/ggml-large-v3-q5_0.bin`

**Parakeet Unified EN 0.6B (single `.gguf` file, recommended):**

- Q8_0 (731 MB): `https://huggingface.co/handy-computer/parakeet-unified-en-0.6b-gguf/resolve/main/parakeet-unified-en-0.6b-Q8_0.gguf`

**Parakeet Models (compressed archives):**

- V2 (473 MB): `https://blob.handy.computer/parakeet-v2-int8.tar.gz`
- V3 (478 MB): `https://blob.handy.computer/parakeet-v3-int8.tar.gz`

#### Step 4: Install Models

**For Whisper Models (.bin files):**

Simply place the `.bin` file directly into the `models` directory:

```
{app_data_dir}/models/
├── ggml-small.bin
├── whisper-medium-q4_1.bin
├── ggml-large-v3-turbo.bin
└── ggml-large-v3-q5_0.bin
```

**For GGUF Models (.gguf files):**

Place the `.gguf` file directly into the `models` directory, exactly like the Whisper `.bin` files above. Handy also picks up models already present in the shared Hugging Face cache (`~/.cache/huggingface/hub`), so a copy downloaded by another tool works without being moved.

**For Parakeet Models (.tar.gz archives):**

1. Extract the `.tar.gz` file
2. Place the **extracted directory** into the `models` folder
3. The directory must be named exactly as follows:
   - **Parakeet V2**: `parakeet-tdt-0.6b-v2-int8`
   - **Parakeet V3**: `parakeet-tdt-0.6b-v3-int8`

Final structure should look like:

```
{app_data_dir}/models/
├── parakeet-tdt-0.6b-v2-int8/     (directory with model files inside)
│   ├── (model files)
│   └── (config files)
└── parakeet-tdt-0.6b-v3-int8/     (directory with model files inside)
    ├── (model files)
    └── (config files)
```

**Important Notes:**

- For Parakeet models, the extracted directory name **must** match exactly as shown above
- Do not rename the `.bin` or `.gguf` files—use the exact filenames from the download URLs
- After placing the files, restart Handy to detect the new models

#### Step 5: Verify Installation

1. Restart Handy
2. Open Settings → Models
3. Your manually installed models should now appear as "Downloaded"
4. Select the model you want to use and test transcription

### Custom Whisper Models

Handy can auto-discover custom Whisper GGML models placed in the `models` directory. This is useful for users who want to use fine-tuned or community models not included in the default model list.

**How to use:**

1. Obtain a Whisper model in GGML `.bin` format (e.g., from [Hugging Face](https://huggingface.co/models?search=whisper%20ggml))
2. Place the `.bin` file in your `models` directory (see paths above)
3. Restart Handy to discover the new model
4. The model will appear in the "Custom Models" section of the Models settings page

**Important:**

- Community models are user-provided and may not receive troubleshooting assistance
- The model must be a valid Whisper GGML format (`.bin` file)
- Model name is derived from the filename (e.g., `my-custom-model.bin` → "My Custom Model")

### Linux Startup Crashes or Instability

If Handy fails to start reliably on Linux — for example, it crashes shortly after launch, never shows its window, or reports a Wayland protocol error — try the steps below in order.

**1. Install (or reinstall) `gtk-layer-shell`**

Handy uses `gtk-layer-shell` for its recording overlay and links against it at runtime. A missing or broken installation is the most common cause of startup failures and can manifest as a crash or a hang well before any window is shown. Make sure the runtime package is installed for your distro:

| Distro        | Package to install    | Example command                        |
| ------------- | --------------------- | -------------------------------------- |
| Ubuntu/Debian | `libgtk-layer-shell0` | `sudo apt install libgtk-layer-shell0` |
| Fedora/RHEL   | `gtk-layer-shell`     | `sudo dnf install gtk-layer-shell`     |
| Arch Linux    | `gtk-layer-shell`     | `sudo pacman -S gtk-layer-shell`       |

If it is already installed and you still see startup problems, try reinstalling it (e.g. `sudo pacman -S gtk-layer-shell` again) in case the library files were corrupted by a partial upgrade.

**2. Disable the GTK layer shell overlay (`HANDY_NO_GTK_LAYER_SHELL`)**

If installing the library does not help, you can skip `gtk-layer-shell` initialization entirely as a workaround. On some compositors (notably KDE Plasma under Wayland) it has been reported to interact poorly with the recording overlay. With this variable set, the overlay falls back to a regular always-on-top window:

```bash
HANDY_NO_GTK_LAYER_SHELL=1 handy
```

**3. Disable WebKit DMA-BUF renderer (`WEBKIT_DISABLE_DMABUF_RENDERER`)**

On some GPU/driver combinations the WebKitGTK DMA-BUF renderer can cause the window to fail to render or to crash. Try:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 handy
```

**Making a workaround permanent**

Once you've found a flag that helps, export it from your shell profile (`~/.bashrc`, `~/.zshenv`, …) or from the desktop autostart entry that launches Handy. If you launch Handy from a `.desktop` file, you can prefix the `Exec=` line, e.g.:

```ini
Exec=env HANDY_NO_GTK_LAYER_SHELL=1 handy
```

If a workaround helps you, please [open an issue](https://github.com/cjpais/Handy/issues) describing your distro, desktop environment, and session type — that information helps us narrow down the underlying bug.

### Handy Starts or Stops Recording on Its Own (Linux)

Handy 0.9.4 and earlier listened for `SIGUSR1` as a remote-control trigger. WebKitGTK — the webview engine embedded in Handy on Linux — uses that same signal internally to coordinate JavaScript garbage collection, so GC cycles were misread as hotkey presses: recordings started on their own, or real dictations were cut off mid-sentence (typically ~2 minutes in). See [#1660](https://github.com/cjpais/Handy/issues/1660).

Update to a newer release, and replace any `pkill -USR1 -n handy` keybindings with `handy --toggle-post-process`.

### How to Contribute

1. **Check existing issues** at [github.com/cjpais/Handy/issues](https://github.com/cjpais/Handy/issues)
2. **Fork the repository** and create a feature branch
3. **Test thoroughly** on your target platform
4. **Submit a pull request** with clear description of changes
5. **Join the discussion** - reach out at [contact@handy.computer](mailto:contact@handy.computer)

The goal is to create both a useful tool and a foundation for others to build upon—a well-patterned, simple codebase that serves the community.

## Sponsors

<div align="center">
  We're grateful for the support of our sponsors who help make Handy possible:
  <br><br>
  <a href="https://wordcab.com">
    <img src="sponsor-images/wordcab.png" alt="Wordcab" width="120" height="120">
  </a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://github.com/epicenter-so/epicenter">
    <img src="sponsor-images/epicenter.png" alt="Epicenter" width="120" height="120">
  </a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://boltai.com?utm_source=handy">
    <img src="sponsor-images/boltai.jpg" alt="Bolt AI" width="120" height="120">
  </a>
</div>

## Related Projects

- **[Handy CLI](https://github.com/cjpais/handy-cli)** - The original Python command-line version
- **[handy.computer](https://handy.computer)** - Project website with demos and documentation

## License

MIT License - see [LICENSE](LICENSE) file for details.

Handy is open-source software, but the Handy name, logo, icon, and brand assets are not open-source. Unofficial forks, rewrites, and redistributions must use their own branding and must not imply endorsement or affiliation.

## Acknowledgments

- **Whisper** by OpenAI for the speech recognition model
- **ggml and transcribe.cpp** for amazing cross-platform speech-to-text inference/acceleration
- **Silero** for great lightweight VAD
- **Tauri** team for the excellent Rust-based app framework
- **Community contributors** helping make Handy better
