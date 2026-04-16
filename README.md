# Send-RS

Send-RS is a Rust-first file transfer tool for LAN + optional public P2P mode.

## Implemented in this revision

- Rust monorepo workspace with dedicated crates for `core`, `discovery`, `transport`, `transfer`, `chat`, `security`, `signaling-client`, `signaling-server`, `ffi`, `cli`.
- CLI commands:
  - `sendrs discover`
  - `sendrs send <file_or_dir> [--public]`
  - `sendrs receive <code> [--target <dir>]`
  - `sendrs history [--limit N]`
  - `sendrs clean [--all|--sessions|--offers|--manifests|--history]`
- File/folder manifest builder with chunk hashes + resume bitmap + full file hash.
- M2 local transfer executor with chunk copy, retry, per-chunk verification, full-file verification, and checkpoint resume via manifest updates.
- Simplified CLI handshake model:
  - sender generates one-time code and blocks until receiver joins
  - first connection requires both sides typing `yes`
  - trusted peers skip future confirmation
- `discover` now shows LAN share offers (`code`, source name, owner, size) for active send sessions.
- `history` keeps completed/failed send and receive records locally.
- `clean` supports clearing transient session/offer state and optional history/manifests cleanup.
- Local trust model for first-time pairing (`.sendrs/trusted_peers.json`).
- Local chat persistence in SQLite (`.sendrs/chat.db`) for long-term history.
- Rust signaling server using WebSocket for `register/offer/answer/candidate/punch_result/disconnect` message relay.
- Flutter app skeleton (`apps/flutter_app`) sharing a unified UI for desktop + Android and wired to Rust FFI symbols.

## Workspace layout

- `crates/core`: shared data models, protocol, error model
- `crates/discovery`: LAN discovery via UDP broadcast beacon
- `crates/transport`: QUIC transport profile/config wrapper
- `crates/transfer`: manifest/chunk hashing/resume bitmap
- `crates/chat`: local chat storage (SQLite)
- `crates/security`: identity + pairing trust store
- `crates/signaling-client`: WebSocket signaling client
- `crates/signaling-server`: signaling relay service
- `crates/ffi`: C ABI for Flutter bridge
- `crates/cli`: command-line entrypoint
- `apps/flutter_app`: Flutter desktop/Android shell

## Quick start

### Rust build and tests

```bash
cargo test --workspace
```

### CLI help

```bash
cargo run -p sendrs-cli -- --help
```

### Start signaling server

```bash
cargo run -p sendrs-signaling-server
```

Server endpoints:

- `GET /health`
- `WS /ws`

## Current assumptions / constraints

- Public network mode is opt-in per transfer (`--public`), default LAN only.
- Public transfer currently supports direct P2P design intent; no relay implementation in v1 base code.
- CLI intentionally excludes chat and clipboard sync.
- Clipboard sync removed from all clients in this plan revision.
- `sendrs send` is blocking by design and keeps sharing after each completed transfer; it only stops when you terminate it (for example `Ctrl+C`).
- `sendrs receive` supports optional `--target`; when omitted, current directory is used.
- Current transfer execution still runs in the same runtime environment (cross-device data channel will be wired in the next milestone).
