//! Preview mode keeps the real overlay on screen while its theme is edited.
//!
//! A root module rather than a command file, because the commands are only
//! adapters onto it. The Appearance tab reaches preview mode through the three
//! `#[tauri::command]`s in `commands/overlay_preview.rs`; the cancel funnel,
//! the window-close handler, the exit handler and the `--preview-overlay` flag
//! reach it directly.
//!
//! The tab starts a preview and leaves it running. The overlay stays up in the
//! configured style, driven by synthetic microphone levels (and in Live a
//! synthetic transcript), cycling through the states a real session visits or
//! pinned to one, until the tab stops it. Every token edit repaints it live,
//! because the preview drives the real overlay, the only way to judge the theme
//! at true size and see the Material rendered.
//!
//! The `--preview-overlay` CLI flag is the same driver on a compressed
//! schedule, stopping itself after a few seconds.
//!
//! Nothing here may disturb a real session. A preview refuses to start while
//! Handy is recording, ends the moment a real recording takes the overlay, and
//! is the only thing a cancel touches while it is the one on screen.

use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::{
    StreamPhase, StreamPhaseEvent, StreamTextEvent, StreamWorkKind,
};
use crate::overlay;
use crate::settings::{get_settings, OverlayStyle};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

/// The sample sentence the Live panel shows when the caller supplies none. The
/// Appearance tab passes its own, already translated. The CLI flag has no
/// webview to ask, so it falls back to this English one. No tray translation
/// carries a sample sentence, and the card is judged by colour, size and
/// spacing, not by the words.
const PREVIEW_SAMPLE_TEXT: &str =
    "The quick brown fox jumps over the lazy dog, and Handy writes it down.";

/// Nothing is on screen and the next start may take the guard.
const GUARD_IDLE: u8 = 0;
/// A driver owns the overlay.
const GUARD_RUNNING: u8 = 1;
/// A driver owns the overlay and has been told to let go. It notices within a
/// frame, hides the overlay and releases the guard.
const GUARD_STOPPING: u8 = 2;

/// Whether a preview owns the overlay, and whether it has been asked to stop.
///
/// One atomic rather than two flags, because the two questions are not
/// independent. A stop landing between "a preview is running" and "clear the
/// stop flag for the preview I am starting" used to be dropped, leaving a
/// preview nothing could stop. As one state machine
/// (`idle → running → stopping → idle`) a start, a stop and the driver's
/// release cannot interleave into a lost stop.
///
/// It also tells `cancel_operation` that a cancel belongs to the preview rather
/// than to a real session.
static PREVIEW_GUARD: AtomicU8 = AtomicU8::new(GUARD_IDLE);

/// The state the running preview should be showing: `PreviewState::Cycle` to
/// loop the whole sequence, or one state to hold. The driver re-reads it every
/// frame, so `set_overlay_preview_state` takes effect without restarting.
static PREVIEW_TARGET: AtomicU8 = AtomicU8::new(0);

/// One synthetic level frame per this interval, which also sets how quickly the
/// driver notices a stop, a pin or a real recording. 40 ms rather than 33,
/// because `overlay::emit_levels` drops any frame arriving less than
/// `EMIT_THROTTLE_MS` (33 ms) after the last one, so a faster cadence would
/// halve to ~50 ms.
const FRAME_INTERVAL: Duration = Duration::from_millis(40);

/// How many frames one loop of the synthetic loudness lasts. 120 frames at
/// [`FRAME_INTERVAL`] is 4.8 s, so every preview step long enough to study
/// shows the quiet half of a breath, and the cycle's burst lands inside the
/// capture step of both sequences below.
const LEVEL_CYCLE_FRAMES: u32 = 120;

/// The loudest the breath alone gets, leaving the top of the range to the
/// burst so the two read as different things.
const BREATH_PEAK: f32 = 0.62;

/// The quietest the preview ever gets. Not zero: an empty lane looks like a
/// microphone that stopped, and every style needs something to draw.
const QUIET_FLOOR: f32 = 0.02;

/// Where the burst starts and ends in the cycle, and how much of it each of its
/// two ramps takes. The plateau between them is 0.24 of the cycle, 29 frames,
/// longer than the travelling wave's own 25-frame sweep, so the burst always
/// contains a frame where a bar reaches the top.
const BURST_START: f32 = 0.55;
const BURST_END: f32 = 0.95;
const BURST_EDGE: f32 = 0.08;

/// The Live panel receives another piece of the sample text this often while
/// the preview is listening.
const STREAM_CHUNK_INTERVAL: Duration = Duration::from_millis(240);

/// How long the one-shot preview (`--preview-overlay`) stays on screen before
/// stopping itself.
const ONE_SHOT_DURATION: Duration = Duration::from_millis(3500);

/// How long the driver lets a fresh `show-overlay` settle before it pushes a
/// working phase after it. See [`phase_needs_settle`].
const SHOW_SETTLE: Duration = Duration::from_millis(300);

/// The longest the settle above sleeps in one go. See [`settle`].
const SETTLE_TICK: Duration = Duration::from_millis(20);

/// How often the driver re-reads the two things it does not own: the overlay
/// style in the settings store, and whether the settings window is on screen.
/// Not every frame, because one costs a full settings deserialize and the
/// other a hop to the main thread, and neither is worth doing 25 times a
/// second. 200 ms is well inside the shortest step of any cycle, so a style
/// switch still looks immediate.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// How long a start waits for a previous driver to let go before refusing. Stop
/// then Start is an ordinary thing to do to a toggle, and the stopped driver
/// needs up to a frame to notice, hide the overlay and release the guard.
/// Without this wait that start is refused as "already running", leaving the
/// user pressing a button that does nothing.
const CLAIM_WAIT: Duration = Duration::from_millis(500);

/// How often the wait above re-checks. Short enough that a start following a
/// stop is indistinguishable from one that had nothing to wait for.
const CLAIM_TICK: Duration = Duration::from_millis(10);

/// A state the preview can show, or `Cycle` to loop through all of them. These
/// are the overlay's own states under both names it uses for capture.
/// `Recording` is the Minimal pill's, `Listening` the Live panel's. Asking for
/// the other style's name is not an error, because the driver maps it onto the
/// current style's own.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreviewState {
    /// Loop the whole sequence a real session visits.
    Cycle,
    /// Shown, but no microphone samples yet (the muted pill).
    Arming,
    /// Capturing, Minimal's name for it.
    Recording,
    /// Capturing, Live's name for it (the panel is open, text arriving).
    Listening,
    /// Finalizing the transcript.
    Transcribing,
    /// Post-processing the transcript.
    Processing,
}

impl PreviewState {
    /// The atomic encoding [`PREVIEW_TARGET`] holds. Explicit rather than a
    /// cast, so reordering the variants can never silently change what a stored
    /// value means.
    fn as_u8(self) -> u8 {
        match self {
            PreviewState::Cycle => 0,
            PreviewState::Arming => 1,
            PreviewState::Recording => 2,
            PreviewState::Listening => 3,
            PreviewState::Transcribing => 4,
            PreviewState::Processing => 5,
        }
    }

    fn from_u8(raw: u8) -> PreviewState {
        match raw {
            1 => PreviewState::Arming,
            2 => PreviewState::Recording,
            3 => PreviewState::Listening,
            4 => PreviewState::Transcribing,
            5 => PreviewState::Processing,
            _ => PreviewState::Cycle,
        }
    }
}

/// The requested state as the current style can actually show it. The two
/// capture states are one state under two names, so a tab that still thinks the
/// style is Live cannot pin a Minimal overlay to a state it does not have.
fn normalize(style: OverlayStyle, state: PreviewState) -> PreviewState {
    let live = style == OverlayStyle::Live;
    match state {
        PreviewState::Recording if live => PreviewState::Listening,
        PreviewState::Listening if !live => PreviewState::Recording,
        other => other,
    }
}

/// One step of the cycle: a state to show for a while, or a gap with the
/// overlay hidden so the next loop replays its entrance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CycleStep {
    /// `None` is the gap, with nothing on screen.
    state: Option<PreviewState>,
    duration_ms: u64,
}

const fn step(state: PreviewState, duration_ms: u64) -> CycleStep {
    CycleStep {
        state: Some(state),
        duration_ms,
    }
}

const fn gap(duration_ms: u64) -> CycleStep {
    CycleStep {
        state: None,
        duration_ms,
    }
}

/// Minimal: arming → recording → transcribing → processing → a gap, then round
/// again. ~8.1 s.
const MINIMAL_CYCLE: &[CycleStep] = &[
    step(PreviewState::Arming, 800),
    step(PreviewState::Recording, 3700),
    step(PreviewState::Transcribing, 1600),
    step(PreviewState::Processing, 1400),
    gap(600),
];

/// Live: arming → listening (text streams in) → transcribing (text held) → a
/// gap, then round again. ~7.7 s.
const LIVE_CYCLE: &[CycleStep] = &[
    step(PreviewState::Arming, 800),
    step(PreviewState::Listening, 4400),
    step(PreviewState::Transcribing, 1900),
    gap(600),
];

/// The one-shot's Minimal sequence: all four states inside
/// [`ONE_SHOT_DURATION`].
const ONE_SHOT_MINIMAL_CYCLE: &[CycleStep] = &[
    step(PreviewState::Arming, 120),
    step(PreviewState::Recording, 1500),
    step(PreviewState::Transcribing, 1000),
    step(PreviewState::Processing, 1000),
];

/// The one-shot's Live sequence: the arming pill, the whole sample sentence
/// streamed, then the spinner, all before [`ONE_SHOT_DURATION`] hides it.
const ONE_SHOT_LIVE_CYCLE: &[CycleStep] = &[
    step(PreviewState::Arming, 120),
    step(PreviewState::Listening, 2160),
    step(PreviewState::Transcribing, 1400),
];

/// The schedule that drives one preview run. Two profiles, because the two
/// callers want different things from the same driver. The tab's preview loops
/// for as long as the user edits, so its steps are long enough to study and it
/// ends each round with a gap that replays the entrance. The one-shot has
/// [`ONE_SHOT_DURATION`] to show an entrance, a full sentence and the spinner,
/// so its steps are compressed, it has no gap, and its sequence deliberately
/// outlasts the deadline. Wrapping round would replay the arming pill for a
/// moment and then hide on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Timing {
    minimal: &'static [CycleStep],
    live: &'static [CycleStep],
}

/// Preview mode. Runs until the tab stops it.
const PERSISTENT_TIMING: Timing = Timing {
    minimal: MINIMAL_CYCLE,
    live: LIVE_CYCLE,
};

/// `--preview-overlay`: one compressed pass, then gone.
const ONE_SHOT_TIMING: Timing = Timing {
    minimal: ONE_SHOT_MINIMAL_CYCLE,
    live: ONE_SHOT_LIVE_CYCLE,
};

impl Timing {
    fn sequence(self, style: OverlayStyle) -> &'static [CycleStep] {
        match style {
            OverlayStyle::Live => self.live,
            _ => self.minimal,
        }
    }

    /// How many pieces the sample sentence is revealed in: one per
    /// [`STREAM_CHUNK_INTERVAL`] of the listening step, so the whole sentence
    /// is on screen by the time the panel stops listening. For the one-shot
    /// that also means before its deadline. Read off the Live sequence whatever
    /// the style, because Minimal has no transcript to stream.
    fn chunk_count(self) -> usize {
        let listening_ms = step_duration_ms(self.live, PreviewState::Listening);
        ((listening_ms / STREAM_CHUNK_INTERVAL.as_millis() as u64) as usize).max(1)
    }
}

fn cycle_total_ms(sequence: &[CycleStep]) -> u64 {
    sequence.iter().map(|step| step.duration_ms).sum()
}

/// How long `sequence` holds `state`, or 0 when it never visits it.
fn step_duration_ms(sequence: &[CycleStep], state: PreviewState) -> u64 {
    sequence
        .iter()
        .find(|step| step.state == Some(state))
        .map(|step| step.duration_ms)
        .unwrap_or(0)
}

/// Which step of `sequence` is showing `elapsed_ms` after the cycle started,
/// wrapping round for as long as the preview runs. `None` is the gap.
fn cycle_state_at(sequence: &[CycleStep], elapsed_ms: u64) -> Option<PreviewState> {
    let total = cycle_total_ms(sequence);
    if total == 0 {
        return None;
    }
    let mut remainder = elapsed_ms % total;
    for step in sequence {
        if remainder < step.duration_ms {
            return step.state;
        }
        remainder -= step.duration_ms;
    }
    // Unreachable. The remainder is less than the total the loop consumes.
    None
}

/// The overlay state a preview state is painted as, which is the `show-overlay`
/// payload the overlay window switches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayShow {
    Recording,
    Streaming,
    Transcribing,
    Processing,
}

/// What the Live panel does with the sample text in a given state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextFlow {
    /// Not a Live state, or nothing to say yet.
    None,
    /// Reveal it a few words at a time.
    Streaming,
    /// Hold the whole sentence, as a real finalize does.
    Held,
}

/// Everything one preview state needs pushed to the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Presentation {
    show: OverlayShow,
    /// Emit `recording-ready`. Microphone samples are flowing, so the card
    /// leaves the muted arming pill.
    ready: bool,
    /// Drive the waveform.
    levels: bool,
    text: TextFlow,
    /// The Live panel's phase. `None` under Minimal, which has a distinct
    /// window state per phase instead.
    phase: Option<(StreamPhase, Option<StreamWorkKind>)>,
}

/// How a state is painted, per style. The one table mapping the tab's chips
/// onto what the overlay actually understands.
fn presentation(style: OverlayStyle, state: PreviewState) -> Presentation {
    let live = style == OverlayStyle::Live;
    let base = Presentation {
        show: if live {
            OverlayShow::Streaming
        } else {
            OverlayShow::Recording
        },
        ready: true,
        levels: false,
        text: TextFlow::None,
        phase: None,
    };

    match normalize(style, state) {
        // `Cycle` never reaches here (the driver resolves it to a real state
        // first), but a state must be painted for it all the same.
        PreviewState::Cycle | PreviewState::Arming => Presentation {
            ready: false,
            phase: live.then_some((StreamPhase::Listening, None)),
            ..base
        },
        PreviewState::Recording => Presentation {
            levels: true,
            ..base
        },
        PreviewState::Listening => Presentation {
            levels: true,
            text: TextFlow::Streaming,
            phase: Some((StreamPhase::Listening, None)),
            ..base
        },
        PreviewState::Transcribing => Presentation {
            show: if live {
                OverlayShow::Streaming
            } else {
                OverlayShow::Transcribing
            },
            text: if live { TextFlow::Held } else { TextFlow::None },
            phase: live.then_some((StreamPhase::Working, Some(StreamWorkKind::Transcribing))),
            ..base
        },
        PreviewState::Processing => Presentation {
            show: if live {
                OverlayShow::Streaming
            } else {
                OverlayShow::Processing
            },
            text: if live { TextFlow::Held } else { TextFlow::None },
            phase: live.then_some((StreamPhase::Working, Some(StreamWorkKind::Polishing))),
            ..base
        },
    }
}

/// Whether a phase has to wait for the show that precedes it. The overlay page
/// handles `show-overlay` asynchronously, reading the settings and the resolved
/// theme before applying the state, then resets the Live panel to the listening
/// phase. That reset would overwrite a working phase emitted in the same breath
/// as the show, leaving the spinner state on a live waveform. A real session
/// records for seconds before finalizing, so it never hits this; a preview
/// pinned straight to `Transcribing` does and waits the show out. Nothing else
/// has to, because the listening phase is where the reset lands anyway.
fn phase_needs_settle(show_emitted: bool, phase: StreamPhase) -> bool {
    show_emitted && phase == StreamPhase::Working
}

/// Whether moving from `current` to `next` needs a fresh `show-overlay`.
/// Re-showing resets the card's capture readiness, clears the Live transcript
/// and replays the entrance, so it happens only when the window state changes
/// or the arming pill has to be replayed, the one thing only a show can do.
fn needs_show(style: OverlayStyle, current: Option<PreviewState>, next: PreviewState) -> bool {
    match current {
        None => true,
        Some(current) => {
            normalize(style, next) == PreviewState::Arming
                || presentation(style, current).show != presentation(style, next).show
        }
    }
}

/// What a style change under a running preview forces the driver to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleChange {
    /// The style the run started on is still the one in the settings.
    Keep,
    /// A different card, shown from the top of its own sequence.
    Replay,
    /// The overlay was turned off, so there is nothing left to preview.
    Stop,
}

/// The style-change decision, as a pure function of the two styles. The driver
/// follows the settings store rather than the tab. The Overlay group sits
/// directly above the preview card, so switching Minimal/Live there while a
/// preview runs has to change what is on screen. The tab sends nothing when the
/// pinned chip exists under both styles, so this is the only thing that
/// notices.
fn style_change(running: OverlayStyle, latest: OverlayStyle) -> StyleChange {
    if latest == OverlayStyle::None {
        StyleChange::Stop
    } else if latest == running {
        StyleChange::Keep
    } else {
        StyleChange::Replay
    }
}

/// Who a run belongs to, which decides what else can end it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewOwner {
    /// The Appearance tab.
    SettingsTab,
    /// A one-shot from the CLI, with nobody watching a window.
    OneShot,
}

/// Whether a run has outlived the window it was started from. Once the settings
/// window is off screen, whether closed to the tray, hidden or minimised,
/// nothing can stop the tab's preview from the UI and nobody is judging the
/// card, so the driver lets go. Watched here rather than only in the window's
/// close handler, because closing to the tray is one of several ways the window
/// goes away and the webview keeps running through all of them, so React never
/// learns. The one-shot has no window of its own and usually runs with the
/// settings window closed.
fn owner_left(owner: PreviewOwner, window_on_screen: bool) -> bool {
    owner == PreviewOwner::SettingsTab && !window_on_screen
}

/// Why a preview cannot start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewRefusal {
    /// Handy is recording. The preview drives the same overlay a real session
    /// uses, so it must never interrupt one.
    Recording,
    /// The overlay is turned off, so there is nothing to show.
    OverlayOff,
    /// A preview is already on screen.
    AlreadyRunning,
}

impl PreviewRefusal {
    /// The message returned to the caller. English, because it reaches the CLI
    /// and the log; the tab disables its button rather than rendering this.
    fn message(self) -> &'static str {
        match self {
            PreviewRefusal::Recording => "Cannot preview the overlay while recording",
            PreviewRefusal::OverlayOff => "The overlay is turned off (Overlay Style: None)",
            PreviewRefusal::AlreadyRunning => "An overlay preview is already running",
        }
    }
}

/// The guard, as a pure decision. May a preview start right now? Recording
/// outranks everything, so the refusal names the right reason rather than the
/// first problem found.
fn preview_refusal(
    is_recording: bool,
    style: OverlayStyle,
    preview_active: bool,
) -> Option<PreviewRefusal> {
    if is_recording {
        Some(PreviewRefusal::Recording)
    } else if style == OverlayStyle::None {
        Some(PreviewRefusal::OverlayOff)
    } else if preview_active {
        Some(PreviewRefusal::AlreadyRunning)
    } else {
        None
    }
}

/// Whether a preview owns the overlay right now. A preview on its way out still
/// owns it, because the overlay stays on screen until its driver takes it down.
fn is_previewing() -> bool {
    PREVIEW_GUARD.load(Ordering::SeqCst) != GUARD_IDLE
}

/// Whether the running preview has been asked to stop.
fn stop_requested() -> bool {
    PREVIEW_GUARD.load(Ordering::SeqCst) == GUARD_STOPPING
}

/// Whether a preview drives the overlay and has not been told to let go.
/// Stricter than [`is_previewing`], which answers "who owns the overlay" and so
/// counts a preview on its way out. This one answers "may the Appearance tab
/// still paint it", and a preview going away may not be, because its driver is
/// about to hide the window.
fn preview_running() -> bool {
    PREVIEW_GUARD.load(Ordering::SeqCst) == GUARD_RUNNING
}

/// May a theme draft repaint the overlay, given the two facts it depends on?
/// Pure, so the live-editing safety rule reads as one line. A draft is an
/// unsaved value the tab is showing the user, so it may only reach an overlay
/// the tab owns. A stopping preview no longer counts, its card being about to
/// come off screen. A real recording outranks everything, since it can take the
/// overlay at any moment and the card it puts up belongs to the session, not to
/// a slider.
fn draft_allowed(preview_running: bool, is_recording: bool) -> bool {
    preview_running && !is_recording
}

/// [`draft_allowed`] against the live guard and the recording manager.
pub(crate) fn accepts_theme_drafts(app: &AppHandle) -> bool {
    draft_allowed(preview_running(), recording_now(app))
}

/// End the running preview, if there is one. The driver notices within one
/// frame and hides the overlay itself, so ownership of the hide stays in one
/// place. Safe to call from anywhere, even when nothing is running. The
/// settings window closing, the app exiting and the cancel funnel all come
/// through here.
pub(crate) fn stop_preview() {
    // A no-op when idle, and equally when a stop is already on its way.
    let _ = PREVIEW_GUARD.compare_exchange(
        GUARD_RUNNING,
        GUARD_STOPPING,
        Ordering::SeqCst,
        Ordering::SeqCst,
    );
}

/// What a cancel must do, given the preview guard and whether Handy is
/// recording for real.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancelDisposition {
    /// No preview involved, so cancel the operation exactly as before.
    CancelOperation,
    /// A preview owns the overlay and nothing real is running, so the cancel is
    /// the preview's alone. Recording, model and coordinator stay untouched.
    EndPreviewOnly,
    /// A preview is on screen while a real recording is running, which the
    /// preview never starts but can be overtaken by. Both must end, and the
    /// real cancel is the one that must not be swallowed.
    EndPreviewAndCancel,
}

/// The cancel decision, as a pure function of the two facts it depends on.
fn cancel_disposition(preview_active: bool, is_recording: bool) -> CancelDisposition {
    match (preview_active, is_recording) {
        (false, _) => CancelDisposition::CancelOperation,
        (true, false) => CancelDisposition::EndPreviewOnly,
        (true, true) => CancelDisposition::EndPreviewAndCancel,
    }
}

/// What is left of a cancel once the preview has taken its share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CancelRemainder {
    /// A real operation still needs cancelling. Either nothing was previewing,
    /// or a real recording had already taken the overlay from the preview.
    CancelOperation,
    /// The cancel belonged to the preview alone. Recording, model and
    /// coordinator were never involved and must be left untouched.
    Handled,
}

/// Give the running preview, if any, first refusal on a cancel. The one call
/// the cancel funnel makes. It asks the guard, ends the preview when the cancel
/// is the preview's, and answers whether the caller still has something of its
/// own to cancel. One function rather than a guard read, a decision and a stop
/// the caller recombines, so the rule is stated once here. A real recording's
/// cancel is never swallowed.
pub(crate) fn take_cancel(is_recording: bool) -> CancelRemainder {
    match cancel_disposition(is_previewing(), is_recording) {
        CancelDisposition::CancelOperation => CancelRemainder::CancelOperation,
        CancelDisposition::EndPreviewOnly => {
            stop_preview();
            CancelRemainder::Handled
        }
        CancelDisposition::EndPreviewAndCancel => {
            stop_preview();
            CancelRemainder::CancelOperation
        }
    }
}

/// Holds the preview guard for as long as the driver runs. Releasing on `Drop`
/// rather than at the end of the driver keeps the guard from latching. The
/// driver thread can panic and the next preview still starts.
struct PreviewGuard;

impl Drop for PreviewGuard {
    fn drop(&mut self) {
        PREVIEW_GUARD.store(GUARD_IDLE, Ordering::SeqCst);
    }
}

/// Whether Handy is recording for real right now.
fn recording_now(app: &AppHandle) -> bool {
    app.try_state::<Arc<AudioRecordingManager>>()
        .map(|manager| manager.is_recording())
        .unwrap_or(false)
}

/// Whether the settings window is on screen right now. Unknown counts as on
/// screen, because a query that fails (the event loop is going away) must not
/// be the thing that takes a preview down.
fn settings_window_on_screen(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .map(|window| window.is_visible().unwrap_or(true))
        .unwrap_or(true)
}

/// Take the preview guard, or report why not. Waits out a previous driver still
/// letting go, for up to [`CLAIM_WAIT`]. Every other refusal comes back at
/// once, because none resolves by waiting.
fn claim_preview(app: &AppHandle, style: OverlayStyle) -> Result<PreviewGuard, PreviewRefusal> {
    let deadline = Instant::now() + CLAIM_WAIT;
    loop {
        match preview_refusal(recording_now(app), style, is_previewing()) {
            None => {
                // The check above can race a second call. The swap makes the
                // guard exclusive, and the same swap clears any stop left from
                // the run before. The guard is one state, so a stop can never
                // land between the two and be forgotten.
                if PREVIEW_GUARD
                    .compare_exchange(
                        GUARD_IDLE,
                        GUARD_RUNNING,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    )
                    .is_ok()
                {
                    return Ok(PreviewGuard);
                }
            }
            Some(PreviewRefusal::AlreadyRunning) => {}
            Some(refusal) => return Err(refusal),
        }
        if Instant::now() >= deadline {
            return Err(PreviewRefusal::AlreadyRunning);
        }
        std::thread::sleep(CLAIM_TICK);
    }
}

/// One sleep inside a settle: a whole tick, or whatever is left of it.
fn settle_slice(remaining: Duration) -> Duration {
    remaining.min(SETTLE_TICK)
}

/// Wait [`SHOW_SETTLE`] out, and report whether the run may carry on. Never one
/// blind sleep. A stop landing inside it would be held up for its whole length,
/// and a recording starting inside it would be handed the working phase the
/// settle exists to deliver, over an overlay that belongs to a real session by
/// then. Sleeping a tick before each check also bounds how fast the driver's
/// loop can spin when it gives up here.
fn settle(app: &AppHandle) -> bool {
    let deadline = Instant::now() + SHOW_SETTLE;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return true;
        }
        std::thread::sleep(settle_slice(remaining));
        if stop_requested() || recording_now(app) {
            return false;
        }
    }
}

/// 16 synthetic microphone buckets for one frame, each in `0.0..=1.0`. A
/// travelling wave under a fixed per-bar envelope, scaled by
/// [`synthetic_loudness`]. It reads as speech, not a test pattern, and every
/// bar moves, so the accent is easy to judge.
fn synthetic_levels(frame: u32) -> Vec<f32> {
    let loudness = synthetic_loudness(frame);
    (0..16)
        .map(|bucket| {
            let bar = bucket as f32;
            let travelling = ((frame as f32 * 0.25 - bar * 0.4).sin() + 1.0) * 0.5;
            let envelope = 0.45 + 0.55 * (bar * 0.55).cos().abs();
            (travelling * envelope * loudness).clamp(0.0, 1.0)
        })
        .collect()
}

/// How loud the whole waveform is on `frame`, `0.0..=1.0`: one slow breath and
/// one burst per [`LEVEL_CYCLE_FRAMES`].
///
/// Without it the preview never goes quiet and never peaks, and a waveform
/// style that lights up with loudness (`motes`, `matrix`) would only ever be
/// judged at its middle. The breath takes the card from near silence to a
/// speaking level and back; the burst is the one moment of full voice a cycle
/// gets. A small floor keeps the card from looking like a dropped microphone.
fn synthetic_loudness(frame: u32) -> f32 {
    let phase = (frame % LEVEL_CYCLE_FRAMES) as f32 / LEVEL_CYCLE_FRAMES as f32;
    let breath = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
    let burst = plateau(phase, BURST_START, BURST_END, BURST_EDGE);
    (BREATH_PEAK * breath).max(burst).max(QUIET_FLOOR)
}

/// A raised-cosine plateau: 0 outside `start..end`, 1 between the two `edge`
/// ramps, and a smooth ramp across each. One shape, so the burst arrives and
/// leaves rather than switching on.
fn plateau(phase: f32, start: f32, end: f32, edge: f32) -> f32 {
    if phase <= start || phase >= end {
        return 0.0;
    }
    let ramp =
        |travelled: f32| 0.5 - 0.5 * (travelled.clamp(0.0, 1.0) * std::f32::consts::PI).cos();
    ramp((phase - start) / edge).min(ramp((end - phase) / edge))
}

/// `count` progressively longer word prefixes of `text`, the last one complete.
/// The Live panel is meant to look like a transcript arriving, so the preview
/// reveals the sample sentence a few words at a time rather than all at once.
fn stream_chunks(text: &str, count: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if count == 0 || words.is_empty() {
        return Vec::new();
    }
    (1..=count)
        .map(|step| {
            // Ceiling division, so the last step always takes every word.
            let taken = (words.len() * step).div_ceil(count);
            words[..taken].join(" ")
        })
        .collect()
}

/// What the driver must do at the top of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameDecision {
    /// Keep painting.
    Continue,
    /// End the preview and hide the overlay it owns.
    Stop,
    /// A real recording took the overlay, so end the preview and leave the
    /// overlay alone. Hiding it would take down a session the user started.
    Preempted,
}

/// The frame decision, as a pure function of the three facts it depends on. A
/// real recording outranks a stop. Even a stop the user asked for must not hide
/// an overlay that no longer belongs to the preview.
fn frame_decision(
    is_recording: bool,
    stop_requested: bool,
    deadline_reached: bool,
) -> FrameDecision {
    if is_recording {
        FrameDecision::Preempted
    } else if stop_requested || deadline_reached {
        FrameDecision::Stop
    } else {
        FrameDecision::Continue
    }
}

/// How a preview run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewOutcome {
    /// Stopped by the tab, the cancel funnel, or its own deadline.
    Stopped,
    /// A real recording started while it was on screen.
    Preempted,
}

/// One preview run.
struct PreviewRun {
    /// Where the driver starts: `Cycle`, or the state to hold.
    initial: PreviewState,
    /// Which schedule drives the run.
    timing: Timing,
    /// Stop by itself after this long, which is the one-shot flag's whole
    /// behaviour. `None` runs until something stops it.
    auto_stop: Option<Duration>,
    /// Whose window the run ends with, if any.
    owner: PreviewOwner,
    sample_text: String,
    /// The driver sends on this once the run has ended, so an awaiting caller
    /// learns when the overlay is gone.
    done: Option<Sender<Result<(), String>>>,
}

/// Start a preview on its own OS thread and return as soon as it owns the
/// overlay. A plain `std::thread`, never the async runtime. This is reached
/// from the single-instance callback, which runs on a runtime worker parked in
/// a blocking accept loop, and a task spawned from there lands in that worker's
/// LIFO slot where no other worker may steal it (see `lib.rs`). The driver also
/// sleeps for as long as the preview lasts, which is no way to treat a worker.
fn start_preview(app: &AppHandle, run: PreviewRun) -> Result<(), String> {
    let style = get_settings(app).overlay_style;
    let guard = claim_preview(app, style).map_err(|refusal| {
        warn!("Overlay preview refused: {}", refusal.message());
        refusal.message().to_string()
    })?;

    PREVIEW_TARGET.store(normalize(style, run.initial).as_u8(), Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        let done = run.done.clone();
        let outcome = drive_preview(&app, style, run, guard);
        if let Some(done) = done {
            let _ = done.send(Ok(()));
        }
        debug!("Overlay preview ended: {outcome:?}");
    });
    Ok(())
}

/// The synthetic session itself, running until something ends it. `_guard` is
/// held for the whole run and released on drop, so a panic here still frees the
/// next preview.
fn drive_preview(
    app: &AppHandle,
    mut style: OverlayStyle,
    run: PreviewRun,
    _guard: PreviewGuard,
) -> PreviewOutcome {
    debug!("Overlay preview starting ({style:?}, {:?})", run.initial);
    let started = Instant::now();
    let mut sequence = run.timing.sequence(style);
    let chunks = stream_chunks(&run.sample_text, run.timing.chunk_count());

    // The cycle's own clock, restarted when the style changes under the preview
    // so the new card is replayed from its entrance rather than joining the old
    // one's sequence part-way through. The deadline keeps to `started`.
    let mut cycle_started = started;
    // What is on screen right now: the state last painted, or `None` while the
    // overlay is hidden (before the first frame, and during a cycle's gap).
    let mut shown: Option<PreviewState> = None;
    let mut shown_since = started;
    let mut frame: u32 = 0;
    let mut last_chunk: Option<usize> = None;
    // Refreshed on `POLL_INTERVAL`, starting with the first frame.
    let mut next_poll = started;
    let mut latest_style = style;
    let mut window_on_screen = true;

    let outcome = loop {
        let now = Instant::now();
        if now >= next_poll {
            next_poll = now + POLL_INTERVAL;
            latest_style = get_settings(app).overlay_style;
            window_on_screen = settings_window_on_screen(app);
        }

        let elapsed = started.elapsed();
        let deadline_reached = run.auto_stop.is_some_and(|limit| elapsed >= limit);
        match frame_decision(
            recording_now(app),
            stop_requested() || owner_left(run.owner, window_on_screen),
            deadline_reached,
        ) {
            FrameDecision::Continue => {}
            FrameDecision::Stop => break PreviewOutcome::Stopped,
            FrameDecision::Preempted => break PreviewOutcome::Preempted,
        }

        match style_change(style, latest_style) {
            StyleChange::Keep => {}
            StyleChange::Stop => break PreviewOutcome::Stopped,
            StyleChange::Replay => {
                debug!("Overlay preview following the overlay style to {latest_style:?}");
                style = latest_style;
                sequence = run.timing.sequence(style);
                cycle_started = Instant::now();
                // The other style's card is a different window state, so the
                // next frame has to show it rather than adjust it.
                shown = None;
                last_chunk = None;
            }
        }

        // Re-read every frame, because `set_overlay_preview_state` switches the
        // running preview without restarting it.
        let target = PreviewState::from_u8(PREVIEW_TARGET.load(Ordering::SeqCst));
        let wanted = match target {
            PreviewState::Cycle => {
                cycle_state_at(sequence, cycle_started.elapsed().as_millis() as u64)
            }
            pinned => Some(normalize(style, pinned)),
        };

        match wanted {
            None => {
                // The cycle's gap. Nothing on screen, so the next loop replays
                // the overlay's entrance exactly as a fresh session does.
                if shown.is_some() {
                    overlay::hide_recording_overlay(app);
                    shown = None;
                    last_chunk = None;
                }
            }
            Some(state) => {
                if shown != Some(state) {
                    if !apply_state(app, style, shown, state) {
                        // The settle inside was cut short. Straight back to the
                        // top, where the one decision that ends a run is made.
                        continue;
                    }
                    shown = Some(state);
                    shown_since = Instant::now();
                    last_chunk = None;
                }
                let present = presentation(style, state);
                if present.levels {
                    overlay::emit_levels(app, &synthetic_levels(frame));
                }
                emit_text(
                    app,
                    present.text,
                    &chunks,
                    shown_since.elapsed(),
                    &mut last_chunk,
                );
            }
        }

        frame = frame.wrapping_add(1);
        std::thread::sleep(FRAME_INTERVAL);
    };

    match outcome {
        PreviewOutcome::Preempted => {
            debug!("Overlay preview preempted by a real recording");
        }
        PreviewOutcome::Stopped => overlay::hide_recording_overlay(app),
    }
    outcome
}

/// Push one state to the overlay: the window state (only when it really
/// changes), capture readiness, and the Live panel's phase. Returns `false`
/// when the settle before a working phase was cut short because the run has to
/// end. The caller's own frame decision then ends it.
fn apply_state(
    app: &AppHandle,
    style: OverlayStyle,
    current: Option<PreviewState>,
    next: PreviewState,
) -> bool {
    let present = presentation(style, next);
    let show_emitted = needs_show(style, current, next);
    if show_emitted {
        match present.show {
            OverlayShow::Recording => overlay::show_recording_overlay(app),
            OverlayShow::Streaming => overlay::show_streaming_overlay(app),
            OverlayShow::Transcribing => overlay::show_transcribing_overlay(app),
            OverlayShow::Processing => overlay::show_processing_overlay(app),
        }
    }
    if present.ready {
        // The first real microphone chunk does exactly this, leaving the arming
        // pill. Queued on the main thread behind any show above, so no show can
        // undo it.
        overlay::emit_recording_ready(app);
    }
    if let Some((phase, kind)) = present.phase {
        if phase_needs_settle(show_emitted, phase) && !settle(app) {
            return false;
        }
        if let Err(error) = (StreamPhaseEvent { phase, kind }).emit(app) {
            warn!("Overlay preview could not emit its phase: {error}");
        }
    }
    true
}

/// Feed the Live panel: a longer prefix every [`STREAM_CHUNK_INTERVAL`] while
/// listening, the whole sentence at once while the spinner is up.
fn emit_text(
    app: &AppHandle,
    flow: TextFlow,
    chunks: &[String],
    elapsed_in_state: Duration,
    last_chunk: &mut Option<usize>,
) {
    if chunks.is_empty() {
        return;
    }
    let index = match flow {
        TextFlow::None => return,
        TextFlow::Held => chunks.len() - 1,
        TextFlow::Streaming => ((elapsed_in_state.as_millis() as u64
            / STREAM_CHUNK_INTERVAL.as_millis() as u64) as usize)
            .min(chunks.len() - 1),
    };
    if *last_chunk == Some(index) {
        return;
    }
    *last_chunk = Some(index);
    if let Err(error) = (StreamTextEvent {
        committed: chunks[index].clone(),
        tentative: String::new(),
    })
    .emit(app)
    {
        warn!("Overlay preview could not emit its sample text: {error}");
    }
}

/// Show the real overlay and keep it there, cycling or pinned, until something
/// stops it.
///
/// `sample_text` is the Live panel's transcript, passed in already translated
/// so i18n stays entirely on the frontend. `None` falls back to the built-in
/// English sentence. Returns as soon as the overlay is up. The tab keeps
/// editing tokens while it runs, and every change repaints the overlay live.
///
/// Blocking, because the claim inside waits briefly for a previous driver to
/// let go, so the command adapter runs this off the main thread.
pub(crate) fn start(
    app: &AppHandle,
    state: PreviewState,
    sample_text: Option<String>,
) -> Result<(), String> {
    start_preview(
        app,
        PreviewRun {
            initial: state,
            timing: PERSISTENT_TIMING,
            auto_stop: None,
            owner: PreviewOwner::SettingsTab,
            sample_text: sample_text.unwrap_or_else(|| PREVIEW_SAMPLE_TEXT.to_string()),
            done: None,
        },
    )
}

/// Set which state the running preview shows, without restarting the driver.
/// Always writes the target, whether or not a preview is running. The tab can
/// race a preview the backend already ended (a real recording took the
/// overlay), and remembering the pin costs nothing. The next start reads it
/// anyway, and no driver reads it while none is running.
pub(crate) fn pin_state(state: PreviewState) {
    PREVIEW_TARGET.store(state.as_u8(), Ordering::SeqCst);
}

/// The `--preview-overlay` CLI flag, one compressed pass of the preview. Called
/// from the single-instance callback on a plain thread of its own, so it runs
/// the whole thing to completion rather than returning early.
pub(crate) fn run_cli_preview(app: AppHandle) -> Result<(), String> {
    let (done, finished) = std::sync::mpsc::channel();
    start_preview(
        &app,
        PreviewRun {
            initial: PreviewState::Cycle,
            timing: ONE_SHOT_TIMING,
            auto_stop: Some(ONE_SHOT_DURATION),
            owner: PreviewOwner::OneShot,
            sample_text: PREVIEW_SAMPLE_TEXT.to_string(),
            done: Some(done),
        },
    )?;
    finished.recv().unwrap_or(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_outranks_every_other_refusal() {
        assert_eq!(
            preview_refusal(true, OverlayStyle::None, true),
            Some(PreviewRefusal::Recording)
        );
        assert_eq!(
            preview_refusal(true, OverlayStyle::Minimal, false),
            Some(PreviewRefusal::Recording)
        );
    }

    #[test]
    fn a_disabled_overlay_has_nothing_to_preview() {
        assert_eq!(
            preview_refusal(false, OverlayStyle::None, false),
            Some(PreviewRefusal::OverlayOff)
        );
    }

    #[test]
    fn two_previews_never_overlap() {
        assert_eq!(
            preview_refusal(false, OverlayStyle::Live, true),
            Some(PreviewRefusal::AlreadyRunning)
        );
    }

    #[test]
    fn an_idle_handy_with_an_overlay_may_preview() {
        assert_eq!(preview_refusal(false, OverlayStyle::Minimal, false), None);
        assert_eq!(preview_refusal(false, OverlayStyle::Live, false), None);
    }

    #[test]
    fn a_cancel_only_belongs_to_the_preview_when_nothing_real_is_running() {
        assert_eq!(
            cancel_disposition(false, false),
            CancelDisposition::CancelOperation
        );
        assert_eq!(
            cancel_disposition(false, true),
            CancelDisposition::CancelOperation
        );
        assert_eq!(
            cancel_disposition(true, false),
            CancelDisposition::EndPreviewOnly
        );
        // The one that matters: a real recording's cancel is never swallowed.
        assert_eq!(
            cancel_disposition(true, true),
            CancelDisposition::EndPreviewAndCancel
        );
    }

    /// A draft only ever repaints an overlay the tab still owns, neither one a
    /// recording has taken nor one whose preview is on its way out (the `false`
    /// column, which `preview_running` supplies for a stopping guard).
    #[test]
    fn a_draft_may_only_repaint_a_running_preview_nothing_is_recording_over() {
        assert!(draft_allowed(true, false));
        // Pre-empted, so the recording owns the overlay from this moment on.
        assert!(!draft_allowed(true, true));
        // Idle or stopping, so nothing of the tab's is on screen to paint.
        assert!(!draft_allowed(false, false));
        assert!(!draft_allowed(false, true));
    }

    /// The guard is process-wide, so everything touching it is one test rather
    /// than several racing each other, and it is left exactly as found.
    #[test]
    fn the_guard_records_a_stop_and_is_released_even_if_the_driver_never_finishes() {
        assert!(!is_previewing());
        stop_preview(); // no-op while idle
        assert!(!stop_requested());

        {
            PREVIEW_GUARD.store(GUARD_RUNNING, Ordering::SeqCst);
            let _guard = PreviewGuard;
            // A running preview is the one state the Appearance tab may paint.
            assert!(preview_running());
            stop_preview();
            // ...and a stopping one is not, because the driver is about to hide
            // the window, so a draft now would paint a card on its way out.
            assert!(!preview_running());
            assert!(
                is_previewing(),
                "a stopping preview still owns the overlay, it just cannot be drafted onto"
            );
            // The same atomic that says a preview is running also records the
            // stop, so a start that is claiming cannot clear it.
            assert!(stop_requested());
            assert!(
                is_previewing(),
                "a preview on its way out still owns the overlay"
            );
            // A second stop changes nothing.
            stop_preview();
            assert!(stop_requested());
            // Dropped here without any explicit release, as a panicked driver
            // thread would be.
        }

        assert!(!is_previewing());
        assert!(!stop_requested());
    }

    #[test]
    fn a_preview_state_survives_the_atomic_it_is_stored_in() {
        for state in [
            PreviewState::Cycle,
            PreviewState::Arming,
            PreviewState::Recording,
            PreviewState::Listening,
            PreviewState::Transcribing,
            PreviewState::Processing,
        ] {
            assert_eq!(PreviewState::from_u8(state.as_u8()), state);
        }
        // Anything unrecognised falls back to the loop, not a half-valid pin.
        assert_eq!(PreviewState::from_u8(200), PreviewState::Cycle);
    }

    #[test]
    fn the_two_capture_states_are_one_state_under_two_names() {
        assert_eq!(
            normalize(OverlayStyle::Live, PreviewState::Recording),
            PreviewState::Listening
        );
        assert_eq!(
            normalize(OverlayStyle::Minimal, PreviewState::Listening),
            PreviewState::Recording
        );
        // Every other state is the same under both styles.
        for state in [
            PreviewState::Arming,
            PreviewState::Transcribing,
            PreviewState::Processing,
        ] {
            assert_eq!(normalize(OverlayStyle::Live, state), state);
            assert_eq!(normalize(OverlayStyle::Minimal, state), state);
        }
    }

    #[test]
    fn the_minimal_cycle_visits_every_state_it_has_and_then_a_gap() {
        let seq = PERSISTENT_TIMING.sequence(OverlayStyle::Minimal);
        let at = |ms| cycle_state_at(seq, ms);
        assert_eq!(at(0), Some(PreviewState::Arming));
        assert_eq!(at(799), Some(PreviewState::Arming));
        assert_eq!(at(800), Some(PreviewState::Recording));
        assert_eq!(at(4_499), Some(PreviewState::Recording));
        assert_eq!(at(4_500), Some(PreviewState::Transcribing));
        assert_eq!(at(6_100), Some(PreviewState::Processing));
        assert_eq!(at(7_500), None, "the gap hides the overlay");
        // ... and then round again, for as long as the preview runs.
        assert_eq!(at(8_100), Some(PreviewState::Arming));
        assert_eq!(at(8_100 * 42 + 900), Some(PreviewState::Recording));
    }

    #[test]
    fn the_live_cycle_streams_text_rather_than_switching_windows() {
        let seq = PERSISTENT_TIMING.sequence(OverlayStyle::Live);
        let at = |ms| cycle_state_at(seq, ms);
        assert_eq!(at(0), Some(PreviewState::Arming));
        assert_eq!(at(800), Some(PreviewState::Listening));
        assert_eq!(at(5_200), Some(PreviewState::Transcribing));
        assert_eq!(at(7_100), None);
        assert_eq!(at(7_700), Some(PreviewState::Arming));

        // Live never leaves the streaming window, whatever state it shows.
        for state in [
            PreviewState::Arming,
            PreviewState::Listening,
            PreviewState::Transcribing,
            PreviewState::Processing,
        ] {
            assert_eq!(
                presentation(OverlayStyle::Live, state).show,
                OverlayShow::Streaming
            );
        }
    }

    /// Where a sequence first reaches `state`, in ms from its start.
    fn starts_at(sequence: &[CycleStep], state: PreviewState) -> Option<u64> {
        let mut at = 0;
        for step in sequence {
            if step.state == Some(state) {
                return Some(at);
            }
            at += step.duration_ms;
        }
        None
    }

    #[test]
    fn the_one_shot_shows_a_whole_session_before_its_deadline() {
        let deadline_ms = ONE_SHOT_DURATION.as_millis() as u64;
        let chunk_ms = STREAM_CHUNK_INTERVAL.as_millis() as u64;

        for style in [OverlayStyle::Minimal, OverlayStyle::Live] {
            let seq = ONE_SHOT_TIMING.sequence(style);
            // The arming pill is held for a moment and capture then starts,
            // which is what the flag did before preview mode existed.
            assert_eq!(seq[0].state, Some(PreviewState::Arming));
            assert_eq!(seq[0].duration_ms, 120);
            // No gap, and longer than the deadline, so the run is hidden from
            // its last state rather than wrapping round to replay the entrance.
            assert!(seq.iter().all(|step| step.state.is_some()));
            assert!(cycle_total_ms(seq) >= deadline_ms);
            // The spinner is reached with time to be seen.
            let transcribing_at = starts_at(seq, PreviewState::Transcribing)
                .expect("the one-shot visits transcribing");
            assert!(transcribing_at < deadline_ms);
            assert_eq!(
                cycle_state_at(seq, transcribing_at),
                Some(PreviewState::Transcribing)
            );
        }

        // ... and in Live the whole sample sentence is on screen before the
        // panel stops listening, so the spinner holds a finished transcript.
        let live = ONE_SHOT_TIMING.sequence(OverlayStyle::Live);
        let listening_at = starts_at(live, PreviewState::Listening).unwrap();
        let last_chunk_at = listening_at + (ONE_SHOT_TIMING.chunk_count() as u64 - 1) * chunk_ms;
        assert!(
            last_chunk_at < starts_at(live, PreviewState::Transcribing).unwrap(),
            "the sentence must finish inside the listening step"
        );
        assert!(last_chunk_at < deadline_ms);

        // Which is exactly what the persistent cycle cannot do. On its own
        // schedule the panel is still listening when the deadline arrives.
        assert_eq!(
            cycle_state_at(
                PERSISTENT_TIMING.sequence(OverlayStyle::Live),
                deadline_ms - 1
            ),
            Some(PreviewState::Listening)
        );
    }

    #[test]
    fn the_driver_follows_the_overlay_style_it_is_previewing() {
        assert_eq!(
            style_change(OverlayStyle::Live, OverlayStyle::Live),
            StyleChange::Keep
        );
        // The group above the preview card switched styles under a running
        // preview. The other card is a different window state, so it has to be
        // replayed rather than adjusted.
        assert_eq!(
            style_change(OverlayStyle::Live, OverlayStyle::Minimal),
            StyleChange::Replay
        );
        assert_eq!(
            style_change(OverlayStyle::Minimal, OverlayStyle::Live),
            StyleChange::Replay
        );
        // Turning the overlay off leaves nothing to preview.
        assert_eq!(
            style_change(OverlayStyle::Live, OverlayStyle::None),
            StyleChange::Stop
        );
        assert_eq!(
            style_change(OverlayStyle::None, OverlayStyle::None),
            StyleChange::Stop
        );
        // ... and the sequence that plays next is the new style's own.
        assert_eq!(
            PERSISTENT_TIMING.sequence(OverlayStyle::Minimal),
            MINIMAL_CYCLE
        );
        assert_eq!(PERSISTENT_TIMING.sequence(OverlayStyle::Live), LIVE_CYCLE);
    }

    #[test]
    fn only_the_tabs_preview_ends_with_the_settings_window() {
        assert!(owner_left(PreviewOwner::SettingsTab, false));
        assert!(!owner_left(PreviewOwner::SettingsTab, true));
        // The CLI one-shot usually runs with no settings window on screen at
        // all, so its own deadline is the only thing that ends it.
        assert!(!owner_left(PreviewOwner::OneShot, false));
        assert!(!owner_left(PreviewOwner::OneShot, true));
    }

    #[test]
    fn minimal_paints_each_state_as_its_own_overlay_window_state() {
        let show = |state| presentation(OverlayStyle::Minimal, state).show;
        assert_eq!(show(PreviewState::Arming), OverlayShow::Recording);
        assert_eq!(show(PreviewState::Recording), OverlayShow::Recording);
        assert_eq!(show(PreviewState::Transcribing), OverlayShow::Transcribing);
        assert_eq!(show(PreviewState::Processing), OverlayShow::Processing);
    }

    #[test]
    fn only_arming_holds_the_card_before_capture() {
        for style in [OverlayStyle::Minimal, OverlayStyle::Live] {
            assert!(!presentation(style, PreviewState::Arming).ready);
            for state in [
                PreviewState::Recording,
                PreviewState::Listening,
                PreviewState::Transcribing,
                PreviewState::Processing,
            ] {
                assert!(presentation(style, state).ready);
            }
        }
    }

    #[test]
    fn the_waveform_runs_only_while_the_preview_is_capturing() {
        assert!(presentation(OverlayStyle::Minimal, PreviewState::Recording).levels);
        assert!(presentation(OverlayStyle::Live, PreviewState::Listening).levels);
        for state in [
            PreviewState::Arming,
            PreviewState::Transcribing,
            PreviewState::Processing,
        ] {
            assert!(!presentation(OverlayStyle::Minimal, state).levels);
            assert!(!presentation(OverlayStyle::Live, state).levels);
        }
    }

    #[test]
    fn live_holds_the_transcript_through_the_working_states() {
        assert_eq!(
            presentation(OverlayStyle::Live, PreviewState::Listening).text,
            TextFlow::Streaming
        );
        assert_eq!(
            presentation(OverlayStyle::Live, PreviewState::Transcribing).text,
            TextFlow::Held
        );
        assert_eq!(
            presentation(OverlayStyle::Live, PreviewState::Processing).text,
            TextFlow::Held
        );
        // Minimal has no transcript to hold.
        assert_eq!(
            presentation(OverlayStyle::Minimal, PreviewState::Transcribing).text,
            TextFlow::None
        );
    }

    #[test]
    fn the_live_spinner_names_which_kind_of_work_is_running() {
        assert_eq!(
            presentation(OverlayStyle::Live, PreviewState::Transcribing).phase,
            Some((StreamPhase::Working, Some(StreamWorkKind::Transcribing)))
        );
        assert_eq!(
            presentation(OverlayStyle::Live, PreviewState::Processing).phase,
            Some((StreamPhase::Working, Some(StreamWorkKind::Polishing)))
        );
        assert_eq!(
            presentation(OverlayStyle::Live, PreviewState::Listening).phase,
            Some((StreamPhase::Listening, None))
        );
        // Minimal switches windows instead of phases.
        assert_eq!(
            presentation(OverlayStyle::Minimal, PreviewState::Transcribing).phase,
            None
        );
    }

    #[test]
    fn the_overlay_is_re_shown_only_when_it_has_to_be() {
        // Nothing on screen yet.
        assert!(needs_show(
            OverlayStyle::Live,
            None,
            PreviewState::Listening
        ));
        // Only a show replays arming.
        assert!(needs_show(
            OverlayStyle::Live,
            Some(PreviewState::Listening),
            PreviewState::Arming
        ));
        // Live's phases share one window, so a phase change must not re-show.
        // That would clear the transcript and replay the entrance.
        assert!(!needs_show(
            OverlayStyle::Live,
            Some(PreviewState::Listening),
            PreviewState::Transcribing
        ));
        assert!(!needs_show(
            OverlayStyle::Live,
            Some(PreviewState::Transcribing),
            PreviewState::Listening
        ));
        // Minimal's states are separate windows, so each one is a show.
        assert!(needs_show(
            OverlayStyle::Minimal,
            Some(PreviewState::Recording),
            PreviewState::Transcribing
        ));
        assert!(needs_show(
            OverlayStyle::Minimal,
            Some(PreviewState::Transcribing),
            PreviewState::Processing
        ));
        // Arming and recording are the same window under Minimal, but arming
        // still needs the show that resets capture readiness.
        assert!(needs_show(
            OverlayStyle::Minimal,
            Some(PreviewState::Recording),
            PreviewState::Arming
        ));
        assert!(!needs_show(
            OverlayStyle::Minimal,
            Some(PreviewState::Arming),
            PreviewState::Recording
        ));
    }

    #[test]
    fn a_real_recording_ends_the_preview_without_taking_the_overlay_down() {
        // Pre-emption outranks a stop. The overlay belongs to the recording
        // now, so even a stop the user asked for must not hide it.
        assert_eq!(frame_decision(true, false, false), FrameDecision::Preempted);
        assert_eq!(frame_decision(true, true, true), FrameDecision::Preempted);
    }

    #[test]
    fn a_stop_and_a_deadline_both_end_the_preview() {
        assert_eq!(frame_decision(false, true, false), FrameDecision::Stop);
        assert_eq!(frame_decision(false, false, true), FrameDecision::Stop);
        assert_eq!(frame_decision(false, false, false), FrameDecision::Continue);
    }

    #[test]
    fn a_working_phase_waits_out_the_show_it_follows() {
        // The case that showed up on screen. A preview pinned straight to
        // Transcribing painted a listening card, because the overlay page's own
        // show handler resets the phase once its async reads finish.
        assert!(phase_needs_settle(true, StreamPhase::Working));
        // Nothing else waits, because the listening phase is where that reset
        // lands anyway, and a phase change without a show has nothing to race.
        assert!(!phase_needs_settle(true, StreamPhase::Listening));
        assert!(!phase_needs_settle(false, StreamPhase::Working));
        assert!(!phase_needs_settle(false, StreamPhase::Listening));
    }

    #[test]
    fn the_settle_is_slept_in_ticks_rather_than_in_one_go() {
        // A blind 300 ms sleep would hold a stop up for its whole length and
        // hand a recording starting inside it the working phase it delivers.
        let mut remaining = SHOW_SETTLE;
        let mut slices = Vec::new();
        while !remaining.is_zero() {
            let slice = settle_slice(remaining);
            assert!(slice <= SETTLE_TICK);
            assert!(!slice.is_zero(), "a zero slice would spin");
            remaining -= slice;
            slices.push(slice);
        }
        assert_eq!(slices.iter().sum::<Duration>(), SHOW_SETTLE);
        assert!(
            slices.len() >= 10,
            "{} sleeps is not a tick loop",
            slices.len()
        );
    }

    #[test]
    fn synthetic_levels_fill_sixteen_buckets_and_move() {
        let first = synthetic_levels(0);
        assert_eq!(first.len(), 16);
        assert!(first.iter().all(|level| (0.0..=1.0).contains(level)));
        assert_ne!(
            format!("{first:?}"),
            format!("{:?}", synthetic_levels(4)),
            "the waveform must animate between frames"
        );
    }

    /// The preview has to walk the whole range, because a waveform style that
    /// lights up with loudness (`motes`, `matrix`) is only judged by its
    /// quiet frame and its loud one. One cycle must contain both.
    #[test]
    fn one_level_cycle_goes_quiet_and_bursts() {
        let peak = |frame: u32| {
            synthetic_levels(frame)
                .into_iter()
                .fold(0.0f32, |highest, level| highest.max(level))
        };
        let peaks: Vec<f32> = (0..LEVEL_CYCLE_FRAMES).map(peak).collect();
        let quietest = peaks.iter().copied().fold(f32::MAX, f32::min);
        let loudest = peaks.iter().copied().fold(0.0f32, f32::max);

        assert!(quietest < 0.15, "no quiet frame in a cycle: {quietest}");
        assert!(loudest > 0.85, "no loud frame in a cycle: {loudest}");
        // Never silent, so no style ever draws an empty lane.
        assert!(peaks.iter().all(|peak| *peak > 0.0));
    }

    /// The burst is a shape, not a switch: it ramps in, holds and ramps out.
    #[test]
    fn the_burst_ramps_in_and_out_of_its_plateau() {
        assert_eq!(
            plateau(BURST_START - 0.01, BURST_START, BURST_END, BURST_EDGE),
            0.0
        );
        assert_eq!(
            plateau(BURST_END + 0.01, BURST_START, BURST_END, BURST_EDGE),
            0.0
        );
        let middle = plateau(
            (BURST_START + BURST_END) / 2.0,
            BURST_START,
            BURST_END,
            BURST_EDGE,
        );
        assert_eq!(middle, 1.0);
        let entering = plateau(
            BURST_START + BURST_EDGE / 2.0,
            BURST_START,
            BURST_END,
            BURST_EDGE,
        );
        assert!(entering > 0.0 && entering < 1.0, "{entering}");
    }

    #[test]
    fn stream_chunks_grow_to_the_whole_sentence() {
        let chunks = stream_chunks("one two three four five six", 3);
        assert_eq!(
            chunks,
            vec![
                "one two",
                "one two three four",
                "one two three four five six"
            ]
        );

        // Degenerate inputs produce nothing rather than an empty first chunk.
        assert!(stream_chunks("anything", 0).is_empty());
        assert!(stream_chunks("   ", 4).is_empty());

        // The last chunk is always the complete text, whatever the step count.
        for count in 1..=8 {
            let chunks = stream_chunks(PREVIEW_SAMPLE_TEXT, count);
            assert_eq!(chunks.len(), count);
            assert_eq!(chunks[count - 1], PREVIEW_SAMPLE_TEXT);
        }
    }
}
