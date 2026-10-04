#!/usr/bin/env python3
"""A tiny stand-in for GitHub's OAuth web flow and REST /user endpoint.

Used by the end-to-end tests (MINREGISTRY_GITHUB_URL / MINREGISTRY_GITHUB_API_URL
point here). The sign-in identity is chosen by appending `&login=<name>` to the
authorize URL; without it FAKE_GITHUB_DEFAULT_LOGIN is used.

Environment:
  FAKE_GITHUB_PORT            listen port (default 5555)
  FAKE_GITHUB_CLIENT_ID       expected client_id (optional)
  FAKE_GITHUB_CLIENT_SECRET   expected client_secret (optional)
  FAKE_GITHUB_DEFAULT_LOGIN   login used when none is given (default e2e-admin)
"""

import hashlib
import json
import os
import secrets
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlencode, urlparse

CLIENT_ID = os.environ.get("FAKE_GITHUB_CLIENT_ID")
CLIENT_SECRET = os.environ.get("FAKE_GITHUB_CLIENT_SECRET")
DEFAULT_LOGIN = os.environ.get("FAKE_GITHUB_DEFAULT_LOGIN", "e2e-admin")

codes = {}  # code -> login
tokens = {}  # access token -> login
lock = threading.Lock()


def user_id(login):
    return int.from_bytes(hashlib.sha256(login.lower().encode()).digest()[:6], "big")


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):  # keep test output quiet
        pass

    def send_json(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        url = urlparse(self.path)
        q = {k: v[0] for k, v in parse_qs(url.query).items()}
        if url.path == "/healthz":
            return self.send_json(200, {"ok": True})
        if url.path == "/login/oauth/authorize":
            if CLIENT_ID and q.get("client_id") != CLIENT_ID:
                return self.send_json(400, {"error": "unknown client_id"})
            if q.get("scope") != "read:user":
                return self.send_json(400, {"error": "unexpected scope", "scope": q.get("scope")})
            code = secrets.token_hex(16)
            with lock:
                codes[code] = q.get("login", DEFAULT_LOGIN)
            target = q["redirect_uri"] + "?" + urlencode({"code": code, "state": q.get("state", "")})
            self.send_response(302)
            self.send_header("Location", target)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None
        if url.path == "/user":
            auth = self.headers.get("Authorization", "")
            token = auth.split(" ", 1)[1] if " " in auth else ""
            with lock:
                login = tokens.get(token)
            if not login:
                return self.send_json(401, {"message": "Bad credentials"})
            return self.send_json(200, {"login": login, "id": user_id(login), "name": login.title()})
        return self.send_json(404, {"message": "Not Found"})

    def do_POST(self):
        url = urlparse(self.path)
        length = int(self.headers.get("Content-Length", "0"))
        form = {k: v[0] for k, v in parse_qs(self.rfile.read(length).decode()).items()}
        if url.path != "/login/oauth/access_token":
            return self.send_json(404, {"message": "Not Found"})
        if CLIENT_ID and form.get("client_id") != CLIENT_ID:
            return self.send_json(200, {"error": "incorrect_client_credentials"})
        if CLIENT_SECRET and form.get("client_secret") != CLIENT_SECRET:
            return self.send_json(200, {"error": "incorrect_client_credentials"})
        with lock:
            login = codes.pop(form.get("code", ""), None)
            if login is None:
                return self.send_json(200, {"error": "bad_verification_code"})
            token = "gho_" + secrets.token_hex(16)
            tokens[token] = login
        return self.send_json(200, {"access_token": token, "token_type": "bearer", "scope": "read:user"})


def main():
    port = int(os.environ.get("FAKE_GITHUB_PORT", "5555"))
    server = ThreadingHTTPServer(("0.0.0.0", port), Handler)
    print("fake GitHub listening on :%d" % port, flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
