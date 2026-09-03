//! Showing the real overlay on demand, with no microphone involved.
//!
//! The Appearance tab's "Show on screen" button and the `--preview-overlay` CLI
//! flag both land here: the overlay is shown in the user's configured style,
//! driven for a few seconds by synthetic microphone levels (and, in Live, by a
//! synthetic transcript), then hidden. It is the only way to judge the overlay
//! theme at its true size, and the only way to see the Material actually
//! rendered.
//!
//! Nothing here may disturb a real session: the preview refuses to start while
//! Handy is recording, and while it runs it is the only thing a cancel touches.

use crate::managers::audio::AudioRecordingManager;
use crate::managers::transcription::StreamTextEvent;
use crate::overlay;
use crate::settings::{get_settings, OverlayStyle};
use log::{debug, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

/// The sample sentence the CLI flag shows in the Live panel.
///
/// The command takes its text from the frontend, already translated. The CLI
/// has no webview to ask, so it uses this. Deliberately English: no tray
/// translation carries a sample sentence, and inventing one would mean a new
/// key in all 24 locale files for a developer-facing flag.
pub const CLI_PREVIEW_SAMPLE_TEXT: &str =
    "The quick brown fox jumps over the lazy dog, and Handy writes it down.";

/// A preview is running. Guards against overlap, and tells `cancel_operation`
/// that a cancel belongs to the preview rather than to a real session.
static PREVIEW_ACTIVE: AtomicBool = AtomicBool::new(false);

/// A cancel arrived for the running preview. The driver checks it between
/// frames and ends the preview early.
static PREVIEW_CANCELLED: AtomicBool = AtomicBool::new(false);

/// Wait before leaving the arming state, so the muted arming pill is visible
/// for a moment exactly as it is in a real session.
const READY_DELAY: Duration = Duration::from_millis(120);

/// One synthetic level frame per this interval.
///
/// 40 ms, not 33: `overlay::emit_levels` drops any frame arriving less than
/// `EMIT_THROTTLE_MS` (33 ms) after the last one, so a faster cadence would
/// halve to ~50 ms.
const LEVEL_INTERVAL: Duration = Duration::from_millis(40);

/// How long the synthetic levels run.
const LEVELS_DURATION: Duration = Duration::from_millis(3200);

/// The Live panel receives another piece of the sample text every this many
/// level frames — six frames of 40 ms, i.e. 240 ms.
const STREAM_CHUNK_FRAMES: u32 = 6;

/// Why a preview cannot start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRefusal {
    /// Handy is recording. The preview drives the same overlay a real session
    /// uses, so it must never interrupt one.
    Recording,
    /// The overlay is turned off, so there is nothing to show.
    OverlayOff,
    /// A preview is already on screen.
    AlreadyRunning,
}

impl PreviewRefusal {
    /// The message returned to the caller. English: it reaches the CLI and the
    /// log; the tab disables its button rather than rendering this.
    pub fn message(self) -> &'static str {
        match self {
            PreviewRefusal::Recording => "Cannot preview the overlay while recording",
            PreviewRefusal::OverlayOff => "The overlay is turned off (Overlay Style: None)",
            PreviewRefusal::AlreadyRunning => "An overlay preview is already running",
        }
    }
}

/// The guard, as a pure decision: may a preview start right now?
///
/// Recording outranks everything — refusing for the right reason matters more
/// than reporting the first problem found.
pub fn preview_refusal(
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

/// Whether a preview owns the overlay right now.
pub fn is_previewing() -> bool {
    PREVIEW_ACTIVE.load(Ordering::SeqCst)
}

/// End the running preview, if there is one.
///
/// The driver notices the flag within one frame interval and hides the overlay
/// itself, so ownership of the hide stays in one place.
pub fn cancel_preview() {
    if is_previewing() {
        PREVIEW_CANCELLED.store(true, Ordering::SeqCst);
    }
}

/// What a cancel must do, given the preview guard and whether Handy is
/// recording for real.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelDisposition {
    /// No preview involved: cancel the operation exactly as before.
    CancelOperation,
    /// A preview owns the overlay and nothing real is running, so the cancel is
    /// the preview's alone — recording, model and coordinator stay untouched.
    EndPreviewOnly,
    /// A preview is on screen *and* a real recording is running, which the
    /// preview never starts but can be overtaken by. Both must end, and the
    /// real cancel is the one that must not be swallowed.
    EndPreviewAndCancel,
}

/// The cancel decision, as a pure function of the two facts it depends on.
pub fn cancel_disposition(preview_active: bool, is_recording: bool) -> CancelDisposition {
    match (preview_active, is_recording) {
        (false, _) => CancelDisposition::CancelOperation,
        (true, false) => CancelDisposition::EndPreviewOnly,
        (true, true) => CancelDisposition::EndPreviewAndCancel,
    }
}

/// Holds the preview guard for as long as the driver runs.
///
/// Releasing on `Drop` rather than at the end of the driver is what keeps the
/// guard from latching: the future can be dropped (the command's webview goes
/// away) or panic, and the next preview still starts.
struct PreviewGuard;

impl Drop for PreviewGuard {
    fn drop(&mut self) {
        PREVIEW_CANCELLED.store(false, Ordering::SeqCst);
        PREVIEW_ACTIVE.store(false, Ordering::SeqCst);
    }
}

/// Whether Handy is recording for real right now.
fn recording_now(app: &AppHandle) -> bool {
    app.try_state::<Arc<AudioRecordingManager>>()
        .map(|manager| manager.is_recording())
        .unwrap_or(false)
}

/// Take the preview guard, or report why not.
fn claim_preview(app: &AppHandle, style: OverlayStyle) -> Result<PreviewGuard, PreviewRefusal> {
    if let Some(refusal) = preview_refusal(recording_now(app), style, is_previewing()) {
        return Err(refusal);
    }

    // The check above can race a second call; the swap is what actually makes
    // the guard exclusive.
    PREVIEW_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| PreviewRefusal::AlreadyRunning)?;
    PREVIEW_CANCELLED.store(false, Ordering::SeqCst);
    Ok(PreviewGuard)
}

/// 16 synthetic microphone buckets for one frame, each in `0.0..=1.0`.
///
/// A travelling wave under a fixed per-bar envelope: it reads as speech rather
/// than as a test pattern, and every bar moves, so the accent is easy to judge.
pub fn synthetic_levels(frame: u32) -> Vec<f32> {
    (0..16)
        .map(|bucket| {
            let bar = bucket as f32;
            let travelling = ((frame as f32 * 0.25 - bar * 0.4).sin() + 1.0) * 0.5;
            let envelope = 0.45 + 0.55 * (bar * 0.55).cos().abs();
            (travelling * envelope).clamp(0.0, 1.0)
        })
        .collect()
}

/// `count` progressively longer word prefixes of `text`, the last one complete.
///
/// The Live panel is meant to look like a transcript arriving, so the preview
/// reveals the sample sentence a few words at a time rather than all at once.
pub fn stream_chunks(text: &str, count: usize) -> Vec<String> {
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

/// Show the overlay and drive it through a synthetic session.
///
/// Shared by the `preview_overlay_on_screen` command and the `--preview-overlay`
/// CLI flag. Awaits the whole sequence (about 3.5 s) so the caller can keep its
/// button busy for exactly as long as the overlay is up.
pub async fn run_overlay_preview(app: AppHandle, sample_text: String) -> Result<(), String> {
    let style = get_settings(&app).overlay_style;
    let _guard = claim_preview(&app, style).map_err(|refusal| {
        warn!("Overlay preview refused: {}", refusal.message());
        refusal.message().to_string()
    })?;

    debug!("Overlay preview starting ({:?})", style);
    let live = style == OverlayStyle::Live;
    if live {
        overlay::show_streaming_overlay(&app);
    } else {
        overlay::show_recording_overlay(&app);
    }

    // A preview that was overtaken by a real recording must leave the overlay
    // alone: the recording owns it now, and hiding it would take down a session
    // the user actually started.
    if drive_preview(&app, &sample_text, live).await == PreviewOutcome::Preempted {
        debug!("Overlay preview preempted by a real recording");
    } else {
        overlay::hide_recording_overlay(&app);
        debug!("Overlay preview finished");
    }
    Ok(())
}

/// How a preview ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewOutcome {
    /// The synthetic session ran to the end.
    Completed,
    /// A cancel arrived for it.
    Cancelled,
    /// A real recording started while it was on screen.
    Preempted,
}

/// The synthetic session itself: arming, then levels (and text, in Live).
async fn drive_preview(app: &AppHandle, sample_text: &str, live: bool) -> PreviewOutcome {
    tokio::time::sleep(READY_DELAY).await;
    if PREVIEW_CANCELLED.load(Ordering::SeqCst) {
        return PreviewOutcome::Cancelled;
    }
    // Leave the arming state, exactly as the first real microphone chunk does.
    overlay::emit_recording_ready(app);

    let frames = (LEVELS_DURATION.as_millis() / LEVEL_INTERVAL.as_millis()) as u32;
    let chunks = if live {
        stream_chunks(sample_text, (frames / STREAM_CHUNK_FRAMES) as usize)
    } else {
        Vec::new()
    };

    for frame in 0..frames {
        if PREVIEW_CANCELLED.load(Ordering::SeqCst) {
            debug!("Overlay preview cancelled");
            return PreviewOutcome::Cancelled;
        }
        // Checked every frame, not only at the start: a shortcut press during
        // the preview starts a real session, and from that moment the synthetic
        // levels would be drawing over someone's actual recording.
        if recording_now(app) {
            return PreviewOutcome::Preempted;
        }
        overlay::emit_levels(app, &synthetic_levels(frame));

        if frame % STREAM_CHUNK_FRAMES == 0 {
            if let Some(text) = chunks.get((frame / STREAM_CHUNK_FRAMES) as usize) {
                if let Err(error) = (StreamTextEvent {
                    committed: text.clone(),
                    tentative: String::new(),
                })
                .emit(app)
                {
                    warn!("Overlay preview could not emit its sample text: {error}");
                }
            }
        }

        tokio::time::sleep(LEVEL_INTERVAL).await;
    }
    PreviewOutcome::Completed
}

/// Show the real overlay for a few seconds, driven by synthetic audio.
///
/// `sample_text` is the Live panel's transcript, passed in already translated so
/// i18n stays entirely on the frontend. Resolves when the overlay is hidden
/// again, so the caller can disable its button for the duration.
#[tauri::command]
#[specta::specta]
pub async fn preview_overlay_on_screen(app: AppHandle, sample_text: String) -> Result<(), String> {
    run_overlay_preview(app, sample_text).await
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

    #[test]
    fn the_guard_is_released_even_if_the_driver_never_finishes() {
        // The guard is process-wide, so leave it exactly as it was found.
        assert!(!is_previewing());
        cancel_preview(); // no-op while idle
        assert!(!PREVIEW_CANCELLED.load(Ordering::SeqCst));

        {
            PREVIEW_ACTIVE.store(true, Ordering::SeqCst);
            let _guard = PreviewGuard;
            cancel_preview();
            assert!(PREVIEW_CANCELLED.load(Ordering::SeqCst));
            // Dropped here without any explicit release, as a panicked or
            // dropped driver future would be.
        }

        assert!(!is_previewing());
        assert!(!PREVIEW_CANCELLED.load(Ordering::SeqCst));
    }

    #[test]
    fn synthetic_levels_fill_sixteen_buckets_and_move() {
        let first = synthetic_levels(0);
        assert_eq!(first.len(), 16);
        assert!(first.iter().all(|level| (0.0..=1.0).contains(level)));
        assert_ne!(
            format!("{:?}", first),
            format!("{:?}", synthetic_levels(4)),
            "the waveform must animate between frames"
        );
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
            let chunks = stream_chunks(CLI_PREVIEW_SAMPLE_TEXT, count);
            assert_eq!(chunks.len(), count);
            assert_eq!(chunks[count - 1], CLI_PREVIEW_SAMPLE_TEXT);
        }
    }
}
