//! The title screen every host shows before a game loads.
//!
//! Picture and words are separate layers, so the picture can fade out while the words stay.

use rustboy_core::FRAMEBUFFER_LEN;

/// The picture on its own, 160 by 144, in red, green, blue, alpha order.
pub const PICTURE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/splash.rgba"));
/// One byte per pixel, naming which layer that pixel belongs to.
pub const TEXT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/splash_text.mask"));

/// Marks a letter of the title in [`TEXT`].
pub const LETTER: u8 = 1;
/// Marks a title letter's shadow in [`TEXT`].
pub const SHADOW: u8 = 2;
/// Marks a letter of the smaller line underneath in [`TEXT`].
pub const BYLINE: u8 = 3;

const LETTER_COLOUR: [f32; 3] = [224.0, 248.0, 208.0]; // the pale green of a blank screen
const SHADOW_COLOUR: [f32; 3] = [16.0, 24.0, 16.0];

const FADE_IN: f32 = 1.5; // picture and words rise out of black together
const HOLD: f32 = 3.0; // both at full brightness
const FADE_OUT: f32 = 2.0; // the picture sinks away, the words stay
const TEXT_HOLD: f32 = 1.5; // words alone on black, with the byline joining them
const BYLINE_FADE: f32 = 0.5; // how long the byline takes to appear

/// How long the whole title screen lasts.
pub const SECONDS: f32 = FADE_IN + HOLD + FADE_OUT + TEXT_HOLD;

/// How bright the picture, title and byline are now, or `None` once the title screen is over.
pub fn levels(seconds: f32) -> Option<(f32, f32, f32)> {
    if seconds >= SECONDS {
        return None;
    }
    let text = (seconds / FADE_IN).min(1.0);
    let dark = FADE_IN + HOLD + FADE_OUT; // when the picture is fully gone
    let picture = if seconds < FADE_IN + HOLD {
        text
    } else {
        (1.0 - (seconds - FADE_IN - HOLD) / FADE_OUT).max(0.0)
    };
    // The byline only shows up once the picture has left, on plain black.
    let byline = ((seconds - dark) / BYLINE_FADE).clamp(0.0, 1.0);
    Some((picture, text, byline))
}

/// Paint the title screen into `frame` ([`FRAMEBUFFER_LEN`] bytes) and say if it is still running.
pub fn render(seconds: f32, frame: &mut [u8]) -> bool {
    debug_assert_eq!(frame.len(), FRAMEBUFFER_LEN);
    let Some((picture, text, byline)) = levels(seconds) else {
        return false;
    };

    // Fading means mixing towards black, so every channel is simply scaled.
    for (i, out) in frame.chunks_exact_mut(4).enumerate() {
        let colour = match TEXT[i] {
            LETTER => LETTER_COLOUR.map(|c| c * text),
            SHADOW => SHADOW_COLOUR.map(|c| c * text),
            // Before the byline appears its pixels must show the picture, not black.
            BYLINE if byline > 0.0 => LETTER_COLOUR.map(|c| c * byline),
            _ => [0, 1, 2].map(|c| PICTURE[i * 4 + c] as f32 * picture),
        };
        out[0] = colour[0] as u8;
        out[1] = colour[1] as u8;
        out[2] = colour[2] as u8;
        out[3] = 0xFF;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_assets_are_the_right_size() {
        assert_eq!(PICTURE.len(), FRAMEBUFFER_LEN);
        assert_eq!(TEXT.len(), FRAMEBUFFER_LEN / 4);
    }

    // The build script keeps its own copy of these numbers, so check they agree.
    #[test]
    fn the_build_script_used_the_same_layer_numbers() {
        assert!(
            TEXT.iter()
                .all(|&v| matches!(v, 0 | LETTER | SHADOW | BYLINE))
        );
        assert!(TEXT.contains(&LETTER));
        assert!(TEXT.contains(&SHADOW));
        assert!(TEXT.contains(&BYLINE));
    }

    #[test]
    fn it_starts_black_and_ends() {
        assert_eq!(levels(0.0), Some((0.0, 0.0, 0.0)));
        assert_eq!(levels(SECONDS), None);
    }

    #[test]
    fn the_words_outlive_the_picture() {
        let (picture, text, _) = levels(SECONDS - 0.1).unwrap();
        assert_eq!(picture, 0.0);
        assert_eq!(text, 1.0);
    }

    #[test]
    fn the_byline_waits_for_the_picture_to_go() {
        let (picture, _, early) = levels(FADE_IN + HOLD).unwrap();
        assert!(picture > 0.0);
        assert_eq!(early, 0.0);

        let (_, _, late) = levels(SECONDS - 0.1).unwrap();
        assert_eq!(late, 1.0);
    }

    #[test]
    fn the_byline_does_not_punch_a_hole_in_the_picture() {
        let spot = TEXT.iter().position(|&v| v == BYLINE).unwrap();
        let mut frame = vec![0; FRAMEBUFFER_LEN];
        render(FADE_IN, &mut frame); // picture at full, byline not yet due
        assert_eq!(
            &frame[spot * 4..spot * 4 + 3],
            &PICTURE[spot * 4..spot * 4 + 3]
        );
    }

    #[test]
    fn render_reports_when_it_is_over() {
        let mut frame = vec![0; FRAMEBUFFER_LEN];
        assert!(render(0.5, &mut frame));
        assert!(!render(SECONDS, &mut frame));
    }
}
