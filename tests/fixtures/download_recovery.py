#!/usr/bin/env python3
"""Offline HLS + audio fixture for real yt-dlp recovery tests (never contacts YouTube)."""
import json
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import subprocess
import sys
import threading
from urllib.parse import parse_qs, urlsplit


def serve(root):
    lock = threading.Lock()

    class Handler(SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(root), **kwargs)

        def log_message(self, *args):
            pass

        def do_GET(self):
            url = urlsplit(self.path)
            with lock, (root / "requests.jsonl").open("a") as log:
                log.write(json.dumps({"path": url.path, "query": url.query}) + "\n")
            if url.path == "/audio.m4a" and (root / "deny-audio").exists():
                self.send_error(403, "Synthetic media URL rejection")
                return
            if url.path == "/audio.m4a" and parse_qs(url.query).get("attempt") == ["1"]:
                self.send_error(403, "Synthetic expired media URL")
                return
            super().do_GET()

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    (root / "address").write_text(f"http://127.0.0.1:{server.server_port}")
    server.serve_forever()


def extract(root, binary, args):
    counter = root / "attempts"
    attempt = int(counter.read_text()) + 1 if counter.exists() else 1
    counter.write_text(str(attempt))
    with (root / "arguments.jsonl").open("a") as log:
        log.write(json.dumps(args) + "\n")
    address = (root / "address").read_text()
    info = {
        "id": "offline", "title": "Offline recovery fixture", "duration": 6,
        "extractor": "fixture", "webpage_url": "https://youtube.com/watch?v=fixture",
        "formats": [
            {"format_id": "613", "ext": "mp4", "protocol": "m3u8_native",
             "vcodec": "avc1", "acodec": "none", "height": 64,
             "url": address + "/video.m3u8"},
            {"format_id": "140", "ext": "m4a", "protocol": "http",
             "vcodec": "none", "acodec": "mp4a.40.2", "abr": 128,
             "filesize": (root / "audio.m4a").stat().st_size,
             "url": address + f"/audio.m4a?attempt={attempt}"},
        ],
    }
    manifest = root / f"info-{attempt}.json"
    manifest.write_text(json.dumps(info))
    args = args[:args.index("--")]
    result = subprocess.run([binary, *args, "--load-info-json", str(manifest),
                             "--no-simulate", "--retries", "0", "--fragment-retries", "0"],
                            capture_output=True, text=True)
    (root / f"wire-{attempt}.log").write_text(result.stdout + "\n" + result.stderr)
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", file=sys.stderr, flush=True)
    return result.returncode


if __name__ == "__main__":
    action, root = sys.argv[1], Path(sys.argv[2])
    if action == "serve":
        serve(root)
    elif action == "extract":
        sys.exit(extract(root, sys.argv[3], sys.argv[4:]))
    else:
        raise SystemExit("Unknown fixture action")
