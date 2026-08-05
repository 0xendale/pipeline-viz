# Shared packages

`protocol/` contains the frontend mirror of the Rust `Snapshot`/`Patch` wire
contract plus shared live-state helpers. Templates depend on this package
instead of importing from another template's source tree.
