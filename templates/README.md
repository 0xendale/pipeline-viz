# UI templates

Each directory is an independently runnable dashboard surface for the same
pipeline-viz WebSocket protocol.

- `simple/`: dense 2D operational dashboard.
- `three-d/`: immersive WebGL dashboard, added separately from the simple UI.

Templates are source projects. The Rust crate does not embed or select a
template yet.
