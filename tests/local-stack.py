"""Integration: python3 tests/local-stack.py (after scripts/build.sh)."""
import json
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time

REPO = Path(__file__).resolve().parents[1]
BINARY = REPO / "bin/fabric"

with tempfile.TemporaryDirectory(prefix="fab-local-", dir="/tmp") as directory:
    root = Path(directory)
    config = (REPO / "fabric.example.yaml").read_text().replace("path: ./examples", f"path: {REPO / 'examples'}")
    (root / "fabric.yaml").write_text(config)
    log = open(root / "launcher.log", "w+")

    def call(method, arguments):
        return json.loads(subprocess.check_output([str(BINARY), "call", method, json.dumps(arguments)], cwd=root, stderr=subprocess.PIPE, timeout=30))

    def start():
        process = subprocess.Popen([str(BINARY), "up"], cwd=root, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 180
            while time.monotonic() < deadline:
                assert process.poll() is None, (root / ".fabric/controls.log").read_text()
                try:
                    call("catalog", {})
                    return process
                except (subprocess.CalledProcessError, FileNotFoundError):
                    time.sleep(.1)
            raise AssertionError("local stack startup timeout")
        except BaseException:
            process.send_signal(signal.SIGINT)
            process.wait(timeout=40)
            raise

    def stop(process, sig=signal.SIGINT):
        endpoint = json.loads((root / ".fabric/controls/endpoint.json").read_text())["url"]
        from urllib.parse import urlparse
        address = urlparse(endpoint)
        process.send_signal(sig)
        assert process.wait(timeout=40) == 0
        assert not (root / ".fabric/controls/postgres-data/postmaster.pid").exists(), "PostgreSQL still running"
        connection = socket.socket(socket.AF_INET)
        try:
            connection.connect((address.hostname, address.port))
        except (ConnectionRefusedError, FileNotFoundError):
            pass
        else:
            raise AssertionError("Controls still running after shutdown")
        finally:
            connection.close()

    process = start()
    try:
        duplicate = subprocess.run([str(BINARY), "up"], cwd=root, capture_output=True, timeout=10)
        assert duplicate.returncode != 0 and b"already running" in duplicate.stderr
        accepted = call("invoke", {"request_key":"local-smoke", "package":"utility", "capability":"echo", "input":{"hello":"local"}})
        result = call("wait", {"id":accepted["id"], "seconds":20})
        assert result["state"] == "completed", result
        assert result["output"] == {"hello":"local"}, result
    finally:
        stop(process)
    process = start()
    try:
        restored = call("get", {"id":accepted["id"]})
        assert restored["state"] == "completed" and restored["output"] == result["output"]
    finally:
        stop(process, signal.SIGTERM)
    print("PASS: startup, duplicate rejection, invocation, Ctrl-C, durable restart, SIGTERM")
