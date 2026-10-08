import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('paired_artifact', Path(__file__).resolve().parents[2] / 'scripts/paired-coordinator-artifact.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class CoordinatorArtifactTests(unittest.TestCase):
    def test_exact_test_artifact_and_negative_controls(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / 'target'
            target.mkdir()
            executable = target / 'paired_vm-a123'
            executable.write_bytes(b'owned test artifact; never executed')
            executable.chmod(0o700)
            record = {'reason': 'compiler-artifact', 'profile': {'test': True},
                      'target': {'name': 'paired_vm', 'kind': ['test'],
                                 'src_path': str(root / 'apps/media-admin/tests/paired_vm.rs')},
                      'executable': str(executable)}
            encode = lambda value: json.dumps(value).encode()
            self.assertEqual(module.select_artifact(encode(record), root, target), str(executable))
            for change in [lambda r: r.update(executable=None),
                           lambda r: r.update(executable=str(executable)+'\nother'),
                           lambda r: r['profile'].update(test=False),
                           lambda r: r['target'].update(src_path=str(root/'wrong.rs')),
                           lambda r: r['target'].update(kind=['bin']),
                           lambda r: r['target'].update(name='other')]:
                bad = json.loads(json.dumps(record))
                change(bad)
                with self.assertRaises(ValueError):
                    module.select_artifact(encode(bad), root, target)
            for bad in [b'', b'not-json', encode(record)+b'\n'+encode(record), b' '*(8*1024*1024+1)]:
                with self.assertRaises(ValueError):
                    module.select_artifact(bad, root, target)
            outside = root / 'paired_vm-beef'
            outside.write_bytes(b'outside'); outside.chmod(0o700)
            record['executable'] = str(outside)
            with self.assertRaises(ValueError):
                module.select_artifact(encode(record), root, target)
            link = target / 'paired_vm-beef'
            link.symlink_to(executable)
            record['executable'] = str(link)
            with self.assertRaises(ValueError):
                module.select_artifact(encode(record), root, target)
            record['executable'] = str(executable)
            executable.chmod(0o600)
            with self.assertRaises(ValueError):
                module.select_artifact(encode(record), root, target)


if __name__ == '__main__':
    unittest.main()
