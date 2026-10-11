#!/usr/bin/env python3
"""Package v2 native artifacts with offline, Cargo.lock-verified dependency notices.

MSVC is the distribution baseline. GNU cross-builds are labelled engineering
previews and require their compiler runtime license directory explicitly.
"""
import argparse
import json
import sys
import zipfile
from pathlib import Path
import package as legacy
from verify_v2 import source_identity

ROOT=legacy.ROOT
V2_DOCUMENTS={'SOURCE.json','DESIGN.md','docs/V2-DEVELOPMENT.md','docs/V2-VERIFICATION.md'}

def write_windows_archive(payload, destination, target):
    """Keep v1's strict manifest checks, with an explicit v2 document allowlist."""
    required, previous_target = legacy.REQUIRED, legacy.TARGET
    try:
        legacy.REQUIRED = required | V2_DOCUMENTS
        legacy.TARGET = target
        return legacy.write_archive(payload, destination)
    finally:
        legacy.REQUIRED, legacy.TARGET = required, previous_target

def metadata_for(name, target):
    meta=json.loads(legacy.run([legacy.executable('cargo'),'metadata','--offline','--locked',
        '--filter-platform',target,'--format-version','1']))
    matches=[p for p in meta['packages'] if p['name']==name and p['source'] is None]
    if len(matches)!=1: raise ValueError('Ambiguous workspace package')
    meta['resolve']['root']=matches[0]['id']
    # Workspace-owned MIT code is covered by the root LICENSE. Traverse its
    # dependencies, but do not pretend it is a registry archive.
    owned=[p for p in meta['packages'] if p['source'] is None]
    for p in owned:
        if not Path(p['manifest_path']).resolve().is_relative_to(ROOT):
            raise ValueError('Unreviewed external path dependency')
        if p.get('license')!='MIT': raise ValueError('Unreviewed workspace license')
    meta['packages']=[p for p in meta['packages'] if p['source'] is not None or p['id']==matches[0]['id']]
    return meta,matches[0]

def notices(name,target,payload):
    meta,p=metadata_for(name,target)
    inventory=legacy.dependency_notices(meta,payload)
    payload['third-party/DEPENDENCIES.json']=legacy.json_bytes(inventory)
    compiler=legacy.rust_notices(payload)
    return p,inventory,compiler

def compiler_notices(directory, payload):
    if not directory or not directory.is_dir():
        raise ValueError('GNU preview requires reviewed compiler runtime licenses')
    required = {'mingw-w64-COPYING', 'mingw-w64-AUTHORS',
                'GCC-Runtime-Library-Exception-3.1.txt', 'GPL-3.0.txt'}
    sources = json.loads((directory / 'SOURCES.json').read_text())
    if not required.issubset(sources):
        raise ValueError('Compiler runtime license inventory is incomplete')
    for name, entry in sources.items():
        if Path(name).name != name:
            raise ValueError('Invalid compiler license path')
        path = directory / name
        if path.is_symlink() or not path.is_file():
            raise ValueError('Compiler license is missing or is a link')
        data = path.read_bytes()
        if not entry.get('source') or legacy.digest(data) != entry.get('sha256'):
            raise ValueError('Compiler runtime notice checksum mismatch')
        payload['third-party/mingw-runtime/' + name] = data
    payload['third-party/mingw-runtime/SOURCES.json'] = legacy.json_bytes(sources)

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--platform', choices=['windows','macos'],required=True)
    p.add_argument('--target')
    p.add_argument('--artifact',type=Path,required=True)
    p.add_argument('--output',type=Path,default=ROOT/'dist')
    p.add_argument('--compiler-licenses',type=Path)
    a=p.parse_args()
    target=a.target or ('x86_64-pc-windows-msvc' if a.platform=='windows' else 'aarch64-apple-darwin')
    payload={n:(ROOT/n).read_bytes() for n in ['LICENSE','README.md','THIRD_PARTY_NOTICES.md']}
    for n in ['docs/V2-DEVELOPMENT.md', 'docs/V2-VERIFICATION.md', 'DESIGN.md']:
        if (ROOT/n).is_file(): payload[n]=(ROOT/n).read_bytes()
    source=source_identity()
    payload['SOURCE.json']=legacy.json_bytes(source)
    name='pasteleaf-windows' if a.platform=='windows' else 'pasteleaf-core'
    meta,inv,compiler=notices(name,target,payload)
    if target.endswith('-gnu'):
        compiler_notices(a.compiler_licenses, payload)
    payload['RELEASE.json']=legacy.json_bytes({'name':name,'version':meta['version'],'target':target,
        'rustc':compiler,'third_party_package_count':len(inv),
        'cargo_lock_sha256':legacy.digest((ROOT/'Cargo.lock').read_bytes()),
        'readiness':'engineering-preview; real Mac-Windows iCloud verification HOLD',
        'source_revision':legacy.run(['git','rev-parse','HEAD']).strip(),
        'source_tree_sha256':source['tree_sha256'],
        'source_dirty':bool(legacy.run(['git','status','--porcelain']).strip())})
    a.output.mkdir(parents=True,exist_ok=True)
    if a.platform=='windows':
        payload['pasteleaf.exe']=a.artifact.read_bytes()
        out=a.output/f'pasteleaf-{meta["version"]}-windows-x64-preview.zip'
        print(json.dumps(write_windows_archive(payload,out,target),indent=2))
    else:
        app=a.artifact.resolve()
        if app.suffix!='.app' or not (app/'Contents/MacOS/PasteLeaf').is_file():
            raise ValueError('Expected a complete PasteLeaf.app bundle')
        # Package adjacent notices: never mutate a previously signed app bundle.
        mode={}
        for f in app.rglob('*'):
            if f.is_symlink(): raise ValueError('Unexpected symlink in application bundle')
            if f.is_file():
                n=app.name+'/'+f.relative_to(app).as_posix();payload[n]=f.read_bytes();mode[n]=f.stat().st_mode
        payload['SHA256SUMS']=''.join(f'{legacy.digest(d)}  {n}\n' for n,d in sorted(payload.items())).encode()
        out=a.output/f'pasteleaf-{meta["version"]}-macos-arm64-preview.zip'
        with zipfile.ZipFile(out,'w',zipfile.ZIP_DEFLATED) as z:
            for n,d in sorted(payload.items()):
                info=zipfile.ZipInfo(n,(1980,1,1,0,0,0));info.create_system=3
                info.external_attr=mode.get(n,0o100644)<<16;info.compress_type=zipfile.ZIP_DEFLATED;z.writestr(info,d)
        with zipfile.ZipFile(out) as z:
            if z.testzip():raise ValueError('ZIP integrity failed')
            for line in z.read('SHA256SUMS').decode().splitlines():
                checksum,n=line.split('  ',1)
                if legacy.digest(z.read(n))!=checksum:raise ValueError('Payload checksum mismatch')
        checksum=legacy.digest(out.read_bytes())
        out.with_suffix('.zip.sha256').write_text(f'{checksum}  {out.name}\n')
        print(json.dumps({'archive':str(out),'sha256':checksum,'dependencies':len(inv)},indent=2))
if __name__=='__main__':
    try:main()
    except (ValueError,OSError,KeyError) as e:
        print('Packaging failed: '+str(e),file=sys.stderr);sys.exit(1)
