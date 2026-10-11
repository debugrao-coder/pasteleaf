import json
from pathlib import Path
import unittest
import tempfile
from unittest.mock import patch
import package_v2 as p
from test_package import payload

class V2PackageTests(unittest.TestCase):
    def test_v2_docs_allowed_without_weakening_v1_or_private_data_gate(self):
        original_required=p.legacy.REQUIRED.copy()
        with tempfile.TemporaryDirectory() as tmp:
            files=payload()
            files.update({name:b'Synthetic documentation' for name in p.V2_DOCUMENTS})
            result=p.write_windows_archive(files,Path(tmp)/'v2.zip',p.legacy.TARGET)
            self.assertGreater(result['files'],10)
            self.assertEqual(p.legacy.REQUIRED,original_required)
            files['library.sqlite']=b'not allowed'
            with self.assertRaisesRegex(ValueError,'unexpected application data'):
                p.write_windows_archive(files,Path(tmp)/'bad.zip',p.legacy.TARGET)
            self.assertEqual(p.legacy.REQUIRED,original_required)
    def test_local_core_is_traversed_but_not_mislabeled_registry(self):
        m={'packages':[
          {'id':'app','name':'pasteleaf-windows','version':'0.2.0','source':None,'license':'MIT','manifest_path':str(p.ROOT/'clients/windows/Cargo.toml')},
          {'id':'core','name':'pasteleaf-core','version':'0.2.0','source':None,'license':'MIT','manifest_path':str(p.ROOT/'crates/pasteleaf-core/Cargo.toml')},
          {'id':'dep','name':'serde','version':'1','source':'registry+test','license':'MIT','manifest_path':'/cache/serde/Cargo.toml'}],
          'resolve':{'root':None,'nodes':[{'id':'app','dependencies':['core']},{'id':'core','dependencies':['dep']},{'id':'dep','dependencies':[]}]}}
        with patch.object(p.legacy,'run',return_value=json.dumps(m)),patch.object(p.legacy,'executable',return_value='cargo'):
            result,_=p.metadata_for('pasteleaf-windows','x86_64-pc-windows-msvc')
        self.assertEqual([x['name'] for x in p.legacy.windows_packages(result)],['serde'])
    def test_external_path_dependency_blocks_packaging(self):
        m={'packages':[{'id':'app','name':'pasteleaf-core','source':None,'license':'MIT','manifest_path':'/outside/Cargo.toml'}],'resolve':{}}
        with patch.object(p.legacy,'run',return_value=json.dumps(m)),patch.object(p.legacy,'executable',return_value='cargo'):
            with self.assertRaises(ValueError):p.metadata_for('pasteleaf-core','aarch64-apple-darwin')
    def test_compiler_notices_are_complete_and_verified(self):
        result={}
        p.compiler_notices(p.ROOT/'license_assets/mingw-runtime',result)
        self.assertEqual(len(result),5)
    def test_incomplete_compiler_license_inventory_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);(root/'SOURCES.json').write_text('{}')
            with self.assertRaises(ValueError):p.compiler_notices(root,{})
if __name__=='__main__':unittest.main()
