#!/usr/bin/env python3
"""Create a NEW synthetic native-UI library, never a live clipboard/cloud library.

Example: python3 scripts/seed_native_fixture.py .artifacts/native-fixture
The destination must not exist. The generated sync folders are ordinary local
folders, not iCloud evidence. Test.app is the only bundle to use with this data.
"""
import argparse
import base64
import json
from pathlib import Path
from test_v2_abi import Core, ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    root = args.destination.resolve()
    root.mkdir(parents=True, exist_ok=False)
    (root / 'SYNTHETIC-ONLY.txt').write_text('Synthetic local fixture. Not real iCloud. No user clipboard.\n')
    handles = []
    try:
        a, b = Core(root / 'library'), Core(root / 'peer')
        handles += [a, b]
        for core in handles:
            core.request('settings', values={'paused': False, 'onboarding_complete': True})
        category = a.request('category_create', name='设计灵感', color='#347A58')['id']
        conflict = a.request('capture', kind='text', text='合成冲突：同一条设计记录的两个标签版本', source='Synthetic QA')
        a.request('sync_create', directory=str(root / 'local-sync'), password='synthetic-fixture-only', remember=False)
        a.request('sync_now')
        b.request('sync_join', directory=str(root / 'local-sync'), password='synthetic-fixture-only', remember=False)
        b.request('sync_now')
        a.request('tags', id=conflict['id'], tags=['Mac 设计'])
        b.request('tags', id=conflict['id'], tags=['Windows 设计'])
        for core in (a, b, a):
            core.request('sync_now')
        for core in handles:
            core.request('sync_disconnect')
        for n in range(625):
            a.request('capture', kind='text', text=f'设计评审 · 分页合成记录 {n:04d}\n永久历史与中文搜索测试，不是真实剪贴板。', source='Synthetic QA')
        fixture_files = root / 'source-files'
        fixture_files.mkdir()
        paths = []
        for filename in ('跨端设计规范与页面验收清单-这是用于超长文件名换行验收的合成附件.md', 'Synthetic Notes.txt'):
            path = fixture_files / filename
            path.write_text('Only synthetic attachment bytes. 中文文件往返校验。\n')
            paths.append(str(path))
        a.request('capture', kind='files', paths=paths, source='合成文件')
        a.request('capture', kind='rich_text', text='把复杂留在背后，把顺手放在眼前。\n合成富文本，安全预览不执行脚本。', html='<h2>安全预览</h2><p>合成内容</p><img src="https://invalid.test/no-request">', rtf=base64.b64encode(b'{\\rtf1\\ansi Synthetic rich text}').decode(), source='合成文稿')
        a.request('capture', kind='text', text="const idea = {\n  title: 'Make room for good ideas',\n  saved: true\n};", source='合成编辑器')
        link = a.request('capture', kind='text', text='https://github.com/debugrao-coder/pasteleaf', source='合成浏览器')
        a.request('favorite', id=link['id'], category=category)
        a.request('capture', kind='image', image_path=str(ROOT / 'clients/macos/Resources/demo-landscape.png'), source='合成图库')
        note = a.request('capture', kind='text', text='让好想法，随手可得。\n\n原生界面与共享核心，使用完全隔离的合成内容验收。', source='合成备忘录')
        a.request('tags', id=note['id'], tags=['界面验收'])
        a.request('settings', values={'paused': True, 'appearance': 'light'})
        report = {'synthetic_only': True, 'library': str(root / 'library'),
                  'records': a.request('list', limit=1)['total'],
                  'conflicts': len(a.request('conflicts')['items'])}
        (root / 'fixture.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
        print(json.dumps(report, ensure_ascii=False, indent=2))
    finally:
        for core in handles:
            core.close()


if __name__ == '__main__':
    main()
