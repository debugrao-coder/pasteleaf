import json
import hashlib
import struct
import unittest
from pathlib import Path
import generate_design_tokens as g

class TokensTests(unittest.TestCase):
    def test_generated_adapters_match_source(self):
        for name, value in g.generated().items():
            self.assertEqual((g.ROOT/name).read_text(), value, name)
    def test_text_contrast(self):
        def lum(s):
            v=[int(s[i:i+2],16)/255 for i in (0,2,4)]
            v=[x/12.92 if x<=.04045 else ((x+.055)/1.055)**2.4 for x in v]
            return sum(a*b for a,b in zip(v,[.2126,.7152,.0722]))
        t=json.loads((g.ROOT/'design/tokens.json').read_text())
        for name in ('light','dark'):
            p=t[name]
            for fg in ('text','muted'):
                for bg in ('background','surface','elevated','selection','text_header','image_header','link_header','file_header','rich_header'):
                    a,b=sorted((lum(p[fg]),lum(p[bg])))
                    self.assertGreaterEqual((b+.05)/(a+.05),4.5,(name,fg,bg))
            a,b=sorted((lum(p['accent_text']),lum(p['accent'])))
            self.assertGreaterEqual((b+.05)/(a+.05),4.5)
    def test_geometry_and_palette_symmetry(self):
        t=json.loads((g.ROOT/'design/tokens.json').read_text())
        self.assertEqual(t['light'].keys(),t['dark'].keys())
        for key,n in t['geometry'].items():
            if key!='motion_seconds': self.assertEqual(n%4,0)
        for opacity in t['states'].values(): self.assertTrue(0 < opacity <= 1)
        self.assertEqual(len(set(t['category_colors'])), len(t['category_colors']))
    def test_mac_native_roles_are_generated_not_a_private_hex_palette(self):
        t=json.loads((g.ROOT/'design/tokens.json').read_text())
        mac=t['platforms']['macos']
        self.assertEqual(mac['accent'],'system')
        self.assertEqual(mac['navigation_material'],'liquidGlassWithAvailabilityFallback')
        self.assertEqual(mac['window_material'],'underWindowBackground')
        self.assertEqual(set(mac['colors']),{'background','surface','elevated','border',
                         'text','muted','selection','danger','warning'})
        self.assertEqual(mac['colors']['text'],'labelColor')
        self.assertEqual(mac['colors']['muted'],'secondaryLabelColor')
        swift=g.generated()['clients/macos/Sources/DesignTokens.generated.swift']
        for role,color in mac['colors'].items():
            self.assertIn(f'static var {g.camel(role)}: NSColor {{ .{color} }}',swift)
        # Shared card geometry and state meanings do not require pixel-identical
        # materials. Windows retains its generated light/dark fallback palette.
        rust=g.generated()['clients/windows/src/design_tokens.rs']
        self.assertNotIn('NSColor',rust)
        self.assertIn('pub const LIGHT: Palette',rust)

    def test_compact_shelf_content_fits_panel(self):
        geometry=json.loads((g.ROOT/'design/tokens.json').read_text())['geometry']
        rows=('panel_top_padding','header_height','header_gap','category_height',
              'category_gap','card_height','card_strip_extra','footer_height',
              'footer_gap','panel_bottom_padding')
        self.assertEqual(sum(geometry[name] for name in rows),geometry['panel_height'])
        self.assertLess(geometry['panel_height'],350)
        self.assertGreaterEqual(geometry['card_height'],180)
        self.assertEqual(geometry['panel_bottom_inset'],0)

    def test_shared_app_artwork_is_versioned_and_alpha_preserving(self):
        brand=g.ROOT/'assets/brand'
        manifest=json.loads((brand/'pasteleaf-app-icon-v2.json').read_text())
        self.assertTrue(manifest['generation']['prompt'])
        for name,checksum in manifest['sha256'].items():
            self.assertEqual(Path(name).name,name)
            self.assertEqual(hashlib.sha256((brand/name).read_bytes()).hexdigest(),checksum,name)
        for name,size in ((manifest['master'],None),(manifest['distribution']['png'],1024)):
            data=(brand/name).read_bytes()
            self.assertEqual(data[:8],b'\x89PNG\r\n\x1a\n')
            w,h,depth,color=struct.unpack('>IIBB',data[16:26])
            self.assertEqual(w,h)
            self.assertGreaterEqual(w,1024)
            if size: self.assertEqual(w,size)
            self.assertEqual((depth,color),(8,6),'RGBA PNG required')

    def test_native_icon_containers_and_build_references(self):
        brand=g.ROOT/'assets/brand'
        ico=(brand/'pasteleaf-app-icon-v2.ico').read_bytes()
        self.assertEqual(struct.unpack('<HHH',ico[:6]),(0,1,7))
        cursor=6+7*16
        for i,size in enumerate((16,24,32,48,64,128,256)):
            w,h,colors,reserved,planes,bits,length,offset=struct.unpack_from('<BBBBHHII',ico,6+i*16)
            self.assertEqual((w or 256,h or 256,colors,reserved,planes,bits),(size,size,0,0,1,32))
            self.assertEqual(offset,cursor)
            frame=ico[offset:offset+length]
            self.assertEqual(len(frame),length)
            self.assertEqual(frame[:8],b'\x89PNG\r\n\x1a\n')
            self.assertEqual(struct.unpack('>IIBB',frame[16:26]),(size,size,8,6))
            cursor+=length
        self.assertEqual(cursor,len(ico))
        icns=(brand/'pasteleaf-app-icon-v2.icns').read_bytes()
        self.assertEqual(icns[:4],b'icns')
        self.assertEqual(struct.unpack('>I',icns[4:8])[0],len(icns))
        cursor=8; types=[]
        while cursor<len(icns):
            kind,length=struct.unpack_from('>4sI',icns,cursor)
            self.assertGreaterEqual(length,8)
            types.append(kind);cursor+=length
        self.assertEqual(cursor,len(icns))
        self.assertIn(b'ic10',types,'1024-pixel Retina entry required')
        self.assertIn('assets/brand/pasteleaf-app-icon-v2-master.png',(g.ROOT/'clients/macos/build.sh').read_text())
        self.assertIn('assets/brand/pasteleaf-app-icon-v2.ico',(g.ROOT/'clients/windows/build.rs').read_text())
if __name__=='__main__': unittest.main()
