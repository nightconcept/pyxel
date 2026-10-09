# pyxel-ne

A small fork of Pyxel that lets a game install its own screen shader and let the monitor pace the frame loop
(vsync). The shaders were built for Thunderwing (`../raiden`) dynamic lighting, and vsync for Nightrunner's
display pacing. All changes are marked `[pyxel-ne]`.

## Branches

- `origin` is https://github.com/nightconcept/pyxel. `upstream` is https://github.com/kitao/pyxel.
- `main` mirrors `upstream/main`. Never commit to it.
- `nightengine` is our shader work, kept as a few commits on top of `upstream/main`.

To take a new Pyxel release, rebase. Do not merge.

```sh
git fetch upstream
git switch main && git merge --ff-only upstream/main && git push origin main
git switch nightengine && git rebase upstream/main
# Rebuild and test, then bump the version in python/pyproject.toml (2.9.9+ne.2 -> <new>+ne.1).
git push --force-with-lease origin nightengine
```

Conflicts are most likely in `crates/pyxel-core/src/graphics.rs`, around `create_screen_shaders` and `render_screen`.

## API

| Call | What it does |
|------|--------------|
| `pyxel.screen_shader(code)` | Compile `code` (a GLSL ES 1.0 fragment body, appended after `common.frag`) and use it instead of the `screen_mode` shader. `None` restores the built-in shader. A compile error raises `ValueError`, and the previous shader stays active. |
| `pyxel.shader_uniform(name, values)` | Set a uniform: 1–4 floats give `float`/`vec2`/`vec3`/`vec4`; a multiple of 4 gives a `vec4[]`. Values persist. |
| `pyxel.shader_image(slot, img)` | Bind an `Image` (raw colour indices) as `u_image{slot}`, sized `u_imageSize{slot}`. Slots 0–3, texture units 2–5. The image is uploaded each frame. |
| `pyxel.display_capture(filename, scale=1)` | Save the next rendered frame as shown in the window, after the shader. `screenshot()` cannot see shader output. |
| `pyxel.NE`, `pyxel.NUM_SHADER_IMAGES` | Feature check (`getattr(pyxel, "NE", False)`) and slot count. |
| `pyxel.vsync(on)` | On: the loop does not sleep, runs one `update` and one `draw` for each loop (no catch-up), and swaps with swap interval -1 (late swap tearing), or 1 if the driver refuses -1. The swap blocks until the refresh. Off: the timer loop. Applies at once. Returns the interval in use; 0 means vsync is off (headless, a page, or a driver that refuses). Desktop only. |
| `pyxel.vsync_enabled()` | Whether vsync paces the loop now. |
| `pyxel.NE_PACING` | Feature check for `vsync` (`getattr(pyxel, "NE_PACING", False)`). |

The user shader gets everything in `crates/pyxel-core/src/shaders/common.frag`:
`getScreenParams`, `isInScreen`, `getScreenColor`, and the `u_screen*` uniforms.

## Changes

- `crates/pyxel-core/src/graphics.rs`: `build_screen_shader` returns `Err(log)` instead of asserting. Adds the user
  shader, the uniform store, the shader images, and the display capture.
- `crates/pyxel-binding/src/shader_wrapper.rs`: the Python functions.
- `crates/pyxel-core/src/system.rs`: `update_count` (one update for each loop with vsync on), `set_vsync`,
  `vsync_enabled`.
- `crates/pyxel-core/src/platform/facade.rs` and `mod.rs`: the vsync flag (`AtomicBool`) and
  `set_swap_interval`.
- `crates/pyxel-core/src/platform/sdl2/platform_sdl2.rs`: `set_swap_interval`; with vsync on, `run_frame_loop`
  skips the sleep and measures the delta with `SDL_GetPerformanceCounter`, and `step_frame` swaps with no wait.
  The emscripten path is unchanged.
- `crates/pyxel-binding/src/pacing_wrapper.rs`: the Python functions.
- `python/pyxel/__init__.pyi`: stubs. `python/pyproject.toml`: version `2.9.9+ne.2`.
- `mise.toml`: the build tools and tasks.

## Build

[mise](https://mise.jdx.dev) installs the tools: the Rust nightly from `rust-toolchain.toml` (with clippy and
rustfmt), CMake, and uv.

```sh
mise install
mise run test    # the pyxel-core tests
mise run lint    # rustfmt check and clippy
mise run build   # the wheel for this machine, into dist/
```

On macOS the tasks use Apple clang, because a non-Apple `cc` earlier on PATH cannot find `-liconv`.
Other platforms and the wasm build are not built locally yet.

## Release

A release is a GitHub release with the wheels for every platform. There is no PyPI upload.

```sh
# Bump the version in python/pyproject.toml (2.9.9+ne.2 -> 2.9.9+ne.3), commit, and push nightengine.
git tag -a ne-v2.9.9.3 -m "pyxel-ne 2.9.9+ne.3: <what changed>"
git push origin ne-v2.9.9.3
```

The tag runs `.github/workflows/release.yml`: it builds and checks the wheels (macOS x86_64 and arm64, Linux
x86_64, aarch64, and armv7, Windows x64 and x86) and makes the release `pyxel-ne <version>`. A game installs it with
`pyxel==<version>` and `find-links` to `https://github.com/nightconcept/pyxel/releases/expanded_assets/<tag>`.
The CI doc generators drop the docstrings of the fork's functions from the built stubs.

## TODO

- Test vsync in WASM builds. The wasm wheel in `wasm/` is stock 2.9.9, and `wasm/pyxel.js` reads the version
  with `/pyxel-([\d.]+)-/`, which does not match a `+ne.N` version.
