# xuan

A native Linux port of [Compositor](../Compositor), rewritten in Rust with egui and the wgpu rendering backend. The interface follows Compositor's dark neutral palette, compact contextual controls, vertical tool rail, tabbed workspace, and Layers panel.

Development is in progress. See [the porting plan](docs/PORTING.md) for implementation and verification status.

## Build

Requires Rust 1.88 or newer, a Linux desktop (Wayland or X11), and a Vulkan-capable graphics driver (Mesa's software Vulkan driver is also suitable).

```sh
cargo run --release
cargo test
```

Original Compositor copyright © 2026 Wonder Assembly LLC, MIT licensed. See [LICENSE](LICENSE).

