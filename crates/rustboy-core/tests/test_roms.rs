//! Runs Blargg's test ROMs and checks each one prints "Passed" through the link port.
//!
//! The ROMs are not in the repository. Put them in `test-roms/` or set RUSTBOY_TEST_ROMS.

use std::path::PathBuf;

use rustboy_core::Emulator;

const FRAME_LIMIT: usize = 6_000; // about 100 seconds of console time, far more than any ROM needs

// RUSTBOY_TEST_ROMS if set, otherwise test-roms/ at the top of the repository.
fn rom_folder() -> PathBuf {
    std::env::var_os("RUSTBOY_TEST_ROMS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-roms"))
}

// Run one ROM until it says Passed or Failed. None means the ROM is not there and was skipped.
fn run(name: &str) -> Option<String> {
    let path = rom_folder().join(name);
    let Ok(rom) = std::fs::read(&path) else {
        if std::env::var_os("RUSTBOY_REQUIRE_TEST_ROMS").is_some() {
            panic!("missing {}", path.display());
        }
        eprintln!("skipping {name}: not found at {}", path.display());
        return None;
    };

    let mut emulator = Emulator::new();
    emulator
        .load_rom(rom)
        .expect("test ROMs are valid cartridges");
    for _ in 0..FRAME_LIMIT {
        emulator.run_frame();
        let printed = String::from_utf8_lossy(emulator.bus.serial.output());
        if printed.contains("Passed") || printed.contains("Failed") {
            return Some(printed.into_owned());
        }
    }
    Some(String::from_utf8_lossy(emulator.bus.serial.output()).into_owned())
}

fn assert_passes(name: &str) {
    if let Some(printed) = run(name) {
        assert!(
            printed.contains("Passed"),
            "{name} did not pass:\n{printed}"
        );
    }
}

#[test]
fn cpu_instrs() {
    assert_passes("cpu_instrs/cpu_instrs.gb");
}

#[test]
fn instr_timing() {
    assert_passes("instr_timing/instr_timing.gb");
}
