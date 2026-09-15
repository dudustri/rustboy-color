use super::*;

#[test]
fn pressed_button_reads_as_zero() {
    let mut joypad = Joypad::new();
    joypad.write(0x20); // ask for the d-pad row
    joypad.set_button(Button::Right, true);
    assert_eq!(joypad.read() & 0x01, 0);
}

#[test]
fn unselected_row_is_not_reported() {
    let mut joypad = Joypad::new();
    joypad.write(0x10); // ask for the face button row
    joypad.set_button(Button::Right, true);
    assert_eq!(joypad.read() & 0x01, 0x01);
}
