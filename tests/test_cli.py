#!/usr/bin/env python3
"""rustdesk++ CLI smoke tests. Requires: relay running, peer connected, password known.

Usage:
    uv run python tests/test_cli.py [--password Sec10000] [--peer 254504451]

Environment:
    RUSTDESK_CLI      path to rustdesk.exe (default: ../target/debug/rustdesk.exe)
    RUSTDESK_PEER     peer ID to test against (default: 254504451)
    RUSTDESK_PASSWORD password for peer (default: Sec10000)
    HBB_PORT          hbbs port (default: 21116)
    API_PORT          API server port (default: 10806)
"""

import os
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.error
import json

CLI = os.environ.get("RUSTDESK_CLI", os.path.join(os.path.dirname(__file__), "..", "target", "debug", "rustdesk.exe"))
PEER = os.environ.get("RUSTDESK_PEER", "254504451")
PASSWORD = os.environ.get("RUSTDESK_PASSWORD", "Sec10000")
API_PORT = os.environ.get("API_PORT", "10806")

passed = 0
failed = 0

def run_cli(*args, timeout=60):
    cmd = [CLI] + list(args)
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return r.returncode == 0, r.stdout, r.stderr
    except subprocess.TimeoutExpired:
        return False, "", "TIMEOUT"
    except FileNotFoundError:
        return False, "", f"CLI not found: {CLI}"

def api_get(path):
    try:
        r = urllib.request.urlopen(f"http://127.0.0.1:{API_PORT}{path}", timeout=10)
        return r.status, json.loads(r.read())
    except Exception as e:
        return 0, {"error": str(e)}

def check(name, ok, detail=""):
    global passed, failed
    if ok:
        passed += 1
        print(f"  PASS  {name}")
    else:
        failed += 1
        print(f"  FAIL  {name}" + (f"  -- {detail}" if detail else ""))

def main():
    global passed, failed
    print(f"\n rustdesk++ CLI Smoke Tests")
    print(f"  CLI:      {CLI}")
    print(f"  Peer:     {PEER}")
    print(f"  Password: {'<set>' if PASSWORD else '<empty>'}")
    print()

    # --- Prerequisites ---
    ok, out, err = run_cli("--help")
    check("--help prints usage", ok and "send-file" in out and "recv-file" in out, err)

    ok, out, err = run_cli("--status")
    check("--status returns ID and service", ok and "ID:" in out and "1413488068" in out, err)

    ok, out, err = run_cli("--version")
    check("--version prints version", ok and "1.4.9" in out, err)

    ok, out, err = run_cli("--peer-info", PEER)
    check("--peer-info shows online", ok and ("online" in out or "offline" in out), err)

    # --- File Operations ---
    tmp = tempfile.NamedTemporaryFile(delete=False, suffix=".txt", mode="w")
    tmp.write("rustdesk++ test file\n")
    tmp.close()
    test_file = tmp.name

    # Send file
    remote = f"C:\\Users\\minipc\\Desktop\\clitest_{int(time.time())}.txt"
    ok, out, err = run_cli("--send-file", PEER, test_file, remote, PASSWORD, timeout=120)
    check("--send-file transfers file", ok, err)

    # List directory
    ok, out, err = run_cli("--list-dir", PEER, "C:\\Users\\minipc\\Desktop", PASSWORD, timeout=120)
    check("--list-dir shows Desktop contents", ok and "clitest_" in out, err)

    # Create directory
    test_dir = f"C:\\Users\\minipc\\Desktop\\clitest_dir_{int(time.time())}"
    ok, out, err = run_cli("--create-dir", PEER, test_dir, PASSWORD, timeout=120)
    check("--create-dir creates remote directory", ok, err)

    # Delete remote file
    ok, out, err = run_cli("--delete-remote", PEER, remote, PASSWORD, timeout=120)
    check("--delete-remote removes file", ok, err)

    # --- API Server ---
    r = api_get("/api/v1/health")
    if r[0] == 200:
        passed += 1
        print(f"  PASS  API /health")
    else:
        failed += 1
        print(f"  FAIL  API /health: {r}")

    r = api_get(f"/api/v1/peer/{PEER}")
    if r[0] == 200 and r[1].get("status") in ("online", "offline"):
        passed += 1
        print(f"  PASS  API /peer/{PEER}")
    else:
        failed += 1
        print(f"  FAIL  API /peer/{PEER}: {r}")

    # --- Cleanup ---
    os.unlink(test_file)

    print(f"\n  Results: {passed} passed, {failed} failed")
    return 0 if failed == 0 else 1

if __name__ == "__main__":
    sys.exit(main())
