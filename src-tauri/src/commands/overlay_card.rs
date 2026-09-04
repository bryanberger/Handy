//! Tauri command for the overlay webview's card-shape reports.
//!
//! Under Glass the native window is the card (window slack is zero), and the
//! Live panel's open/collapsed morph is a webview decision driven by streamed
//! text and phase. Rust cannot see it, so this is the one thing the overlay
//! page tells the backend about itself. Under Flat, and off macOS where the
//! effective Material is never Glass, it is a no-op. `overlay::set_card_shape`
//! records the shape and returns once it sees the Material is not Glass.

use crate::overlay;
use crate::overlay_geometry::{OverlayCardShape, CARD_MORPH_MS, MAX_CARD_MORPH_MS};
use tauri::AppHandle;

/// The overlay webview calls this whenever the card's shape changes.
///
/// The payload is a symbolic shape plus a duration, never pixels. The backend
/// recomputes the window size from its own constants and the resolved size
/// scale, and coalesces by shape identity rather than by time, so a repeated
/// report costs nothing and a real change never waits.
///
/// `durationMs` is how long the window may take to reach the new shape, 0
/// (snap) to 2000 ms. Anything longer is rejected, not clamped, since it would
/// leave a native window animation running long after the card settled.
#[tauri::command]
#[specta::specta]
pub fn set_overlay_card_shape(
    app: AppHandle,
    shape: OverlayCardShape,
    duration_ms: u32,
) -> Result<(), String> {
    overlay::set_card_shape(&app, shape, checked_duration_ms(duration_ms)?);
    Ok(())
}

/// The morph duration a report asked for, if the overlay could plausibly want
/// it.
///
/// Nothing downstream sanitises this. It becomes an `NSAnimationContext`
/// duration on the main thread, so an absurd value is a window that keeps
/// moving for a minute. The bound is several times the card's own morph, so it
/// catches a bug or a hostile caller, never a deliberately unhurried one.
fn checked_duration_ms(duration_ms: u32) -> Result<u32, String> {
    if duration_ms > MAX_CARD_MORPH_MS {
        return Err(format!(
            "card shape duration {duration_ms}ms is out of range (0..={MAX_CARD_MORPH_MS}ms, \
             the card's own morph being {}ms)",
            CARD_MORPH_MS
        ));
    }
    Ok(duration_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_shape_duration_accepts_the_card_morph_and_a_snap() {
        assert_eq!(checked_duration_ms(0), Ok(0));
        assert_eq!(checked_duration_ms(CARD_MORPH_MS), Ok(CARD_MORPH_MS));
        assert_eq!(
            checked_duration_ms(MAX_CARD_MORPH_MS),
            Ok(MAX_CARD_MORPH_MS)
        );
    }

    #[test]
    fn card_shape_duration_rejects_anything_past_the_bound() {
        for duration_ms in [MAX_CARD_MORPH_MS + 1, 60_000, u32::MAX] {
            let error = checked_duration_ms(duration_ms)
                .expect_err("a duration past the bound must not reach the window");
            assert!(
                error.contains(&duration_ms.to_string()),
                "the rejected value belongs in the message: {error}"
            );
        }
    }
}
