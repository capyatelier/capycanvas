import gzip
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

import app_names


class StoreMetadataTests(unittest.TestCase):
    def test_android_names_escape_xml_characters(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with patch.object(app_names, 'app_names', return_value={'en': 'Capy & <Canvas>'}):
                app_names.write_names('android', output)
            resource = ET.parse(output / 'values/app_name.xml')
            self.assertEqual(resource.findtext('string'), 'Capy & <Canvas>')

    def test_tracked_linux_metadata_matches_fluent_and_regenerates_identically(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            paths = ['art.capycanvas.CapyCanvas.desktop', 'art.capycanvas.CapyCanvas.metainfo.xml']
            original = {name: (app_names.ROOT / 'apps/layer-linux' / name).read_bytes() for name in paths}
            for name, contents in original.items():
                (output / name).write_bytes(contents)
            for _ in range(2):
                app_names.write_names('linux', output)
                self.assertEqual({name: (output / name).read_bytes() for name in paths}, original)
            component = ET.parse(output / paths[1]).getroot()
            for parent, tag in [(component, 'name'), (component, 'summary'), (component.find('description'), 'p')]:
                self.assertEqual(len({element.get(app_names.XML_LANG, 'en').replace('_', '-') for element in parent.findall(tag)}), 15)
                self.assertEqual(sum(app_names.XML_LANG not in element.attrib for element in parent.findall(tag)), 1)
                translations = {element.get(app_names.XML_LANG): element.text for element in parent.findall(tag)}
                for posix, bcp47 in [('zh_CN', 'zh-CN'), ('zh_TW', 'zh-TW'), ('pt_BR', 'pt-BR')]:
                    self.assertEqual(translations[posix], translations[bcp47])
            for screenshot in component.findall('screenshots/screenshot'):
                self.assertEqual(len({element.get(app_names.XML_LANG, 'en').replace('_', '-') for element in screenshot.findall('caption')}), 15)

    def test_language_enrichment_preserves_catalog_and_replaces_stale_languages(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'catalog.xml.gz'
            source = b'''<components origin="capycanvas" version="1.0">
              <component type="desktop-application"><id>art.capycanvas.CapyCanvas</id>
                <summary xml:lang="ja">\xe7\xb5\xb5\xe3\x82\x92\xe6\x8f\x8f\xe3\x81\x8f</summary>
                <supports><internet>offline-only</internet><control>tablet</control></supports>
                <requires><display_length>280</display_length></requires>
                <languages><lang percentage="20">obsolete</lang></languages>
              </component>
              <component><id>another.app</id><languages><lang>fr</lang></languages></component>
            </components>'''
            path.write_bytes(gzip.compress(source))
            app_names.write_catalog_languages(path)
            first = path.read_bytes()
            catalog = ET.fromstring(gzip.decompress(first))
            component = catalog.find("component[id='art.capycanvas.CapyCanvas']")
            self.assertEqual(catalog.attrib, {'origin': 'capycanvas', 'version': '1.0'})
            self.assertEqual(component.findtext('summary'), '絵を描く')
            self.assertEqual(component.findtext('supports/internet'), 'offline-only')
            self.assertEqual(component.findtext('supports/control'), 'tablet')
            self.assertEqual(component.findtext('requires/display_length'), '280')
            languages = {element.text: element.attrib for element in component.findall('languages/lang')}
            self.assertEqual(len(languages), 15)
            for locale in ['en', 'ja', 'zh_CN', 'zh_TW', 'pt_BR', 'th']:
                self.assertEqual(languages[locale], {'percentage': '100'})
            self.assertNotIn('obsolete', languages)
            self.assertEqual(catalog.findtext("component[id='another.app']/languages/lang"), 'fr')
            app_names.write_catalog_languages(path)
            self.assertEqual(path.read_bytes(), first)

    def test_catalog_without_the_application_is_left_untouched(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'catalog.xml.gz'
            original = gzip.compress(b'<components><component><id>another.app</id></component></components>')
            path.write_bytes(original)
            with self.assertRaisesRegex(ValueError, 'must contain art.capycanvas.CapyCanvas'):
                app_names.write_catalog_languages(path)
            self.assertEqual(path.read_bytes(), original)


if __name__ == '__main__':
    unittest.main()
