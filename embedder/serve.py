"""Private loopback OpenVINO endpoint. Never downloads models or logs input text."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import socket
import threading

from model import Encoder, REVISION


class Server(ThreadingHTTPServer):
    daemon_threads = True
    request_queue_size = 4

    def __init__(self, address, encoder):
        self.encoder = encoder
        self.slots = threading.BoundedSemaphore(2)
        super().__init__(address, Handler)

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            request.close()
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.slots.release()

    def handle_error(self, request, client_address):
        # Neither exception strings nor request paths/body belong in operational logs.
        pass


class Handler(BaseHTTPRequestHandler):
    def setup(self):
        self.request.settimeout(3)
        super().setup()

    def log_message(self, *args):
        pass

    def reply(self, status, value):
        body = json.dumps(value, separators=(",", ":"), allow_nan=False).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    def do_GET(self):
        if self.path != "/healthz":
            self.reply(404, {"error": "not_found"})
        else:
            self.reply(200, {"model_revision": REVISION})

    def do_POST(self):
        if self.path != "/embed":
            self.reply(404, {"error": "not_found"})
            return
        try:
            sizes = self.headers.get_all("Content-Length", [])
            if (len(sizes) != 1 or self.headers.get("Transfer-Encoding") is not None
                    or self.headers.get("Content-Type") != "application/json"):
                raise ValueError("invalid request framing")
            size = int(sizes[0])
            if not 0 < size <= 80000:
                raise ValueError("body limit")
            data = self.rfile.read(size)
            if len(data) != size:
                raise ValueError("incomplete body")
            value = json.loads(data)
            if not isinstance(value, dict) or set(value) != {"texts"}:
                raise ValueError("invalid fields")
            vectors = self.server.encoder.encode(value["texts"])
            self.reply(200, {"model_revision": REVISION, "embeddings": vectors})
        except (ValueError, UnicodeError):
            self.reply(400, {"error": "invalid_request"})
        except (TimeoutError, socket.timeout):
            self.close_connection = True
        except Exception:
            self.reply(503, {"error": "inference_unavailable"})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("directory")
    parser.add_argument("--port", type=int, default=8788)
    parser.add_argument("--threads", type=int, choices=range(1, 5), default=2)
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error("invalid port")
    encoder = Encoder(args.directory, args.threads)
    # Warm the actual model before accepting requests.
    encoder.encode(["Ready for local memory retrieval."])
    with Server(("127.0.0.1", args.port), encoder) as server:
        server.serve_forever()


if __name__ == "__main__":
    main()
