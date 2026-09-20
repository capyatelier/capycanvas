#!/usr/bin/env python3
"""Bundle the canonical SVGs without changing their geometry or paint rules."""
from pathlib import Path
import sys
import xml.etree.ElementTree as ET


def bundle(directory: Path) -> str:
    icons = []
    for path in sorted(directory.glob("layer-*-symbolic.svg")):
        source = path.read_text().strip()
        root = ET.fromstring(source)
        if root.tag != "{http://www.w3.org/2000/svg}svg" or not source.startswith("<svg "):
            raise ValueError(f"Expected a standalone SVG: {path}")
        name = path.name.removeprefix("layer-").removesuffix("-symbolic.svg")
        icons.append(source.replace("<svg ", f'<svg data-asset="{name}" ', 1))
    return '<svg xmlns="http://www.w3.org/2000/svg">\n' + "\n".join(icons) + "\n</svg>\n"


if __name__ == "__main__":
    root = Path(__file__).resolve().parents[2]
    output = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "apps/layer-web/icons.svg"
    output.write_text(bundle(root / "apps/layer-web/icons"))
