# sendrs_app

Flutter client for Send-RS (`macOS + Windows + Android`).

## Current status

- Unified shell screens for Devices / Transfers / Chat / Settings.
- Rust FFI bridge scaffold (`native_bridge.dart`) wired to `sendrs-ffi` exported symbols.
- Public network mode is represented in UI and intentionally disabled by default at business logic level.

## Next integration steps

1. Use `flutter_rust_bridge` or manual FFI wrappers for all core APIs.
2. Bind discovery list to real `start_discovery()` and `list_peers()` responses.
3. Add transfer queue stream + task progress from Rust core.
4. Connect chat panel to `send_chat` and `list_chat_messages`.
