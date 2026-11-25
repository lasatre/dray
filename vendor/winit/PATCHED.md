# Vendored winit 0.30.13 (patched)

Unmodified copy of winit 0.30.13 from crates.io, except for one change in
`src/platform_impl/linux/x11/event_processor.rs` (search for `[gdriveoperator patch]`):

The `XdndPosition` handler now emits `WindowEvent::CursorMoved` with the drag position
(converted from root to window coordinates). Upstream winit drops these coordinates, so an
application can't tell where over its window files are being dragged. GDrive Operator needs
that to support dropping onto a specific folder.

Remove this directory and the `[patch.crates-io]` entry in Cargo.toml once winit exposes drag
positions itself.
