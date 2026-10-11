#!/usr/bin/env python3
"""Use an explicitly provisioned local signing identity; never create or trust one.

The public fingerprint config is outside Git. The private key stays in Keychain.
Configured identities fail closed; they never silently fall back to ad-hoc.
"""
import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess

PRODUCTION_ID = 'io.pasteleaf.desktop.v2'
ALLOWED_IDS = {PRODUCTION_ID, PRODUCTION_ID + '.demo', PRODUCTION_ID + '.test'}


def signing_plan(bundle_id, config):
    if bundle_id not in ALLOWED_IDS:
        raise ValueError('Not a PasteLeaf bundle')
    if config is None or bundle_id != PRODUCTION_ID:
        return 'ad-hoc', ['--sign', '-']
    fingerprint = config.get('identity_sha1', '')
    keychain = config.get('keychain', '')
    if not isinstance(fingerprint, str) or not re.fullmatch(r'[0-9a-fA-F]{40}', fingerprint):
        raise ValueError('Expected the exact signing-certificate SHA-1 fingerprint')
    if not isinstance(keychain, str) or not Path(keychain).is_absolute():
        raise ValueError('Expected an absolute signing keychain path')
    requirement = '=designated => identifier "' + bundle_id + '" and certificate leaf = H"' + fingerprint + '"'
    return 'local-certificate', ['--sign', fingerprint, '--keychain', keychain,
                                 '--timestamp=none', '--requirements', requirement]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('app', type=Path)
    args = parser.parse_args()
    info_path = args.app / 'Contents/Info.plist'
    info = plistlib.loads(info_path.read_bytes())
    config = None
    config_path = Path(os.environ.get('PASTELEAF_SIGNING_CONFIG',
        str(Path.home() / 'Library/Application Support/PasteLeaf Signing/identity.json')))
    if info.get('CFBundleIdentifier') == PRODUCTION_ID and config_path.exists():
        stat = config_path.stat()
        if config_path.is_symlink() or stat.st_uid != os.getuid() or stat.st_mode & 0o022 or stat.st_size > 65536:
            raise ValueError('Signing config must be owned by the current user and not writable by others')
        config = json.loads(config_path.read_text())
    elif 'PASTELEAF_SIGNING_CONFIG' in os.environ and info.get('CFBundleIdentifier') == PRODUCTION_ID:
        raise ValueError('Explicit signing config is missing; refusing ad-hoc fallback')
    mode, flags = signing_plan(info.get('CFBundleIdentifier'), config)
    info['PLSigningMode'] = mode
    info_path.write_bytes(plistlib.dumps(info))
    subprocess.run(['codesign', '--force', '--deep', *flags, str(args.app)], check=True)
    subprocess.run(['codesign', '--verify', '--deep', '--strict', str(args.app)], check=True)
    print('Signed PasteLeaf bundle: ' + mode + ' (not Developer ID / not notarized)')


if __name__ == '__main__':
    main()
