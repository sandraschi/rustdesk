# rustdesk++ — Enhanced RustDesk fork

**Forked from [rustdesk/rustdesk](https://github.com/rustdesk/rustdesk) v1.4.9**

This fork adds **headless CLI file transfer** and a **REST API** to RustDesk, turning it from a GUI remote desktop into a programmable remote desktop platform.

## Headless CLI

### Info
- `--status` — Local RustDesk ID, service status, rendezvous/relay servers
- `--peer-info <peer_id>` — Check if a peer is online
- `--get-id` — Print local RustDesk ID
- `--version` — Print version

### File Operations (relay-based, password optional for passwordless peers)
- `--send-file <id> <local> <remote> [password]` — Send file to peer (WORKING)
- `--recv-file <id> <remote> <local> [password]` — Receive file from peer
- `--list-dir <id> <path> [password]` — List remote directory (WORKING)
- `--delete-remote <id> <path> [password]` — Delete remote file
- `--move-remote <id> <old> <new> [password]` — Move/rename remote file
- `--send-dir <id> <local_dir> <remote> [password]` — Send directory contents
- `--create-dir <id> <path> [password]` — Create remote directory

### Remote Control
- `--restart <id> [password]` — Restart remote PC
- `--shutdown <id> [password]` — Shutdown remote PC
- `--screenshot <id> <output> [password]` — Capture remote screenshot

### REST API Server
- `--api-server [port]` — Start HTTP API server (default 10806)

### Endpoints
- `GET  /api/v1/health` — Server status
- `GET  /api/v1/peer/{id}` — Peer online/offline status
- `POST /api/v1/file/upload` — Upload file (body: peer_id, local_path, remote_path, password)
- `POST /api/v1/file/download` — Download file (body: peer_id, remote_path, local_path, password)
- `POST /api/v1/peer/{id}/restart` — Restart peer
- `POST /api/v1/peer/{id}/shutdown` — Shutdown peer
- `POST /api/v1/peer/{id}/dir` — Create directory (body: path, password)
- `GET  /api/v1/peers` — List registered peers from hbbs database

### Auth
- `--login` — OAuth login, prints token
- `--option <key> [value]` — Get/set config options
- `--ipc-send <peer_id> <local> <remote>` — Send file via IPC tunnel

## Relay Infrastructure
- Self-hosted hbbs/hbbr with `--mask`, `-r`, `-k ""` for LAN detection
- ALWAYS_USE_RELAY forced for same-LAN peers
- Permanent password auth with SHA256(SHA256(password + salt) + challenge)
- install-server.bat creates ONSTART scheduled tasks

## Build

```bash
cargo build --release
python3 build.py        # Full build with vcpkg + packaging
```

## Usage

```bash
# Send a file to peer 254504451
rustdesk --send-file 254504451 /path/to/local/file.txt /remote/path/

# Receive a file from peer
rustdesk --recv-file 254504451 /remote/path/file.txt /local/path/

# List remote directory
rustdesk --list-dir 254504451 /home/user/documents
```

## Why fork instead of contribute upstream?

The upstream RustDesk project has no interest in headless/CLI file transfer — their focus is the GUI. This fork is additive (new flags only, zero changes to existing code paths) so it stays easy to rebase on upstream releases.

## License

AGPL-3.0 (same as upstream)
