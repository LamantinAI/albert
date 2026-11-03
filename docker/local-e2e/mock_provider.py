"""Offline OAuth and Codex fixture. Never use it with real credentials."""

import base64
import hashlib
import html
import json
import secrets
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlencode, urlsplit


def encoded(value):
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode()


def jwt(account):
    claims = {
        "exp": int(time.time()) + 3600,
        "jti": secrets.token_hex(8),
        "sub": account,
        "https://api.openai.com/auth": {
            "chatgpt_account_id": account,
            "chatgpt_plan_type": "local-fixture",
        },
    }
    return ".".join(
        [encoded(b'{"alg":"none"}'), encoded(json.dumps(claims).encode()), "fixture"]
    )


class State:
    def __init__(self):
        self.lock = threading.Lock()
        self.codes = {}
        self.refreshes = set()
        self.accesses = set()
        self.counts = {"exchanges": 0, "refreshes": 0, "responses": 0, "catalogs": 0}
        self.reject_next_response = False
        self.catalog_failures = 0

    def tokens(self):
        account = "local-fixture-account"
        # A random claim makes rotation observable without recording token bytes.
        access = jwt(account)
        refresh = "fixture-refresh-" + secrets.token_urlsafe(24)
        self.accesses.add(access)
        self.refreshes.add(refresh)
        return {
            "access_token": access,
            "refresh_token": refresh,
            "id_token": jwt(account),
            "token_type": "Bearer",
            "expires_in": 3600,
        }


STATE = State()


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        # The default access log includes authorization query strings.
        pass

    def send(self, status, body, content_type="application/json"):
        data = body.encode() if isinstance(body, str) else json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def form(self):
        size = int(self.headers.get("Content-Length", "0"))
        if not 0 <= size <= 2 * 1024 * 1024:
            raise ValueError("Body limit")
        return self.rfile.read(size)

    def do_GET(self):
        request = urlsplit(self.path)
        if request.path == "/health":
            return self.send(200, {"ready": True, "fixture": True})
        if request.path == "/metrics":
            with STATE.lock:
                return self.send(200, dict(STATE.counts))
        if request.path == "/backend-api/codex/models":
            return self.catalog(parse_qs(request.query))
        if request.path != "/oauth/authorize":
            return self.send(404, {"error": "not_found"})
        params = parse_qs(request.query, keep_blank_values=True)
        required = ("state", "code_challenge", "redirect_uri", "client_id")
        if any(len(params.get(key, [])) != 1 or not params[key][0] for key in required):
            return self.send(400, {"error": "invalid_authorization"})
        callback = urlsplit(params["redirect_uri"][0])
        if (
            callback.scheme != "http"
            or callback.hostname != "localhost"
            or callback.port != 1455
            or callback.path != "/auth/callback"
            or params.get("code_challenge_method") != ["S256"]
        ):
            return self.send(400, {"error": "invalid_redirect_or_pkce"})
        hidden = "".join(
            f'<input type="hidden" name="{html.escape(key)}" '
            f'value="{html.escape(values[0], quote=True)}">'
            for key, values in params.items()
        )
        self.send(
            200,
            "<!doctype html><title>Local authorization fixture</title>"
            "<h1>Local OpenAI test sign-in</h1>"
            "<p>This isolated fixture uses no real account.</p>"
            f'<form method="post" action="/oauth/authorize">{hidden}'
            '<button name="decision" value="approve">Approve test sign-in</button>'
            '<button name="decision" value="deny">Decline</button></form>',
            "text/html; charset=utf-8",
        )

    def do_POST(self):
        path = urlsplit(self.path).path
        try:
            body = self.form()
            if path == "/oauth/authorize":
                return self.authorize(parse_qs(body.decode()))
            if path == "/oauth/token":
                if self.headers.get("Content-Type", "").startswith("application/json"):
                    params = {key: [value] for key, value in json.loads(body).items()}
                else:
                    params = parse_qs(body.decode())
                return self.token(params)
            if path == "/test-control":
                controls = json.loads(body)
                with STATE.lock:
                    if "reject_next_response" in controls:
                        STATE.reject_next_response = bool(controls["reject_next_response"])
                    if "catalog_failures" in controls:
                        STATE.catalog_failures = max(0, min(10, int(controls["catalog_failures"])))
                return self.send(200, {"configured": True})
            if path.endswith("/responses"):
                return self.codex_response(json.loads(body))
            return self.send(404, {"error": "not_found"})
        except (ValueError, KeyError, TypeError):
            return self.send(400, {"error": "invalid_request"})

    def authorize(self, params):
        if params.get("decision") == ["deny"]:
            query = {"error": "access_denied", "state": params["state"][0]}
        else:
            code = secrets.token_urlsafe(24)
            with STATE.lock:
                STATE.codes[code] = {
                    "challenge": params["code_challenge"][0],
                    "redirect": params["redirect_uri"][0],
                    "client": params["client_id"][0],
                    "expires": time.monotonic() + 90,
                }
            query = {"code": code, "state": params["state"][0]}
        callback = params["redirect_uri"][0] + "?" + urlencode(query)
        self.send(
            200,
            "<!doctype html><title>Authorization complete</title>"
            "<h1>Return to Albert</h1><label>Returned address"
            f'<input readonly value="{html.escape(callback, quote=True)}"></label>'
            "<p>Paste this address into Albert to finish this test sign-in.</p>",
            "text/html; charset=utf-8",
        )

    def token(self, params):
        with STATE.lock:
            grant = params.get("grant_type", [""])[0]
            if grant == "authorization_code":
                record = STATE.codes.pop(params.get("code", [""])[0], None)
                challenge = encoded(hashlib.sha256(params.get("code_verifier", [""])[0].encode()).digest())
                if (
                    not record
                    or record["expires"] < time.monotonic()
                    or record["challenge"] != challenge
                    or params.get("redirect_uri") != [record["redirect"]]
                    or params.get("client_id") != [record["client"]]
                ):
                    return self.send(400, {"error": "invalid_grant"})
                STATE.counts["exchanges"] += 1
            elif grant == "refresh_token":
                refresh = params.get("refresh_token", [""])[0]
                if refresh not in STATE.refreshes:
                    return self.send(400, {"error": "invalid_grant"})
                STATE.refreshes.remove(refresh)
                STATE.counts["refreshes"] += 1
            else:
                return self.send(400, {"error": "unsupported_grant_type"})
            return self.send(200, STATE.tokens())

    def catalog(self, params):
        token = self.headers.get("Authorization", "").removeprefix("Bearer ")
        with STATE.lock:
            if token not in STATE.accesses or self.headers.get("ChatGPT-Account-ID") != "local-fixture-account":
                return self.send(401, {"error": {"code": "token_expired"}})
            STATE.counts["catalogs"] += 1
            if STATE.catalog_failures:
                STATE.catalog_failures -= 1
                return self.send(503, {"error": {"code": "server_error"}})
        if not params.get("client_version"):
            return self.send(400, {"error": {"code": "invalid_request"}})
        self.send(200, {"models": [
            {"slug": "fixture-chat", "display_name": "Fixture Chat", "description": "A local model for reasoning, images and tool use.", "visibility": "list", "supported_in_api": False, "tool_mode": "code_mode_only", "input_modalities": ["text", "image"], "context_window": 128000, "priority": 1, "default_reasoning_level": "medium", "supported_reasoning_levels": [{"reasoning_effort": "low", "description": "Quick answers"}, {"reasoning_effort": "medium", "description": "Balanced reasoning"}]},
            {"slug": "fixture-fast", "display_name": "Fixture Fast", "description": "A small text model for quick replies.", "visibility": "list", "supported_in_api": False, "tool_mode": "code_mode_only", "input_modalities": ["text"], "context_window": 64000, "priority": 2, "default_reasoning_level": "low", "supported_reasoning_levels": [{"reasoning_effort": "low", "description": "Quick answers"}]},
            {"slug": "fixture-hidden", "display_name": "Hidden Fixture", "description": "Not part of the picker.", "visibility": "hide", "input_modalities": ["text"], "tool_mode": "direct"},
        ]})

    def codex_response(self, body):
        token = self.headers.get("Authorization", "").removeprefix("Bearer ")
        with STATE.lock:
            if token not in STATE.accesses:
                return self.send(401, {"detail": "token_expired"})
            if STATE.reject_next_response:
                STATE.reject_next_response = False
                return self.send(401, {"detail": "token_expired"})
            STATE.counts["responses"] += 1
        answer = "Local subscription is connected. This reply came from the offline Docker fixture."
        item = {"id": "fixture-message", "type": "message", "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": answer, "annotations": []}]}
        response = {"id": "fixture-response", "object": "response", "created_at": 0, "status": "completed", "model": body.get("model", "fixture"), "output": [item], "usage": {"input_tokens": 12, "output_tokens": 16, "total_tokens": 28}}
        events = [
            {"type": "response.created", "response": {**response, "status": "in_progress", "output": []}},
            {"type": "response.output_item.added", "output_index": 0, "item": {**item, "status": "in_progress", "content": []}},
            {"type": "response.content_part.added", "item_id": item["id"], "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": "", "annotations": []}},
            {"type": "response.output_text.delta", "item_id": item["id"], "output_index": 0, "content_index": 0, "delta": answer},
            {"type": "response.output_text.done", "item_id": item["id"], "output_index": 0, "content_index": 0, "text": answer},
            {"type": "response.output_item.done", "output_index": 0, "item": item},
            {"type": "response.completed", "response": response},
        ]
        stream = "".join(f"event: {event['type']}\ndata: {json.dumps({**event, 'sequence_number': index})}\n\n" for index, event in enumerate(events))
        self.send(200, stream, "text/event-stream")


if __name__ == "__main__":
    ThreadingHTTPServer(("0.0.0.0", 8000), Handler).serve_forever()
