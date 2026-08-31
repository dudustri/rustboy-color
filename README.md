# rustboy-color

A Game Boy Color emulator written in Rust, running in the browser (wasm32) and
natively on x86_64 and aarch64 from a single emulation core.

**Status:** stage 1 of 7 — core skeleton. Not yet playable.

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
| `rustboy-core` | CPU, PPU, APU, bus, cartridge — pure state machine |
| `rustboy-desktop` | winit + pixels + cpal *(stage 2)* |
| `rustboy-wasm` | wasm-bindgen + canvas + WebAudio *(stage 3)* |

## Build

```sh
cargo check --workspace
cargo test --workspace
```

## License

MIT OR Apache-2.0
