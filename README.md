# rustboy-color

A Game Boy Color emulator written in Rust, running in the browser (wasm32) and
natively on x86_64 and aarch64 from a single emulation core.

**Status:** M1 of 6 — the desktop host runs. Not yet playable: only 6 of ~500 CPU instructions exist.

## Design

See [`docs/architecture.md`](docs/architecture.md) for the hardware model, the
memory map, and the reasoning behind each design decision.

The short version:

| Choice | Why |
|---|---|
| Tick-accurate timing, `bus.tick(4)` per M-cycle | compatibility with timing-sensitive games |
| Pixel FIFO PPU | mid-scanline effects work; mode 3 length emerges naturally |
| `rustboy-core` has zero dependencies and no I/O | the same core drives desktop and web |

## Layout

| Crate | Role |
|---|---|
| `rustboy-core` | CPU, PPU, APU, bus, cartridge — pure state machine, no dependencies |
| `rustboy-splash` | the title screen, built from a photo at compile time |
| `rustboy-frontend` | the `Host` trait and the frame driver, shared by every platform |
| `rustboy-desktop` | winit + pixels *(audio in M5)* |
| `rustboy-wasm` | wasm-bindgen + canvas + WebAudio *(PR-04)* |

## Build

```sh
cargo check --workspace
cargo test --workspace
cargo run -p rustboy-desktop                # title screen, then a blank LCD
cargo run -p rustboy-desktop -- game.gbc    # load a cartridge
```

For the browser, build the wasm once, then serve `web/`:

```sh
wasm-pack build crates/rustboy-wasm --target web --out-dir ../../web/pkg
cargo run -p rustboy-wasm                   # http://localhost:8080
```

Keys: arrows, A and X for the A and B buttons, Enter, Shift, F11 for fullscreen,
Escape to quit.

See [`docs/build.md`](docs/build.md) for how the build fits together.

## License

Dual licensed under [MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your
option.
