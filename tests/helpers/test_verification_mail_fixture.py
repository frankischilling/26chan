import importlib.util
import os
from pathlib import Path
import smtplib
import stat
import tempfile
import threading
import unittest

spec = importlib.util.spec_from_file_location('verification_fixture', Path(__file__).resolve().parents[2] / 'scripts/dev-verification-mail.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class MailFixtureTests(unittest.TestCase):
    def test_private_bounded_local_delivery(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            server = module.Fixture(('127.0.0.1', 0), output)
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            try:
                with smtplib.SMTP('127.0.0.1', server.server_address[1], timeout=2) as smtp:
                    smtp.ehlo()
                    smtp.sendmail('noreply@localhost', ['synthetic@example.test'], 'Subject: owned fixture\r\n\r\nSynthetic verification link.\r\n')
                paths = list(output.iterdir())
                self.assertEqual(len(paths), 1)
                self.assertEqual(stat.S_IMODE(paths[0].stat().st_mode), 0o600)
                self.assertIn('Synthetic verification link.', paths[0].read_text())
                old_limit = module.MAX_MESSAGES
                module.MAX_MESSAGES = 1
                try:
                    self.assertFalse(server.save(b'Not written'))
                    name = paths[0].name
                    server.owned[name] -= 3601
                    self.assertTrue(server.save(b'New owned fixture'))
                    self.assertFalse(paths[0].exists())
                finally:
                    module.MAX_MESSAGES = old_limit
            finally:
                server.shutdown()
                server.server_close()
                worker.join(timeout=3)

if __name__ == '__main__':
    unittest.main()
