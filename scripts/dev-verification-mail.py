#!/usr/bin/env python3
"""Loopback-only, synthetic-account SMTP fixture. Never use for real email."""
import argparse
import os
from pathlib import Path
import socketserver
import threading
import time
import uuid

MAX_MESSAGE = 8192
MAX_MESSAGES = 100

class Fixture(socketserver.ThreadingTCPServer):
    allow_reuse_address = False
    daemon_threads = True
    def __init__(self, address, output):
        self.output = output
        self.lock = threading.Lock()
        self.slots = threading.BoundedSemaphore(4)
        self.owned = {}
        super().__init__(address, Handler)
    def process_request(self, request, address):
        if not self.slots.acquire(blocking=False):
            request.close()
            return
        try:
            super().process_request(request, address)
        except BaseException:
            self.slots.release()
            raise
    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            self.slots.release()
    def handle_error(self, request, address):
        # Never print message content, recipients, or tokens on protocol errors.
        pass
    def save(self, message):
        with self.lock:
            now = time.monotonic()
            for name, created in list(self.owned.items()):
                if now - created >= 3600:
                    (self.output / name).unlink(missing_ok=True)
                    del self.owned[name]
            if len(self.owned) >= MAX_MESSAGES:
                return False
            name = f'{uuid.uuid4()}.eml'
            fd = os.open(self.output / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, 'wb') as file:
                file.write(message)
                file.flush()
                os.fsync(file.fileno())
            self.owned[name] = now
            return True

class Handler(socketserver.StreamRequestHandler):
    def handle(self):
        self.connection.settimeout(10)
        self.wfile.write(b'220 owned verification fixture\r\n')
        state = 0
        for _ in range(8):
            line = self.rfile.readline(1025)
            if len(line) > 1024 or not line.endswith(b'\r\n'):
                return
            if state == 0 and line.upper().startswith(b'EHLO '):
                state = 1
                self.wfile.write(b'250 fixture\r\n')
            elif state == 1 and line.upper().startswith(b'MAIL FROM:<'):
                state = 2
                self.wfile.write(b'250 sender accepted\r\n')
            elif state == 2 and line.upper().startswith(b'RCPT TO:<'):
                state = 3
                self.wfile.write(b'250 recipient accepted\r\n')
            elif state == 3 and line.upper() == b'DATA\r\n':
                self.wfile.write(b'354 message follows\r\n')
                data = bytearray()
                while True:
                    piece = self.rfile.readline(1025)
                    if len(piece) > 1024 or not piece.endswith(b'\r\n'):
                        return
                    if piece == b'.\r\n':
                        break
                    data.extend(piece[1:] if piece.startswith(b'..') else piece)
                    if len(data) > MAX_MESSAGE:
                        return
                accepted = self.server.save(data)
                self.wfile.write(b'250 stored locally\r\n' if accepted else b'452 fixture full\r\n')
                state = 4
            elif line.upper() == b'QUIT\r\n':
                self.wfile.write(b'221 closing\r\n')
                return
            else:
                self.wfile.write(b'503 invalid sequence\r\n')
                return

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=1025)
    parser.add_argument('--output', type=Path, required=True, help='New private directory for synthetic messages')
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error('Port must be nonzero.')
    if os.name != 'posix':
        parser.error('Use a Unix-like host so fixture directory permissions can be enforced.')
    args.output.mkdir(mode=0o700, parents=False, exist_ok=False)
    os.chmod(args.output, 0o700)
    with Fixture(('127.0.0.1', args.port), args.output) as server:
        print('Synthetic mail fixture listening on loopback. Messages expire after one hour during subsequent deliveries.', flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass

if __name__ == '__main__':
    main()
