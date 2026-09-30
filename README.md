# Engine

A listen-server game engine. The server simulates the world, the local player replays their own commands, and other players interpolate. Maps are Quake-style brushes, with a voxel world beside them. Game code is LuaJIT.

The default run starts both sides in one process. The client connects to `127.0.0.1:25400`.

## Build

```sh
cargo build --release -p base -p editor -p launcher
```

Debug:

```sh
cargo run -p base
```

Settings launcher (renderer, host, map, tickrate, editor):

```sh
cargo run -p launcher
```

The launcher writes `launcher.json` beside its binary and starts `base` with `ENGINE_GFX` / `ENGINE_HOST` and the matching flags. Manual `ENGINE_*` env vars still work when you run `base` directly.

A `+` on the command line is the same as `--`, so `+map hall` is `--map hall`.

## Run

```sh
cargo run -p base -- --map hall
```

| Flag | Effect |
| --- | --- |
| `--map <name>` | Map to load. Defaults to `hall`. |
| `--tickrate <n>` | Simulation rate. Defaults to 60. |
| `--editor` | Open the map editor instead of the game. |
| `--compile-map` | Compile a `.map` to `.cmap` and exit. |

Server only, or client only:

```sh
cargo run -p base --no-default-features --features server
cargo run -p base --no-default-features --features client
```

Controls: click to capture the mouse. WASD to move, mouse to look, Space to jump, Shift to sprint, Ctrl to duck, Alt to walk, Esc to release the mouse.

## Graphics

Set `ENGINE_GFX` to pick a backend. With it unset, the first one that initializes is used: Metal on Apple, D3D12 then D3D11 on Windows, then Vulkan, then OpenGL.

```sh
ENGINE_GFX=vulkan cargo run -p base
```

`opengl`, `vulkan`, `metal`, `d3d12`, and `d3d11` are the names. OpenVR is used when a headset is present.

Set `ENGINE_HOST` to pick the window/event-loop host. Default is `winit`. `sdl2` and `xbox` are reserved stubs.

```sh
ENGINE_HOST=winit ENGINE_GFX=metal cargo run -p base
```

## Maps

`hall` is included. A text `.map` compiles to `.cmap`:

```sh
cargo run -p base -- --compile-map --map hall
```

The game looks for maps in `./maps`, `./game/base/maps`, and `game/base/maps` inside the source tree.

The editor:

```sh
cargo run -p editor -- hall
```

## Mobile

Android, with `ANDROID_NDK_HOME` set:

```sh
sh mobile/android/build.sh
```

iOS, with Xcode installed:

```sh
sh mobile/ios/build.sh
```

## Test

```sh
cargo test -p base
```

## License

MIT. See [LICENSE](LICENSE). Third-party terms for the binary are in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
