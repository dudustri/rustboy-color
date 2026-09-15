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
