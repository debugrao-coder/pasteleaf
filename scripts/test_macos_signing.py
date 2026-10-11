import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('mac_signing', Path(__file__).resolve().parents[1] / 'clients/macos/sign_app.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class SigningPlanTests(unittest.TestCase):
    def test_fingerprint_and_bundle_are_both_required(self):
        mode, flags = module.signing_plan(module.PRODUCTION_ID, {'identity_sha1': 'a' * 40, 'keychain': str(Path.cwd() / 'synthetic/login.keychain-db')})
        self.assertEqual(mode, 'local-certificate')
        requirement = flags[flags.index('--requirements') + 1]
        self.assertIn('identifier "io.pasteleaf.desktop.v2" and certificate leaf', requirement)
        self.assertIn('H"' + 'a' * 40 + '"', requirement)
        self.assertNotIn('cdhash', requirement)

    def test_no_automatic_identity_creation_or_test_signing(self):
        self.assertEqual(module.signing_plan(module.PRODUCTION_ID, None), ('ad-hoc', ['--sign', '-']))
        self.assertEqual(module.signing_plan(module.PRODUCTION_ID + '.test', {'identity_sha1': 'invalid'}), ('ad-hoc', ['--sign', '-']))

    def test_invalid_config_never_silently_falls_back(self):
        for config in [{}, {'identity_sha1': 'name', 'keychain': '/tmp/k'},
                       {'identity_sha1': 'a' * 40, 'keychain': 'relative'},
                       {'identity_sha1': '" or true', 'keychain': '/tmp/k'}]:
            with self.assertRaises(ValueError):
                module.signing_plan(module.PRODUCTION_ID, config)

    def test_foreign_bundle_rejected(self):
        with self.assertRaises(ValueError):
            module.signing_plan('com.other.app', None)


if __name__ == '__main__':
    unittest.main()
