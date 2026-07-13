# RustDesk Relay as Fleet Mesh Backbone

**Goal:** Use the self-hosted RustDesk relay (hbbs/hbbr) as a general-purpose transport layer for fleet MCP servers — not just for RustDesk GUI sessions.

## Why the Relay?

The relay already:
- Runs 24/7 as NSSM service, auto-start, log rotation
- Handles NAT traversal, peer discovery, and encrypted streams
- Connects any two peers anywhere (LAN, Tailscale, internet)
- Is proven: `--send-file` works end-to-end through the relay

The missing piece: a **relay protocol adaptor** that turns the relay stream into a general MCP transport, so any MCP server can proxy through it.

## Architecture

```
Peer A (Goliath)                         Peer B (minipc)
  │                                          │
  ├── hbbs/hbbr :21116/21117  ◄──────────────┤
  │    │                                       │
  │    └── relay fork sends file ──────────────┘
  │    └── relay MCP bridge ──────────────────►  MCP tools exposed  
  │    └── relay health metrics ──────────────►  Fleet monitoring
  │    └── relay remote exec ─────────────────►  Script execution
  │    └── relay sysinfo poll ────────────────►  System status
```

## Phase 1: Fleet Mesh Discovery (Next)

| Component | Description | Priority |
|-----------|-------------|----------|
| `GET /api/v1/peers` | List online peers from hbbs SQLite DB | HIGH |
| `GET /api/v1/peer/{id}/status` | Check if peer is online (exists) | HIGH |
| `POST /api/v1/peer/{id}/ping` | Ping peer through relay | MEDIUM |
| `GET /api/v1/relay/health` | hbbs/hbbr connection stats, bandwidth | MEDIUM |

The fork's `--api-server` already has `GET /api/v1/health`, `GET /api/v1/peer/{id}`, `POST /api/v1/file/upload`. Phase 1 adds peer discovery.

## Phase 2: MCP Relay Bridge

| Component | Description | Priority |
|-----------|-------------|----------|
| `--relay-bridge <port>` | Expose MCP stdio through relay tunnel | HIGH |
| `POST /api/v1/tunnel/{peer_id}` | Open relay tunnel to peer for MCP | HIGH |
| `--mcp-relay` | Start as relay MCP proxy | MEDIUM |

The bridge works by:
1. Fork connects to hbbs via `--send-file` protocol (already proven)
2. After relay pairing, instead of sending file data, it sends MCP JSON-RPC messages
3. Both sides see a bidirectional MCP stream through the relay

```python
# Concept: relay_mcp_bridge.py
async def relay_mcp_proxy(peer_id, mcp_port):
    stream = await establish_connection(peer_id)  # existing code
    await do_login(stream, peer_id, password)
    # Now stream is a bidirectional tunnel to the peer
    # Read MCP JSON-RPC from stdin, write to relay stream
    # Read from relay stream, write to MCP stdout
    async def forward_stdio_to_relay():
        async for msg in stdio:
            await stream.send(msg)
    async def forward_relay_to_stdio():
        async for msg in stream:
            await stdio.send(msg)
```

## Phase 3: Remote System Access

| Component | Description | Priority |
|-----------|-------------|----------|
| `--exec <peer_id> <script>` | Send script via relay, execute, return output | HIGH |
| `--sysinfo <peer_id>` | Pull system info (CPU, disk, processes, sensors) | HIGH |
| `--tail-log <peer_id> <path>` | Stream remote log file through relay | MEDIUM |
| `--tunnel <peer_id> <local_port>:<remote_host>:<remote_port>` | TCP tunnel through relay | MEDIUM |

## Phase 4: Health & Safety Monitoring

| Component | Description | Priority |
|-----------|-------------|----------|
| Inactivity detection | No movement detected for >4h via devices-mcp | HIGH |
| Glucose check | CGM data poll via Nightscout/Android | HIGH |
| Wakeup call | `speech-mcp` TTS "Sandra, are you okay?" | HIGH |
| Escalation | No response → email → call → minipc override | HIGH |
| Fallback | If Goliath unresponsive >10min, minipc escalates | HIGH |

### Surveillance State Machine

```
[Every 15min] surveillance_watch
    │
    ├── Health: NSSM services alive?
    ├── Logs: errors in last 24h?
    ├── Motion: devices-mcp detected movement?
    ├── Glucose: CGM readings in range? (future)
    └── Response: did Sandra interact with anything?
         │
         ├── Normal → log, sleep
         ├── Suspicious → check again in 5min
         ├── Warning → speech-mcp "You okay?"
         │       ├── Response → log, sleep
         │       └── No response → escalate
         └── Critical → email + robofang + minipc takeover
```

## Phase 5: Failover (Goliath → Minipc)

| Component | Description | Priority |
|-----------|-------------|----------|
| hbbs/hbbr on minipc | Secondary relay if Goliath goes down | HIGH |
| rsync config sync | Sync `data/` hbbs DB to minipc periodically | HIGH |
| Fleet agent second | Fritz duplicate on minipc | MEDIUM |
| Auto-failover | If Goliath unreachable >5min, minipc takes over | MEDIUM |

## Implementation Order

1. **Phase 1** — `GET /api/v1/peers`, peer discovery (adds SQLite query to api_server.rs)
2. **Phase 2** — MCP relay bridge (proven `--send-file` code + MCP framing)
3. **Phase 3** — `--exec`, `--sysinfo`, `--tail-log` using existing relay + script execution
4. **Phase 4** — Health monitoring via surveillance_watch + devices-mcp motion + speech-mcp
5. **Phase 5** — Minipc failover (separate hardware setup)

## File locations

- Plan: `D:\Dev\repos\rustdesk\docs\relay-mesh-plan.md`
- Fork CLI: `D:\Dev\repos\rustdesk\src\file_cli.rs` — `establish_connection`, `do_login`
- API: `D:\Dev\repos\rustdesk\src\api_server.rs`
- Fritz surveillance: `D:\Dev\repos\fleet-agent-mcp\src\fleet_agent\coworker\surveillance_watch.py`
- Workflow: `D:\Dev\repos\fleet-agent-mcp\workflows\security_escalation.yaml`
