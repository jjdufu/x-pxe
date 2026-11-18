#!/usr/bin/env python3
"""实验室 APT 回源代理（acng 的替身）：GET /<host>/<path> → http://<host>/<path>，带本地缓存。

用于宿主网络环境下 apt-cacher-ng 回源失败（HTTP 500）的情况。只允许 deb.debian.org。
用法：apt-proxy.py <监听地址> <端口> <缓存目录>
"""
import http.server, os, shutil, sys, urllib.request, urllib.error

ALLOWED = {"deb.debian.org"}
CACHE = sys.argv[3]
# 索引文件不缓存，避免 Release/InRelease 与 Packages 不一致
VOLATILE = ("Release", "Release.gpg", "InRelease", "Packages", "Packages.gz", "Packages.xz", "Sources.gz")


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802
        parts = self.path.lstrip("/").split("/", 1)
        if len(parts) != 2 or parts[0] not in ALLOWED or ".." in parts[1]:
            self.send_error(403)
            return
        host, rest = parts
        cached = os.path.join(CACHE, host, rest)
        volatile = rest.rsplit("/", 1)[-1] in VOLATILE or "/by-hash/" in rest
        if not volatile and os.path.isfile(cached):
            self._send_file(cached)
            return
        # 上游偶发断连：重试 3 次，避免 d-i 因一次 502 中断装机
        for attempt in range(3):
            try:
                self._fetch(f"http://{host}/{rest}", cached)
                break
            except urllib.error.HTTPError as e:
                self.send_error(e.code)
                return
            except OSError:
                if attempt == 2:
                    self.send_error(502)
                    return
        self._send_file(cached)

    def _fetch(self, url, cached):
        with urllib.request.urlopen(url, timeout=60) as resp:
            os.makedirs(os.path.dirname(cached), exist_ok=True)
            tmp = f"{cached}.part.{os.getpid()}.{id(self)}"
            with open(tmp, "wb") as out:
                shutil.copyfileobj(resp, out, 1 << 20)
            os.replace(tmp, cached)

    def _send_file(self, path):
        self.send_response(200)
        self.send_header("Content-Length", str(os.path.getsize(path)))
        self.end_headers()
        with open(path, "rb") as f:
            shutil.copyfileobj(f, self.wfile, 1 << 20)

    def log_message(self, fmt, *args):
        sys.stderr.write("apt-proxy: " + (fmt % args) + "\n")


http.server.ThreadingHTTPServer((sys.argv[1], int(sys.argv[2])), Handler).serve_forever()
