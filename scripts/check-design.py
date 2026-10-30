#!/usr/bin/env python3
"""Check the renderer's real colours, kit usage and readable foreground pairs."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def luminance(rgba):
    c = [x / 255 for x in rgba[:3]]
    c = [v / 12.92 if v <= .04045 else ((v + .055) / 1.055) ** 2.4 for v in c]
    return sum(v * w for v, w in zip(c, (.2126, .7152, .0722)))


def contrast(a, b):
    l, h = sorted((luminance(a), luminance(b)))
    return (h + .05) / (l + .05)


def composite(a, b):
    alpha = a[3] / 255
    return [round(x * alpha + y * (1 - alpha)) for x, y in zip(a[:3], b[:3])] + [255]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-only', action='store_true')
    args = parser.parse_args()
    errors = []
    for file in (ROOT / 'crates/arcade-find/src').rglob('*.rs'):
        source = file.read_text(encoding='utf-8').split('#[cfg(test)]')[0]
        if re.search(r'Rgba::hex\(\s*0x[0-9a-fA-F]{6}|#[0-9a-fA-F]{6}\b|size:\s*\d+(?:\.\d+)?\s*\*', source):
            errors.append(f'{file.relative_to(ROOT)}: literal colour or font size')
    settings = ''.join((ROOT / ('crates/arcade-find/src/' + name)).read_text(encoding='utf-8') for name in ('settings_ui.rs', 'installer_ui.rs'))
    if any(value in settings for value in ('egui::Button::', 'ui.checkbox(', 'ui.radio_value(', 'egui::TextEdit::', 'fn apply_visuals(ctx: &egui::Context, t:')):
        errors.append('Settings must use the shared egui components')
    if errors:
        sys.exit('\n'.join(errors))
    if args.source_only:
        print('Design source checks passed; contrast has not run.')
        return
    output = subprocess.check_output(['cargo', 'run', '--quiet', '--locked', '-p', 'arcade-find', '--example', 'design-contract'], cwd=ROOT, text=True)
    checks = 0
    for palette in json.loads(output):
        surfaces = {name: composite(palette[name], palette['panel']) for name in ('panel', 'selection', 'selection_multi', 'text_selection', 'chip', 'hover')}
        for name in ('text', 'muted', 'highlight'):
            for surface, colour in surfaces.items():
                # Selected input uses body text; secondary labels never sit in
                # the editable text selection.
                if surface == 'text_selection' and name != 'text':
                    continue
                ratio = contrast(palette[name], colour)
                checks += 1
                if ratio < 4.5:
                    errors.append(f"{palette['theme']}: {name} on {surface} is {ratio:.2f}:1, needs 4.5:1")
        for surface, colour in surfaces.items():
            if surface == 'text_selection': continue
            ratio = contrast(palette['accent'], colour)
            checks += 1
            if ratio < 3:
                errors.append(f"{palette['theme']}: focus/selection mark on {surface} is {ratio:.2f}:1, needs 3:1")
        for name, surface in (('danger', 'danger_soft'), ('warn', 'warn_soft'), ('ok', 'panel')):
            ratio = contrast(palette[name], palette[surface])
            checks += 1
            if ratio < 4.5: errors.append(f"{palette['theme']}: {name} on {surface} is {ratio:.2f}:1")
        for kind in palette['kinds']:
            for surface in ('panel', 'hover', 'selection', 'selection_multi'):
                ratio = contrast(kind['fill'], palette[surface])
                checks += 1
                if ratio < 3: errors.append(f"{palette['theme']}: {kind['kind']} on {surface} is {ratio:.2f}:1")
            ratio = contrast(palette['kind_symbol'], kind['fill'])
            checks += 1
            if ratio < 3: errors.append(f"{palette['theme']}: {kind['kind']} symbol is {ratio:.2f}:1")
        ratio = contrast(palette['accent_text'], palette['accent'])
        checks += 1
        if ratio < 4.5: errors.append(f"{palette['theme']}: accent label is {ratio:.2f}:1")
    if errors: sys.exit('\n'.join(errors))
    print(f'Design: {checks} actual renderer contrast pairs passed in both themes; kit controls and type tokens verified.')


if __name__ == '__main__': main()
