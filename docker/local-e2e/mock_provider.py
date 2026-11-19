"""Offline OAuth and Codex fixture. Never use it with real credentials."""

import base64
import hashlib
import json
import secrets
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit


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
        self.devices = {}
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
        if request.path != "/codex/device":
            return self.send(404, {"error": "not_found"})
        self.send(
            200,
            "<!doctype html><title>Local device sign-in fixture</title>"
            "<h1>Local OpenAI test sign-in</h1>"
            "<p>This isolated fixture uses no real account.</p>"
            '<form method="post" action="/codex/device">'
            '<label>One-time code <input name="user_code" required></label>'
            '<button name="decision" value="approve">Approve test sign-in</button>'
            '<button name="decision" value="deny">Decline</button></form>',
            "text/html; charset=utf-8",
        )

    def do_POST(self):
        path = urlsplit(self.path).path
        try:
            body = self.form()
            if path == "/codex/device":
                return self.authorize_device(parse_qs(body.decode()))
            if path == "/api/accounts/deviceauth/usercode":
                return self.issue_device(json.loads(body))
            if path == "/api/accounts/deviceauth/token":
                return self.poll_device(json.loads(body))
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

    def issue_device(self, params):
        client = params["client_id"]
        if not isinstance(client, str) or not client:
            return self.send(400, {"error": "invalid_client"})
        device_id = secrets.token_urlsafe(24)
        code = secrets.token_hex(4).upper()
        user_code = code[:4] + "-" + code[4:]
        with STATE.lock:
            now = time.monotonic()
            STATE.devices = {
                key: record for key, record in STATE.devices.items()
                if record["expires"] > now
            }
            STATE.devices[device_id] = {
                "client": client, "user_code": user_code, "expires": now + 90,
                "decision": None,
            }
        return self.send(200, {"device_auth_id": device_id, "user_code": user_code, "interval": "1"})

    def authorize_device(self, params):
        decision = params["decision"][0]
        if decision not in {"approve", "deny"}:
            raise ValueError("Unknown decision")
        with STATE.lock:
            record = next((
                record for record in STATE.devices.values()
                if record["user_code"] == params["user_code"][0]
                and record["expires"] > time.monotonic()
                and record["decision"] is None
            ), None)
            if record is None:
                return self.send(400, {"error": "invalid_device_code"})
            record["decision"] = decision
            record["redirect"] = f"http://{self.headers['Host']}/deviceauth/callback"
        self.send(
            200,
            "<!doctype html><title>Device authorization complete</title>"
            "<h1>Return to Albert</h1>",
            "text/html; charset=utf-8",
        )

    def poll_device(self, params):
        with STATE.lock:
            device_id = params["device_auth_id"]
            record = STATE.devices.get(device_id)
            if not record or record["expires"] <= time.monotonic():
                return self.send(404, {"error": "not_found"})
            if record["user_code"] != params["user_code"]:
                return self.send(400, {"error": "invalid_device_code"})
            if record["decision"] is None:
                return self.send(403, {"error": "authorization_pending"})
            del STATE.devices[device_id]
            if record["decision"] == "deny":
                return self.send(400, {"error": "access_denied"})
            code = secrets.token_urlsafe(24)
            verifier = secrets.token_urlsafe(32)
            challenge = encoded(hashlib.sha256(verifier.encode()).digest())
            STATE.codes[code] = {**record, "challenge": challenge}
        return self.send(200, {"authorization_code": code, "code_verifier": verifier, "code_challenge": challenge})

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
