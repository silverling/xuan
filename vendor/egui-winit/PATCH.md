# Image clipboard paste events

This is egui-winit 0.33.3 from crates.io, based on upstream commit
`44cdd653e2317d300fb8a6c9c36b03f23991e803`. Its MIT and Apache licenses are
included alongside the source.

Upstream consumes the native paste keypress but only emits `Event::Paste` if
the system clipboard has nonempty text. An image-only clipboard therefore
produces no event that Xuan can handle, including through eframe's raw input
hook.

The local patch emits `Event::Paste("")` when clipboard text is unavailable
or empty. Xuan uses that event to read the image clipboard. egui text fields
already ignore empty paste payloads, and normal text paste still normalizes
line endings. The manifest's license paths are adjusted for this directory.

Run the patch's regression test with:

```sh
cargo test --locked --package egui-winit --lib clipboard_paste
```

Remove this patch when upgrading to a backend that preserves paste commands
for non-text clipboard content.
