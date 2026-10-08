import argparse
import json
import re
from pathlib import Path
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parents[2]


def app_names():
    registry = (ROOT / 'crates/layer-ui/src/localization_languages.rs').read_text(encoding='utf-8')
    tags = re.findall(r'\("[^"]+", "([^"]+)",', registry)
    names = {}
    for tag in tags:
        source = (ROOT / 'assets/locales' / tag / 'common.ftl').read_text(encoding='utf-8')
        match = re.search(r'^common-app-name = ([^{}\n]+)$', source, re.M)
        if not match:
            raise ValueError(f'{tag}: application name must be literal text')
        names[tag] = match[1]
    return names


def write_names(platform, output):
    names = app_names()
    if platform == 'linux':
        for filename, field in [('art.capycanvas.CapyCanvas.desktop', 'desktop'),
                ('art.capycanvas.CapyCanvas.metainfo.xml', 'appstream')]:
            path = output / filename
            source = path.read_text(encoding='utf-8')
            tags = {tag: {'zh-Hans': 'zh_CN', 'zh-Hant': 'zh_TW', 'pt-BR': 'pt_BR'}.get(tag, tag) for tag in names}
            if field == 'desktop':
                source = re.sub(r'^Name\[[^]]+\]=.*\n', '', source, flags=re.M)
                labels = ''.join(f'Name[{tags[tag]}]={name}\n' for tag, name in names.items() if tag != 'en')
                source = source.replace(f'Name={names["en"]}\n', f'Name={names["en"]}\n' + labels, 1)
            else:
                source = re.sub(r'^  <name xml:lang="[^"]+">.*</name>\n', '', source, flags=re.M)
                labels = ''.join(f'  <name xml:lang="{tags[tag]}">{escape(name)}</name>\n' for tag, name in names.items() if tag != 'en')
                source = source.replace(f'  <name>{names["en"]}</name>\n', f'  <name>{names["en"]}</name>\n' + labels, 1)
            path.write_text(source, encoding='utf-8')
        return
    for tag, name in names.items():
        if platform == 'android':
            directory = output / ('values' if tag == 'en' else 'values-b+' + tag.replace('-', '+'))
            content = f'<?xml version="1.0" encoding="utf-8"?>\n<resources><string name="capy_app_name">{escape(name)}</string></resources>\n'
            filename = 'app_name.xml'
        else:
            directory = output / f'{tag}.lproj'
            content = ''.join(f'{key} = {json.dumps(name, ensure_ascii=False)};\n' for key in ['CFBundleName', 'CFBundleDisplayName'])
            filename = 'InfoPlist.strings'
        directory.mkdir(parents=True, exist_ok=True)
        (directory / filename).write_text(content, encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('platform', choices=['json', 'android', 'apple', 'linux'])
    parser.add_argument('output', nargs='?', type=Path)
    args = parser.parse_args()
    if args.platform == 'json':
        print(json.dumps(app_names()))
    elif args.output is None:
        parser.error('output is required')
    else:
        write_names(args.platform, args.output)
