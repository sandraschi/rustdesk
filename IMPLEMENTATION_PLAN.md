# rustdesk++ Implementation Plan

## Status Key

- ✅ **Done** — tested and working
- 🟡 **Implemented, needs test** — code written, not yet verified
- 🔜 **Planned** — design clear, not started
- 💡 **Proposed** — needs requirements discussion

---

## Phase 0: Relay Infrastructure (Done)

| Item | Status | Notes |
|------|--------|-------|
| hbbs `--mask` for LAN detection | ✅ Done | `--mask 192.168.0.0/24` gives correct `local_ip` |
| hbbs `-r` for relay servers | ✅ Done | `-r 192.168.0.81:21117` |
| hbbs ALWAYS_USE_RELAY | ✅ Done | Hardcoded `AtomicBool::new(true)` |
| Disable key enforcement | ✅ Done | `hbbs -k ""` skips licence_key check |
| RegisterPk doesn't kill TCP | ✅ Done | `return true` after NOT_SUPPORT response |
| same_intranet bypass for relay | ✅ Done | `!ALWAYS_USE_RELAY` guard on same_intranet |
| REG_TIMEOUT 300s | ✅ Done | Raised from 30s to 300s |
| install-server.bat with args | 🟡 | Needs update to pass `--mask`, `-r`, `-k ""` |
| Scheduled task persistence | 🟡 | Must survive reboot |

## Phase 1: Headless CLI (Login Handshake)

| Item | Status | Notes |
|------|--------|-------|
| `--help` | ✅ Done | All commands listed |
| `--status` | ✅ Done | ID, service, rendezvous/relay |
| `--peer-info <id>` | ✅ Done | Online/offline check via hbbs |
| `--send-file <id> <local> <remote> [pwd]` | ✅ Done | Full pipeline: relay → SignedId → PublicKey → Hash → SHA256 → LoginResponse → FileAction |
| `--recv-file` | 🟡 | Same do_login flow, needs test |
| `--list-dir` | 🟡 | Same flow, needs test |
| `--delete-remote` | 🟡 | Same flow, needs test |
| `--move-remote` | 🟡 | Same flow, needs test |
| `--send-dir` | 🟡 | Iterates send_file per file |
| SHA256 password hashing | ✅ Done | `SHA256(SHA256(pwd + salt) + challenge)` |
| Raw password fallback | ✅ Done | Tries hashed first, then raw |

## Phase 2: REST API Server (`--api-server`)

**Current state:** Bare-bones TCP listener with 3 endpoints. Need to move to a proper HTTP framework.

| Item | Priority | Notes |
|------|----------|-------|
| Upgrade to `tiny_http` or `actix-web` | 🔜 | Current raw TCP listener is fragile |
| `GET /api/v1/health` | ✅ Done | Returns status + version |
| `POST /api/v1/file/upload` | 💡 | Accept peer_id + paths, returns stream handle |
| `POST /api/v1/file/download` | 💡 | Accept peer_id + remote_path, streams back |
| `POST /api/v1/exec` | 💡 | Remote script execution (send .ps1 → run → recv result) |
| `GET /api/v1/peers` | 💡 | List online/offline peers from hbbs DB |
| `GET /api/v1/status` | 💡 | Relay health, bandwidth, connected peers |
| `GET /api/v1/peer/{id}/status` | 💡 | Wraps `--peer-info`, returns online + relay info |

## Phase 3: Multi-Peer & Bulk Operations

| Item | Priority | Notes |
|------|----------|-------|
| `--broadcast-file <id1,id2,...>` | 💡 | Send one file to N peers sequentially |
| `--collect-files <ids...> <glob> <dir>` | 💡 | Pull matching files from multiple peers |
| `--sync-dir <id> <local> <remote>` | 💡 | Bidirectional directory sync (walk tree, diff, xfer) |
| `--mirror <id> <local> <remote>` | 💡 | Full recursive mirror (delete extraneous) |
| `--batch <manifest.json>` | 💡 | Execute sequence of ops from JSON |

## Phase 4: Cross-Fleet MCP Integration

| Item | Priority | Notes |
|------|----------|-------|
| `rustdesk --mcp` — native MCP server | 💡 | Exposes file ops as MCP tools for Cursor/Claude |
| `GET /api/v1/metrics` for monitoring-mcp | 💡 | Memory/disk/uptime from remote via relay |
| `--script <peer_id> <script_type>` | 💡 | "ps1" / "python" / "bash" — upload, exec, return result |
| fleet discovery integration | 💡 | Other MCP servers can discover peers via hbbs API |

## Phase 5: Infrastructure Hardening

| Item | Priority | Notes |
|------|----------|-------|
| Update `install-server.bat` | 🔜 | Must pass `--mask`, `-r`, `-k ""`, `ALWAYS_USE_RELAY=Y` |
| Update `restart-service.bat` | 🔜 | Same args as above |
| hbbs/hbbr as Windows service | 💡 | Instead of scheduled task |
| Health-check integration in webapp | 💡 | Show relay status in rustdesk-mcp dashboard |
| One-command setup: `--setup-relay` | 💡 | Auto-configures everything |

## Phase 6: Fix Remaining Bugs

| Bug | Status | Notes |
|-----|--------|-------|
| `--send-file` hash/challenge auth still fails | 🟡 | Works with debug hbbs (`-k ""`). Release hbbs with key enabled needs licence_key fix |
| Relay pairing race (raw mode bytes split) | 💡 | BytesCodec raw mode can split protobuf messages |
| Relay timeout on first punch | 🟡 | 30s timeout works but slow. Investigate why minipc relay response takes so long |

## Build & Release

| Item | Priority | Notes |
|------|----------|-------|
| GitHub release with pre-built binary | 💡 | `cargo build --release`, upload .exe |
| CI workflow for release builds | 💡 | GitHub Actions on tag push |
| Chocolatey / winget package | 💡 | For easy `winget install rustdesk++` |

---

## Priority Order

1. **Test `--recv-file`, `--list-dir`, `--delete-remote`, `--move-remote`** with `Sec10000` — same code path as `--send-file`, should work immediately
2. **Update `install-server.bat`** with `--mask`, `-r`, `-k ""` — relay survival across reboots
3. **Build release binary** of hbbs with all 3 fixes, ship as `hbbs.exe` replacement
4. **Implement API server** with `tiny_http` or `actix-web` for REST endpoints
5. **Add `--batch`** for JSON-based multi-op manifests
6. **Add `--mcp`** for native MCP protocol server
7. **Cross-fleet integration** with monitoring-mcp, fleet-agent-mcp
