# Test server for the app tests: /sse streams four events 0.3 s apart, anything else answers JSON.
import http.server, socketserver, time, json
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def do_GET(self):
        if self.path == "/sse":
            self.send_response(200); self.send_header("Content-Type", "text/event-stream"); self.send_header("Transfer-Encoding", "chunked"); self.end_headers()
            def send(s):
                b = s.encode(); self.wfile.write(b"%x\r\n%s\r\n" % (len(b), b)); self.wfile.flush()
            for i, w in enumerate(["你", "好", "世界"]):
                send("event: delta\ndata: %s\n\n" % json.dumps({"i": i, "t": w}, ensure_ascii=False)); time.sleep(0.3)
            send("data: [DONE]\n\n"); self.wfile.write(b"0\r\n\r\n")
        else:
            body = json.dumps({"user": {"id": 7, "roles": ["admin", "dev"]}, "ok": True}).encode()
            self.send_response(200); self.send_header("Content-Type", "application/json"); self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)
    def log_message(self, *a): pass
class S(socketserver.ThreadingMixIn, http.server.HTTPServer): daemon_threads = True
s = S(("127.0.0.1", 0), H); print(s.server_port, flush=True); s.serve_forever()
