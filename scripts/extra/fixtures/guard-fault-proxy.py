"""Test-only transport fault before Guard; never used by a deployment template."""
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import threading

ARMED = Path('/fault/armed')
LOCK = threading.Lock()


class Proxy(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *args):
        pass  # Docker request bodies may contain fixture credentials.

    def forward(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
        if self.command == 'POST' and self.path.split('?')[0].endswith('/containers/create'):
            request = json.loads(body)
            service = request.get('Labels', {}).get('com.docker.compose.service')
            with LOCK:
                fail = service == 'backend-volume-init' and ARMED.exists()
                if fail:
                    ARMED.unlink()
                    Path('/fault/injected').write_text('backend-volume-init create rejected once\n')
            if fail:
                self.reply(500, b'{"message":"rehearsal: injected initializer create failure"}')
                return
        connection = http.client.HTTPConnection('docker-guard', 2375, timeout=600)
        headers = {key: value for key, value in self.headers.items()
                   if key.lower() not in ('host', 'connection', 'transfer-encoding')}
        try:
            connection.request(self.command, self.path, body, headers)
            response = connection.getresponse()
            self.reply(response.status, response.read(), response.getheaders())
        finally:
            connection.close()

    def reply(self, status, body, headers=()):
        self.send_response(status)
        for key, value in headers:
            if key.lower() not in ('connection', 'transfer-encoding', 'content-length'):
                # http.client accepts folded upstream headers; BaseHTTPRequestHandler
                # does not sanitize them when emitting a response.
                self.send_header(key.replace('\r', '').replace('\n', ''),
                                 value.replace('\r', '').replace('\n', ''))
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Connection', 'close')
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

    do_GET = do_POST = do_DELETE = do_HEAD = do_PUT = forward


if __name__ == '__main__':
    ThreadingHTTPServer(('0.0.0.0', 2375), Proxy).serve_forever()
