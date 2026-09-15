# rustboy-color

A Game Boy Color emulator written in Rust. The same core runs as a desktop app on
x86_64 and aarch64, and in the browser through WebAssembly.

**Status:** not playable yet. The CPU is finished. The screen, sound and cartridge
banking come next.

## Run it

**Desktop**

```sh
cargo run -p rustboy-desktop                # title screen, then a blank screen
cargo run -p rustboy-desktop -- game.gbc    # load a game
```

**Browser**

Needs `wasm-pack` once: `cargo install wasm-pack`

```sh
cargo run -p rustboy-wasm                   # builds, then serves http://localhost:8080
```

## Controls

| Key | Button |
|---|---|
| Arrows | D-pad |
| A / X | A / B |
| Enter | Start |
| Shift | Select |
| F11 | Fullscreen, desktop only |
| Esc | Quit, desktop only |

## Tests

```sh
cargo test --workspace
```

The CPU is also checked against Blargg's test ROMs, which compare every instruction with a real
Game Boy. They live in `test-roms/` as a git submodule, so clone with:

```sh
git clone --recursive https://github.com/dudustri/rustboy-color
```

Already cloned? Fetch them with `git submodule update --init`. Without them, those tests are skipped.

## How it is built

| Crate | What it does |
|---|---|
| `rustboy-core` | The console itself: CPU, screen, sound, timer, cartridge. No dependencies. |
| `rustboy-frontend` | The loop every platform shares. |
| `rustboy-splash` | The title screen, made from a photo at build time. |
| `rustboy-desktop` | The desktop app. |
| `rustboy-wasm` | The browser version, plus a small local server. |

Three choices shape the design:

- **Every chip moves tick by tick**, like the real hardware, so timing-sensitive games work.
- **The screen is designed to draw one pixel at a time**, so effects that change mid-line work.
- **The core never touches files, windows or speakers**, so the same code runs everywhere.

## Docs

| File | Covers |
|---|---|
| [`architecture.md`](docs/architecture.md) | How the hardware works, and why the emulator is built this way |
| [`core-modules.md`](docs/core-modules.md) | What each part of the core does |
| [`cpu-operation.md`](docs/cpu-operation.md) | How the CPU runs, with every instruction |
| [`build.md`](docs/build.md) | How the build fits together |
| [`roadmap.md`](docs/roadmap.md) | What is done and what comes next |
