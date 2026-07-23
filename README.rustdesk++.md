# rustdesk++ -- Enhanced RustDesk fork

**Forked from [rustdesk/rustdesk](https://github.com/rustdesk/rustdesk) v1.4.9**

This fork adds **headless CLI file transfer**, a **REST API**, and **multi-peer
bulk operations** to RustDesk, turning it from a GUI remote desktop into a
programmable remote desktop platform.

## Headless CLI

### Info

- `--status` -- Local RustDesk ID, service status, rendezvous/relay servers
- `--peer-info <peer_id>` -- Check if a peer is online
- `--get-id` -- Print local RustDesk ID
- `--version` -- Print version

### File Operations (relay-based, password optional for passwordless peers)

- `--send-file <id> <local> <remote> [password]` -- Send file to peer
- `--recv-file <id> <remote> <local> [password]` -- Receive file from peer
- `--list-dir <id> <path> [password]` -- List remote directory
- `--delete-remote <id> <path> [password]` -- Delete remote file
- `--move-remote <id> <old> <new> [password]` -- Move/rename remote file
- `--send-dir <id> <local_dir> <remote> [password]` -- Send directory contents
- `--create-dir <id> <path> [password]` -- Create remote directory

### Remote Control

- `--restart <id> [password]` -- Restart remote PC
- `--shutdown <id> [password]` -- Shutdown remote PC
- `--screenshot <id> <output> [password]` -- Capture remote screenshot

### Multi-Peer & Bulk Operations

- `--broadcast-file <id1,id2,...> <local> <remote> [password]` -- Send one file to N peers sequentially
- `--collect-files <id1,id2,...> <remote_dir> <local_dir> [password]` -- Pull all files from N peers matching a remote dir
- `--batch <manifest.json> [default_password]` -- Execute sequence of ops from a JSON manifest

### Server

- `--api-server [port]` -- Start HTTP API server (default 10806)
- `--ipc-send <peer_id> <local> <remote>` -- Send file via IPC tunnel

### Auth

- `--login` -- OAuth login, prints token
- `--option <key> [value]` -- Get/set config options

## REST API Server

### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/health` | Server status and version |
| GET | `/api/v1/peers` | List registered peers from hbbs database |
| GET | `/api/v1/peer/{id}` | Peer online/offline status |
| GET | `/api/v1/peer/{id}/status` | Same as `/api/v1/peer/{id}` |
| POST | `/api/v1/file/upload` | Upload file to peer (body: peer_id, local_path, remote_path, password) |
| POST | `/api/v1/file/download` | Download file from peer (body: peer_id, remote_path, local_path, password) |
| POST | `/api/v1/exec` | Execute script on peer (stub -- use CLI --send-file + --recv-file) |
| POST | `/api/v1/peer/{id}/restart` | Restart peer (body: password) |
| POST | `/api/v1/peer/{id}/shutdown` | Shutdown peer (body: password) |
| POST | `/api/v1/peer/{id}/dir` | Create directory on peer (body: path, password) |

### Stub Endpoints (use CLI instead)

| Method | Path | CLI alternative |
|--------|------|----------------|
| GET | `/api/v1/peer/{id}/screenshot` | `rustdesk --screenshot <id> <output> [password]` |
| GET | `/api/v1/peer/{id}/system` | `rustdesk --exec <id> "systeminfo.exe"` |
| GET | `/api/v1/peer/{id}/apps` | `rustdesk --exec <id> "wmic product get name"` |

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

## Usage Examples

```bash
# Send a file to peer 254504451
rustdesk --send-file 254504451 /path/to/local/file.txt /remote/path/

# Receive a file from peer
rustdesk --recv-file 254504451 /remote/path/file.txt /local/path/

# List remote directory
rustdesk --list-dir 254504451 /home/user/documents

# Create remote directory
rustdesk --create-dir 254504451 /home/user/newfolder

# Send directory contents to remote
rustdesk --send-dir 254504451 ./local_folder /remote/path/

# Restart remote PC
rustdesk --restart 254504451

# Shutdown remote PC
rustdesk --shutdown 254504451

# Capture remote screenshot
rustdesk --screenshot 254504451 screen.png

# Broadcast file to multiple peers
rustdesk --broadcast-file 254504451,254504452 /tmp/update.exe /C:/Deploy/

# Collect files from multiple peers
rustdesk --collect-files 254504451,254504452 /logs/ /local/logs/

# Batch ops from JSON manifest
rustdesk --batch manifest.json

# Start REST API server
rustdesk --api-server 10806
```

### Batch manifest format (JSON)

```json
{
  "password": "optional_default",
  "operations": [
    { "op": "send_file", "peer_id": "254504451", "local": "update.exe", "remote": "C:/Deploy/update.exe" },
    { "op": "restart", "peer_id": "254504451" },
    { "op": "create_dir", "peer_id": "254504452", "path": "C:/Data/NewFolder" },
    { "op": "recv_file", "peer_id": "254504452", "remote": "C:/Data/report.csv", "local": "./reports/" },
    { "op": "delete_remote", "peer_id": "254504452", "path": "C:/Temp/old.log" }
  ]
}
```

Supported `op` values: `send_file`, `recv_file`, `list_dir`, `delete_remote`, `move_remote`,
`send_dir`, `create_dir`, `restart`, `shutdown`, `screenshot`, `peer_info`.

## Why fork instead of contribute upstream?

The upstream RustDesk project has no interest in headless/CLI file transfer --
their focus is the GUI. This fork is additive (new flags only, zero changes to
existing code paths) so it stays easy to rebase on upstream releases.

## License

AGPL-3.0 (same as upstream)
