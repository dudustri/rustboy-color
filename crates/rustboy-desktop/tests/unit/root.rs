use super::*;

#[test]
fn every_button_has_a_key() {
    for (key, button) in [
        (Key::Named(NamedKey::ArrowUp), Button::Up),
        (Key::Named(NamedKey::Enter), Button::Start),
        (Key::Named(NamedKey::Shift), Button::Select),
        (Key::Character("a".into()), Button::A),
        (Key::Character("x".into()), Button::B),
    ] {
        assert_eq!(button_for(&key), Some(button));
    }
}

#[test]
fn holding_shift_still_works() {
    assert_eq!(button_for(&Key::Character("A".into())), Some(Button::A));
}
