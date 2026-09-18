#!/usr/bin/env python3
"""Render the shared Rust sample report as a standalone SDR comparison SVG.

cargo run --release -p layer-core --example sdr_highlights > samples.json
python3 tools/validation/sdr_highlights.py samples.json highlights.svg
"""
import html
import json
from pathlib import Path
import sys

samples = json.loads(Path(sys.argv[1]).read_text())
width, left, bar_width, bar_height = 1120, 270, 816, 22
height = 120 + len(samples) * 29 + 6 * 22
parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" shape-rendering="crispEdges">',
         f'<rect width="{width}" height="{height}" fill="#17191c"/>',
         '<style>text{font-family:sans-serif;fill:#e4e7eb;font-size:13px}.title{font-size:21px;font-weight:bold}.color{font-size:15px;font-weight:bold}</style>',
         '<text x="24" y="33" class="title">HDR highlights → SDR</text>',
         '<text x="24" y="56">Same linear sRGB input. Fixed 1000-nit range; exposure and contrast neutral. HDR master unchanged.</text>']
y = 91
last = None
for record in samples:
    if last != record['color']:
        if last is not None:
            y += 22
        last = record['color']
        parts.append(f'<text x="24" y="{y+16}" class="color">{html.escape(last)}</text>')
    parts.append(f'<text x="97" y="{y+16}">{html.escape(record["method"])}</text>')
    for i, sample in enumerate(record['samples']):
        color = '#'+''.join(f'{round(min(1,max(0,c))*255):02x}' for c in sample['display'])
        x = left + i * bar_width / len(record['samples'])
        parts.append(f'<rect x="{x:.3f}" y="{y}" width="{bar_width/len(record["samples"])+0.03:.3f}" height="{bar_height}" fill="{color}"><title>{sample["ev"]:.2f} EV; linear {sample["linear"]}</title></rect>')
    y += 29
for ev in [-4,-2,0,2,4,6,8]:
    x = left + (ev+4)/12*bar_width
    parts.append(f'<text x="{x}" y="{y+15}" text-anchor="middle">{ev:+d} EV</text>')
parts.append(f'<text x="24" y="{y+43}">White favors luminance; Color favors saturation. These swatches are SDR, not physical HDR display measurements.</text>')
parts.append('</svg>')
Path(sys.argv[2]).write_text('\n'.join(parts)+'\n')
