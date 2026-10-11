#!/usr/bin/env python3
"""Reproducible synthetic checks; never uses a live clipboard or cloud account.

Native Windows interaction and two-device iCloud acceptance are intentionally
not inferred from these commands. Run using Python 3.11 or newer.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def source_identity():
    files = subprocess.check_output(
        ['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=ROOT
    ).split(b'\0')
    inventory = {}
    for raw in sorted(set(files)):
        if not raw:
            continue
        name = os.fsdecode(raw)
        path = ROOT / name
        if path.is_file() and not path.is_symlink():
            inventory[name] = hashlib.sha256(path.read_bytes()).hexdigest()
    return {
        'base_revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'tree_sha256': hashlib.sha256(json.dumps(inventory, sort_keys=True).encode()).hexdigest(),
        'files': inventory,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / '.artifacts/verification')
    parser.add_argument('--skip-native', action='store_true', help='Core + portable model only; not native acceptance')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env['PASTELEAF_DISABLE_SYNC_WORKER'] = '1'
    env['PASTELEAF_DISABLE_KEYCHAIN'] = '1'
    env['MACOSX_DEPLOYMENT_TARGET'] = '14.0'
    py = sys.executable
    commands = [
        ('tokens', [py, 'scripts/generate_design_tokens.py', '--check']),
        ('design-tests', [py, '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_design_tokens.py', '-v']),
        ('package-tests', [py, '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_package*.py', '-v']),
        ('rust-format', ['cargo', 'fmt', '--all', '--check']),
        ('core-clippy', ['cargo', 'clippy', '-p', 'pasteleaf-core', '--locked', '--all-targets', '--', '-D', 'warnings']),
        ('core-tests', ['cargo', 'test', '-p', 'pasteleaf-core', '--locked', '--release', '--all-targets', '--', '--test-threads=1']),
        ('windows-portable-clippy', ['cargo', 'clippy', '-p', 'pasteleaf-windows', '--locked', '--all-targets', '--', '-D', 'warnings']),
        ('windows-portable-tests', ['cargo', 'test', '-p', 'pasteleaf-windows', '--locked', '--release']),
        ('core-library', ['cargo', 'build', '-p', 'pasteleaf-core', '--locked', '--release']),
    ]
    if sys.platform == 'win32':
        env['PASTELEAF_CORE_LIBRARY'] = str(ROOT / 'target/release/pasteleaf_core.dll')
    elif sys.platform == 'darwin':
        env['PASTELEAF_CORE_LIBRARY'] = str(ROOT / 'target/release/libpasteleaf_core.dylib')
    else:
        env['PASTELEAF_CORE_LIBRARY'] = str(ROOT / 'target/release/libpasteleaf_core.so')
    commands.append(('ffi-tests', [py, 'scripts/test_v2_abi.py', '-v']))
    commands.append(('mac-signing-policy-tests', [py, '-m', 'unittest', 'discover', '-s', 'scripts', '-p', 'test_macos_signing.py', '-v']))
    if not args.skip_native:
        if sys.platform == 'darwin':
            # test.sh prefers the explicit ARM64 archive. Refresh it before the
            # ABI tests so an old target-specific archive cannot yield a false pass.
            commands += [('mac-build', ['bash', 'clients/macos/build.sh']),
                         ('mac-typecheck', ['xcrun', 'swiftc', '-swift-version', '5',
                            '-warnings-as-errors', '-typecheck', '-target', 'arm64-apple-macos14.0',
                            *[str(p.relative_to(ROOT)) for p in sorted((ROOT / 'clients/macos/Sources').glob('*.swift'))]]),
                         ('mac-tests', ['bash', 'clients/macos/test.sh']),
                         ('mac-sync-tests', ['bash', 'clients/macos/test_sync_connection.sh'])]
        elif sys.platform == 'win32':
            commands += [('windows-clippy', ['cargo', 'clippy', '-p', 'pasteleaf-windows', '--locked', '--all-targets', '--', '-D', 'warnings']),
                         ('windows-build', ['cargo', 'build', '-p', 'pasteleaf-windows', '--locked', '--release'])]
    report = {'date_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'host': {'system': platform.system(), 'release': platform.release(), 'machine': platform.machine()},
              'scope': 'synthetic local tests only', 'checks': [],
              'windows_visual_uia': 'HOLD: requires actual Windows runtime',
              'real_two_device_icloud': 'HOLD: not exercised by this script',
              'source_start': source_identity()}
    for name, command in commands:
        print(f'[{name}] {" ".join(command)}', flush=True)
        start = time.monotonic()
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True)
        log = args.output / f'{name}.log'
        log.write_text(result.stdout, encoding='utf-8')
        report['checks'].append({'name': name, 'command': command, 'exit_code': result.returncode,
                                 'seconds': round(time.monotonic() - start, 3), 'log': log.name})
        print(f'{name}: {"PASS" if result.returncode == 0 else "FAIL"}', flush=True)
        if result.returncode:
            print(result.stdout[-6000:], flush=True)
            break
    report['source_end'] = source_identity()
    report['source_changed_during_run'] = report['source_start']['tree_sha256'] != report['source_end']['tree_sha256']
    report['all_commands_passed'] = len(report['checks']) == len(commands) and all(c['exit_code'] == 0 for c in report['checks'])
    report['verified_unchanged_source'] = report['all_commands_passed'] and not report['source_changed_during_run']
    (args.output / 'results.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(args.output / 'results.json')
    if report['source_changed_during_run']:
        print('Source changed during verification: rerun before treating this as a version-bound pass.')
    return 0 if report['verified_unchanged_source'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
