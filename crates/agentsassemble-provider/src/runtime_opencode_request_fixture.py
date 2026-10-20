#!/usr/bin/env python3
"""Local OpenCode 2 protocol fixture; never loads a provider or records authentication headers."""
import http.server
import json
import os
import pathlib
import queue
import sys
import threading
import urllib.parse

events = queue.Queue()
answered = threading.Event()
model = {"providerID": "opencode", "id": "fixture"}


def event(kind, **data):
    events.put(
        {
            "id": f"evt_{kind}",
            "created": 1,
            "type": kind,
            "data": {"sessionID": "session-1", **data},
        }
    )


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def reply(self, value, status=200):
        data = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def no_content(self):
        self.send_response(204)
        self.end_headers()

    def do_GET(self):
        path = urllib.parse.urlparse(self.path).path
        if path == "/api/info":
            return self.reply({"version": "2.0.32", "pid": 1, "urls": []})
        if path == "/api/session/session-1":
            return self.reply({"data": {"id": "session-1"}})
        if path == "/api/mcp":
            return self.reply(
                {
                    "location": {"directory": os.getcwd()},
                    "data": [
                        {
                            "name": "agentsassemble_room",
                            "status": {"status": "connected"},
                        }
                    ],
                }
            )
        if path != "/api/event":
            return self.send_error(404)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.flush()
        while True:
            value = events.get(timeout=10)
            self.wfile.write(("data: " + json.dumps(value) + "\n\n").encode())
            self.wfile.flush()
            if value["type"] == "session.execution.succeeded":
                return

    def do_PUT(self):
        path = urllib.parse.urlparse(self.path).path
        payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if path != "/api/experimental/mcp/agentsassemble_room":
            return self.send_error(404)
        pathlib.Path("mcp-request.json").write_text(json.dumps(payload))
        self.no_content()

    def do_POST(self):
        path = urllib.parse.urlparse(self.path).path
        payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if path == "/api/location/reload":
            config = pathlib.Path(os.environ["OPENCODE_CONFIG_DIR"]) / "opencode.json"
            settings = json.loads(config.read_text())
            assert "agent" not in settings
            instruction = pathlib.Path(settings["instructions"][0])
            assert instruction.is_absolute() and instruction.parent == config.parent
            pathlib.Path("room-instructions.txt").write_text(instruction.read_text())
            with open("agent-refreshes.jsonl", "a") as log:
                log.write(config.read_text() + "\n")
            return self.no_content()
        if path == "/api/session":
            pathlib.Path("session-request.json").write_text(json.dumps(payload))
            return self.reply({"data": {"id": "session-1"}})
        if path == "/api/session/session-1/permission/permission-1/reply":
            pathlib.Path("native-reply.json").write_text(json.dumps(payload))
            self.no_content()
            answered.set()
            return
        if path != "/api/session/session-1/prompt":
            return self.send_error(404)
        answered.clear()
        pathlib.Path("prompt-request.json").write_text(json.dumps(payload))
        event("session.inbox.delivered", inboxID="user-1")
        event(
            "permission.asked",
            id="permission-1",
            action="bash",
            resources=["echo fixture"],
        )
        if not answered.wait(10):
            return self.send_error(504)
        event(
            "session.step.started",
            assistantMessageID="assistant-1",
            agent="build",
            model=model,
            started=1,
        )
        event(
            "session.text.ended",
            assistantMessageID="assistant-1",
            text="done",
        )
        event("session.execution.succeeded")
        self.reply(
            {
                "data": {
                    "id": "user-1",
                    "sessionID": "session-1",
                    "time": {"created": 1},
                    "type": "user",
                    "payload": {"text": "Ask"},
                    "delivery": "queue",
                }
            }
        )


port = int(sys.argv[sys.argv.index("--port") + 1])
server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
print(f"opencode server listening on http://127.0.0.1:{port}", flush=True)
server.serve_forever()
