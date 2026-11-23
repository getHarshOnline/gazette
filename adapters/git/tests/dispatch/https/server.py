"""Disposable loopback TLS/auth CGI transport for the actual native Git HTTP backend.

Authentication is a fixed test challenge, not provider emulation or product credential
storage. Each backend is reaped before its response is delivered (or deliberately lost
after its real effect).
"""

import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import socket
import ssl
import subprocess
import sys
from urllib.parse import urlsplit

root, backend = Path(sys.argv[1]), sys.argv[2]
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(root / "certificate.pem", root / "key.pem")


def record(value):
    with (root / "requests.jsonl").open("a", encoding="utf8") as output:
        output.write(json.dumps(value, sort_keys=True) + "\n")


class Server(http.server.HTTPServer):
    def get_request(self):
        connection, address = super().get_request()
        connection.settimeout(3)
        try:
            return context.wrap_socket(connection, server_side=True), address
        except Exception:
            connection.close()
            raise


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def do_GET(self):
        self.handle_git()

    def do_POST(self):
        self.handle_git()

    def respond(self, status, headers, body):
        self.send_response(status)
        for name, value in headers:
            if name.lower() not in ("status", "content-length", "connection"):
                self.send_header(name, value)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    def handle_git(self):
        selected = urlsplit(self.path)
        mode = (root / "mode").read_text(encoding="ascii")
        record({"event": "request", "method": self.command, "path": selected.path,
                "query": selected.query, "authorization": self.headers.get("Authorization"),
                "tls": self.connection.version()})
        if mode == "auth":
            self.respond(401, [("WWW-Authenticate", 'Basic realm="fixture"')], b"refused")
            return
        if mode == "authenticated" and self.headers.get("Authorization") != \
                "Basic Zml4dHVyZTpQUklWQVRFX0hFTFBFUl9TRUNSRVQ=":
            self.respond(401, [("WWW-Authenticate", 'Basic realm="fixture"')], b"required")
            return
        if mode == "redirect":
            self.respond(302, [("Location", "https://localhost:%d/redirect-target.git%s" %
                               (self.server.server_port, "?" + selected.query if selected.query else ""))], b"")
            return
        if self.headers.get("Transfer-Encoding") or selected.path not in (
                "/destination.git/info/refs", "/destination.git/git-receive-pack",
                "/destination.git/git-upload-pack"):
            self.respond(400, [], b"unsupported fixture request")
            return
        length = int(self.headers.get("Content-Length", "0"))
        if not 0 <= length <= 8 * 1024 * 1024:
            self.respond(413, [], b"fixture bound")
            return
        body = self.rfile.read(length)
        if len(body) != length:
            raise ValueError("incomplete request")
        environment = {
            "PATH": "/usr/bin:/bin", "HOME": str(root / "server-home"),
            "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_SYSTEM": "/dev/null",
            "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_PROJECT_ROOT": str(root),
            "GIT_HTTP_EXPORT_ALL": "1", "REQUEST_METHOD": self.command,
            "PATH_INFO": selected.path, "QUERY_STRING": selected.query,
            "CONTENT_TYPE": self.headers.get("Content-Type", ""),
            "CONTENT_LENGTH": str(length), "SERVER_PROTOCOL": self.protocol_version,
            "REMOTE_ADDR": "127.0.0.1", "LC_ALL": "C",
        }
        if self.headers.get("Git-Protocol"):
            environment["HTTP_GIT_PROTOCOL"] = self.headers["Git-Protocol"]
        child = subprocess.Popen([backend], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, env=environment, start_new_session=True)
        try:
            output, errors = child.communicate(body, timeout=8)
        except BaseException:
            os.killpg(child.pid, signal.SIGKILL)
            child.communicate(timeout=2)
            raise
        record({"event": "backend", "path": selected.path, "exit": child.returncode,
                "request_sha256": hashlib.sha256(body).hexdigest(),
                "response_sha256": hashlib.sha256(output).hexdigest(),
                "stderr": errors.decode("utf8", errors="replace")})
        if child.returncode:
            self.respond(500, [], b"native backend failed")
            return
        # Deliberately drop only AFTER receive-pack has completed its native ref transaction.
        if mode == "drop" and selected.path == "/destination.git/git-receive-pack":
            self.connection.shutdown(socket.SHUT_RDWR)
            self.connection.close()
            self.close_connection = True
            return
        head, separator, payload = output.partition(b"\r\n\r\n")
        if not separator:
            head, separator, payload = output.partition(b"\n\n")
        if not separator:
            raise ValueError("native CGI header missing")
        headers = []
        status = 200
        for line in head.decode("latin1").splitlines():
            name, value = line.split(":", 1)
            value = value.strip()
            if name.lower() == "status":
                status = int(value.split(" ", 1)[0])
            else:
                headers.append((name, value))
        self.respond(status, headers, payload)


server = Server(("127.0.0.1", 0), Handler)
server.timeout = 0.1
(root / "endpoint.ready").write_text("https://localhost:%d/destination.git" % server.server_port,
                                     encoding="ascii")
os.replace(root / "endpoint.ready", root / "endpoint")
try:
    while not (root / "stop-server").exists():
        server.handle_request()
finally:
    server.server_close()
