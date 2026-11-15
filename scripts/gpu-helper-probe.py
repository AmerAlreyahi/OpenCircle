#!/usr/bin/env python3
"""Asks the GPU translation helper for its graphics cards over the app's framing
(u32 LE length of the rest, u64 LE request id, JSON) and checks it answers.

    gpu-helper-probe.py <opencircle-mt-gpu exe>

CI and release.yml run it on runners without a GPU: the reply must still come
(`"ok": true`, `"gpu": false`), never a crash.
"""
import json
import struct
import subprocess
import sys


def frame(rid, obj):
    payload = json.dumps(obj).encode()
    return struct.pack("<IQ", len(payload) + 8, rid) + payload


def main():
    exe = sys.argv[1]
    request = frame(1, {"op": "probe", "model_bytes": 1133080448}) + frame(2, {"op": "quit"})
    out = subprocess.run([exe], input=request, capture_output=True, timeout=120).stdout
    if len(out) < 12:
        sys.exit(f"no reply from {exe}")
    length, rid = struct.unpack("<IQ", out[:12])
    reply = json.loads(out[12:4 + length])
    print(reply)
    if rid != 1 or reply.get("ok") is not True or "devices" not in reply:
        sys.exit("bad probe reply")


if __name__ == "__main__":
    main()
