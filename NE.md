# pyxel-ne

A small fork of Pyxel that lets a game install its own screen shader.
It was built for Thunderwing (`../raiden`) dynamic lighting. All changes are marked `[pyxel-ne]`.

## Branches

- `origin` is https://github.com/nightconcept/pyxel. `upstream` is https://github.com/kitao/pyxel.
- `main` mirrors `upstream/main`. Never commit to it.
- `nightengine` is our shader work, kept as a few commits on top of `upstream/main`.

To take a new Pyxel release, rebase. Do not merge.

```sh
git fetch upstream
git switch main && git merge --ff-only upstream/main && git push origin main
git switch nightengine && git rebase upstream/main
# Rebuild and test, then bump the version in python/pyproject.toml (2.9.9+ne.1 -> <new>+ne.1).
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

The user shader gets everything in `crates/pyxel-core/src/shaders/common.frag`:
`getScreenParams`, `isInScreen`, `getScreenColor`, and the `u_screen*` uniforms.

## Changes

- `crates/pyxel-core/src/graphics.rs`: `build_screen_shader` returns `Err(log)` instead of asserting. Adds the user
  shader, the uniform store, the shader images, and the display capture.
- `crates/pyxel-binding/src/shader_wrapper.rs`: the Python functions.
- `python/pyxel/__init__.pyi`: stubs. `python/pyproject.toml`: version `2.9.9+ne.1`.

## Build (macOS arm64)

```sh
cp LICENSE README.md python/pyxel/
cd python
nix shell nixpkgs#rustup nixpkgs#cmake --command env \
  CC=/usr/bin/clang CXX=/usr/bin/clang++ CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/usr/bin/clang \
  uvx maturin build -o ../dist --release --features sdl2_static
```

The Apple clang overrides are needed because a non-Apple `cc` is first on PATH and cannot find `-liconv`.
Other platforms and the wasm build are not built yet.
