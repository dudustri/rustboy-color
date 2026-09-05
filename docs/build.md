# How the build works

Cargo works the whole thing out on its own. There is no Makefile and no order
written down anywhere: it all comes from who depends on whom.

---

## The crates

```mermaid
%%{init: {"flowchart": {"nodeSpacing": 50, "rankSpacing": 80}}}%%
flowchart TB
    CORE["rustboy-core<br/>the console, no dependencies at all"]
    SPLASH["rustboy-splash<br/>the title screen"]
    FRONT["rustboy-frontend<br/>the shared host layer"]
    DESK["rustboy-desktop<br/>window and keyboard"]
    CORE --> SPLASH
    CORE --> FRONT
    SPLASH --> FRONT
    FRONT --> DESK
```

An arrow means "is needed by". The browser host will hang off `rustboy-frontend`
in the same place the desktop one does.

---

## The order

Cargo reads the workspace file, finds the four crates, and sorts them.

```mermaid
%%{init: {"flowchart": {"nodeSpacing": 45, "rankSpacing": 65}}}%%
flowchart TB
    A["1 · build rustboy-core"]
    B["2 · build image and color_quant<br/>they are needed by a build script"]
    C["3 · compile build.rs and run it"]
    D["4 · build rustboy-splash"]
    E["5 · build rustboy-frontend"]
    F["6 · build rustboy-desktop"]
    A --> B --> C --> D --> E --> F
```

Crates that do not need each other are built at the same time, so the real
output is less tidy than this list.

---

## The odd step: a build script

`rustboy-splash` has a `build.rs`. Cargo compiles it into a small program and
runs it *before* the crate itself, handing it a scratch folder called `OUT_DIR`.

```mermaid
%%{init: {"flowchart": {"nodeSpacing": 50, "rankSpacing": 75}}}%%
flowchart LR
    SRC["assets/source.jpeg<br/>the photo, kept in git"]
    RUN["build.rs<br/>crop, recolour, shrink,<br/>then stamp the words"]
    OUT["OUT_DIR<br/>splash.rgba + splash_text.mask"]
    LIB["lib.rs<br/>include_bytes! picks them up"]
    SRC --> RUN --> OUT --> LIB
```

This is why the generated files are not in the repository. Only the photo is
kept; the two layers are rebuilt whenever they are missing.

`OUT_DIR` is a scratch folder inside `target`:

```
target/debug/build/rustboy-splash-<hash>/out/splash.rgba
target/debug/build/rustboy-splash-<hash>/out/splash_text.mask
```

The hash changes with the profile, the features and the target, so there can be
more than one of these at a time and the path must never be written down. That
is what `env!("OUT_DIR")` is for.

### When it runs again

Two lines decide that:

```rust
println!("cargo::rerun-if-changed=build.rs");
println!("cargo::rerun-if-changed={}", source.display());
```

Change the photo or the script, and the title screen is rebuilt. Change anything
else and Cargo skips straight past, reusing what is already in `OUT_DIR`.

---

## Two kinds of dependency

This is the part worth remembering.

| | compiled for | example |
| --- | --- | --- |
| `[dependencies]` | the machine being **targeted** | `winit`, `pixels` |
| `[build-dependencies]` | the machine **doing the building** | `image`, `color_quant` |

```mermaid
%%{init: {"flowchart": {"nodeSpacing": 50, "rankSpacing": 75}}}%%
flowchart TB
    subgraph host["runs on this laptop"]
        IMG["image + color_quant<br/>read the JPEG"]
    end
    subgraph target["runs in a browser tab"]
        WASM["the emulator<br/>plus raw pixel bytes"]
    end
    IMG --> WASM
```

So when the browser build arrives, the JPEG is still decoded here, natively, at
build time. The browser only ever receives plain pixel bytes, and no image
library is shipped to it.

---

## Commands

```sh
cargo build --workspace      # everything
cargo run -p rustboy-desktop # the window
cargo test --workspace       # every test
```

To force the title screen to be rebuilt without changing the photo:

```sh
touch crates/rustboy-splash/build.rs
```
