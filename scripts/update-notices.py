"""Update Windows dependency notices using Cargo metadata and packaged licenses.

Requires Python 3.11+. Includes test dependencies. Preserves extra upstream
license texts and the independently bundled driver/compiler entries.
"""
import argparse
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output([
        'cargo', 'metadata', '--locked', '--format-version', '1',
        '--filter-platform', 'x86_64-pc-windows-msvc',
    ], cwd=ROOT))
    packages = {p['id']: p for p in metadata['packages']}
    edges = {n['id']: [d['pkg'] for d in n['deps']] for n in metadata['resolve']['nodes']}
    seen, stack = set(), list(metadata['workspace_members'])
    while stack:
        item = stack.pop()
        if item not in seen:
            seen.add(item)
            stack.extend(edges.get(item, []))
    path = ROOT / 'assets/open-source.json'
    entries = json.loads(path.read_text(encoding='utf-8-sig'))
    known = {(e['name'], e['version']) for e in entries}
    missing = [packages[key] for key in seen if packages[key]['source'] is not None
               and (packages[key]['name'], packages[key]['version']) not in known]
    if args.check:
        if missing:
            raise SystemExit('Missing notices: ' + ', '.join(p['name'] + ' ' + p['version'] for p in missing))
        print('Windows runtime, build and test dependency notices are covered.')
        return
    for package in sorted(missing, key=lambda p: (p['name'], p['version'])):
        base = Path(package['manifest_path']).parent
        files = [p for p in base.iterdir() if p.is_file()
                 and re.match(r'(?i)^(licen[cs]e|copying|notice)(\b|[._-])', p.name)]
        if not files:
            raise SystemExit('Missing license text for ' + package['name'])
        notice = '\n\n'.join('### ' + p.name + '\n\n' + p.read_text(encoding='utf-8-sig')
                               for p in sorted(files))
        entries.append(dict(name=package['name'], version=package['version'],
                            license=package['license'], repository=package.get('repository') or '', notice=notice))
    entries.sort(key=lambda e: (e['name'].lower(), e['version']))
    path.write_text(json.dumps(entries, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    notices = ROOT / 'assets/THIRD_PARTY_NOTICES.md'
    intro = notices.read_text(encoding='utf-8-sig').split('\n## ', 1)[0]
    intro = intro.replace('运行与构建依赖', '运行、构建与测试依赖')
    notices.write_text(intro + '\n\n' + '\n\n---\n\n'.join(
        f"## {e['name']} {e['version']}\n\nDeclared license: {e['license']}\n\nUpstream: {e['repository']}\n\n{e['notice']}"
        for e in entries).rstrip() + '\n', encoding='utf-8')
    print(f'{len(entries)} entries; added {len(missing)} dependency notices.')


if __name__ == '__main__':
    main()
