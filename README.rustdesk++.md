# rustdesk++ — Enhanced RustDesk fork

**Forked from [rustdesk/rustdesk](https://github.com/rustdesk/rustdesk) v1.4.9**

This fork adds **headless CLI file transfer** and a **REST API** to RustDesk, turning it from a GUI remote desktop into a programmable remote desktop platform.

## What's added (rustdesk++)

### Headless File Transfer CLI

| Flag | Action |
|------|--------|
| `--send-file <peer_id> <local_path> <remote_path>` | Upload file to remote peer |
| `--recv-file <peer_id> <remote_path> <local_path>` | Download file from remote peer |
| `--list-dir <peer_id> <remote_path>` | List remote directory |

No GUI needed. Uses the same rendezvous/relay protocol as the GUI (hbbs:21116, hbbr:21117).

### Upcoming

- `--api-server <port>` — REST API for file operations
- `--mcp` — Native MCP protocol server (stdio JSON-RPC)

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
