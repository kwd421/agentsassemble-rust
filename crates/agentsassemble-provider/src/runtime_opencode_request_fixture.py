#!/usr/bin/env python3
"""Local protocol fixture; never loads a provider or records authentication headers."""
import http.server
import json
import pathlib
import queue
import sys
import threading
import urllib.parse

events = queue.Queue()
answered = threading.Event()
model = {"providerID": "opencode", "modelID": "fixture"}
assistant = {"id": "assistant-1", "parentID": "user-1", "role": "assistant", **model}


def event(kind, **properties):
    events.put({"type": kind, "properties": {"sessionID": "session-1", **properties}})


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def reply(self, value):
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = urllib.parse.urlparse(self.path).path
        if path == "/global/health":
            return self.reply({"healthy": True})
        if path == "/session/session-1":
            return self.reply({"id": "session-1"})
        if path != "/event":
            return self.send_error(404)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        self.wfile.flush()
        while True:
            value = events.get(timeout=10)
            self.wfile.write(("data: " + json.dumps(value) + "\n\n").encode())
            self.wfile.flush()
            if value["type"] == "session.status":
                return

    def do_POST(self):
        path = urllib.parse.urlparse(self.path).path
        payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if path == "/mcp":
            return self.reply({"agentsassemble_room": {"status": "connected"}})
        if path == "/session":
            return self.reply({"id": "session-1"})
        if path == "/permission/permission-1/reply":
            pathlib.Path("native-reply.json").write_text(json.dumps(payload))
            self.reply(True)
            answered.set()
            return
        if path != "/session/session-1/message":
            return self.send_error(404)
        event("message.updated", info={"id": "user-1", "role": "user"})
        event("permission.asked", id="permission-1", permission="bash", patterns=["echo fixture"], always=[])
        if not answered.wait(10):
            return self.send_error(504)
        event("message.updated", info=assistant)
        event("session.status", status={"type": "idle"})
        self.reply({"info": assistant, "parts": [{"type": "text", "text": "done"}]})


port = int(sys.argv[sys.argv.index("--port") + 1])
server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
print(f"opencode server listening on http://127.0.0.1:{port}", flush=True)
server.serve_forever()
