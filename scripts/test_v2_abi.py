#!/usr/bin/env python3
"""Independent FFI smoke/integration checks. Only ephemeral synthetic local folders.
Set PASTELEAF_CORE_LIBRARY to the built dylib/DLL; no live clipboard or iCloud.
"""
import base64
import ctypes
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
LIB_PATH=Path(os.environ.get('PASTELEAF_CORE_LIBRARY',ROOT/'target/release/libpasteleaf_core.dylib'))
os.environ['PASTELEAF_DISABLE_SYNC_WORKER']='1'
os.environ['PASTELEAF_DISABLE_KEYCHAIN']='1'

class Core:
    def __init__(self,path):
        self.lib=ctypes.CDLL(str(LIB_PATH))
        self.lib.pl_api_version.restype=ctypes.c_uint32
        self.lib.pl_open.argtypes=[ctypes.c_char_p];self.lib.pl_open.restype=ctypes.c_void_p
        self.lib.pl_request.argtypes=[ctypes.c_void_p,ctypes.c_char_p];self.lib.pl_request.restype=ctypes.c_void_p
        self.lib.pl_free.argtypes=[ctypes.c_void_p];self.lib.pl_close.argtypes=[ctypes.c_void_p]
        if self.lib.pl_api_version()!=2:raise AssertionError('Incorrect ABI')
        self.handle=self.lib.pl_open(os.fsencode(path))
        if not self.handle:raise AssertionError('Core open failed')
    def raw(self,payload):
        ptr=self.lib.pl_request(self.handle,payload)
        if not ptr:raise AssertionError('Core returned null')
        try:return json.loads(ctypes.string_at(ptr))
        finally:self.lib.pl_free(ptr)
    def request(self,op,**kw):
        result=self.raw(json.dumps(dict(op=op,**kw),ensure_ascii=False).encode())
        if not result.get('ok'):raise AssertionError(result)
        return result['data']
    def close(self):
        if self.handle:self.lib.pl_close(self.handle);self.handle=None

@unittest.skipUnless(LIB_PATH.is_file(),'Build native core first; this is NOT a pass for the ABI')
class ABITests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(prefix='pasteleaf-abi-')
        self.root=Path(self.tmp.name);self.handles=[]
        self.a=self.make('device-a')
    def make(self,name):
        c=Core(self.root/name);self.handles.append(c)
        c.request('settings',values={'paused':False,'onboarding_complete':True})
        return c
    def tearDown(self):
        for c in self.handles:c.close()
        self.tmp.cleanup()
    def items(self,c):return c.request('list',query='',category=None,offset=0,limit=500)['items']
    def test_errors_are_envelopes_and_utf8_survives(self):
        self.assertFalse(self.a.raw(b'{')['ok'])
        self.assertFalse(self.a.raw(b'{"op":"not_real"}')['ok'])
        self.a.request('capture',kind='text',text='中文状态 🍃\ncopy → paste',source='Synthetic')
        item=self.items(self.a)[0]
        self.assertEqual(item['text'],'中文状态 🍃\ncopy → paste')
        self.assertEqual(self.a.request('get',id=item['id'])['text'],item['text'])
        self.assertTrue(self.a.request('list',query='状态',offset=0,limit=20)['items'])
    def test_rich_and_files_survive_restart_without_source_paths(self):
        rtf=base64.b64encode(b'{\\rtf1\\ansi caf\xe9}').decode()
        self.a.request('capture',kind='rich_text',text='café',html='<b>café</b>',rtf=rtf,source='Synthetic')
        original=self.root/'source-secret-name';original.mkdir()
        f=original/'example.txt';f.write_bytes(b'fictional-content\x00\xff')
        self.a.request('capture',kind='files',text='',paths=[str(f)],source='Synthetic')
        f.unlink()
        self.a.close();self.a=self.make('device-a')
        items=self.items(self.a)
        rich=next(x for x in items if x['kind']=='rich_text')
        self.assertEqual(rich['rtf'],rtf)
        attachment=next(x for x in items if x['kind']=='files')['files'][0]
        self.assertEqual(Path(attachment['path']).read_bytes(),b'fictional-content\x00\xff')
        self.assertNotIn('source-secret-name',json.dumps(items))
    def test_encrypted_two_device_roundtrip_and_wrong_password(self):
        cloud=self.root/'synthetic-sync';cloud.mkdir()
        password='synthetic-12-character-key'  # gitleaks:allow -- isolated fixture, not a real credential
        self.a.request('capture',kind='text',text='synthetic confidential payload',source='Synthetic')
        self.a.request('sync_create',directory=str(cloud),password=password,remember=False)
        self.a.request('sync_now')
        b=self.make('device-b')
        bad=b.raw(json.dumps({'op':'sync_join','directory':str(cloud),'password':'wrong-password-long','remember':False}).encode())
        self.assertFalse(bad['ok'])
        b.request('sync_join',directory=str(cloud),password=password,remember=False)
        b.request('sync_now');self.a.request('sync_now')
        self.assertEqual(self.items(b)[0]['text'],'synthetic confidential payload')
        for f in cloud.rglob('*'):
            if f.is_file():
                raw=f.read_bytes()
                self.assertNotIn(b'synthetic confidential payload',raw)
                self.assertNotIn(password.encode(),raw)
        b.request('sync_now')
        self.assertEqual(len(self.items(b)),1)
    def test_v1_directory_not_overwritten(self):
        cloud=self.root/'legacy-sync';cloud.mkdir()
        old=cloud/'pasteleaf-v1.json';old.write_text('{"protocol":1}')
        try:self.a.request('sync_create',directory=str(cloud),password='synthetic-long-password',remember=False)
        except AssertionError:pass  # rejection or a separate v2 child are both safe
        self.assertEqual(old.read_text(),'{"protocol":1}')
    def test_all_supported_content_in_both_directions_with_byte_checks(self):
        cloud=self.root/'synthetic-pair'
        self.a.request('sync_create',directory=str(cloud),password='synthetic-roundtrip-only',remember=False)
        self.a.request('sync_now')
        b=self.make('device-b')
        b.request('sync_join',directory=str(cloud),password='synthetic-roundtrip-only',remember=False)
        for label,sender,receiver in [('A',self.a,b),('B',b,self.a)]:
            records=[]
            for text in ['中文和 emoji 🍃','https://example.invalid/pasteleaf','const text = "synthetic";\nreturn text;']:
                records.append(sender.request('capture',kind='text',text=label+': '+text,source='Synthetic'))
            records.append(sender.request('capture',kind='rich_text',text=label+' rich',html='<b>'+label+'</b>',
                rtf=base64.b64encode(b'{\\rtf1\\ansi '+label.encode()+b" \\'e9}").decode(),source='Synthetic'))
            records.append(sender.request('capture',kind='image',image_path=str(ROOT/'clients/macos/Resources/demo-landscape.png'),source='Synthetic'))
            paths=[]
            for name in ['合成.txt','fixture.bin']:
                f=self.root/(label+'-'+name);f.write_bytes(label.encode()+b'\x00\xff-synthetic');paths.append(str(f))
            records.append(sender.request('capture',kind='files',paths=paths,source='Synthetic'))
            sender.request('sync_now');receiver.request('sync_now');sender.request('sync_now')
            for record in records:
                received=receiver.request('get',id=record['id'])
                for field in ['id','kind','text','html','rtf','source']:
                    self.assertEqual(received.get(field),record.get(field),field)
                if record['kind']=='image':
                    self.assertEqual(Path(received['image_path']).read_bytes(),Path(record['image_path']).read_bytes())
                for source,target in zip(record.get('files',[]),received.get('files',[])):
                    self.assertEqual(source['hash'],target['hash'])
                    self.assertEqual(source['size'],target['size'])
                    self.assertEqual(Path(source['path']).read_bytes(),Path(target['path']).read_bytes())
                    self.assertNotEqual(source['path'],target['path'])
    def test_committed_outbox_survives_abrupt_process_exit(self):
        root=self.root/'crash-device'
        code='''import os,sys
from test_v2_abi import Core
c=Core(sys.argv[1])
c.request('settings',values={'paused':False,'onboarding_complete':True})
c.request('capture',kind='text',text='synthetic abrupt-exit fixture',source='Synthetic')
os._exit(37)
'''
        env=os.environ.copy();env['PYTHONPATH']=str(ROOT/'scripts')
        result=subprocess.run([sys.executable,'-c',code,str(root)],env=env,capture_output=True)
        self.assertEqual(result.returncode,37)
        recovered=self.make('crash-device')
        self.assertEqual(self.items(recovered)[0]['text'],'synthetic abrupt-exit fixture')
        self.assertGreater(recovered.request('status')['pending'],0)

if __name__=='__main__':unittest.main()
