"""Regenerate resolved Rust and installed pnpm notices without network access."""
import hashlib, json, subprocess
from pathlib import Path

desktop = Path(__file__).resolve().parent
repo = desktop.parent.parent
metadata = json.loads(subprocess.check_output([
    'cargo', 'metadata', '--manifest-path', str(desktop/'Cargo.toml'), '--locked',
    '--offline', '--filter-platform', 'x86_64-pc-windows-msvc', '--format-version', '1']))
version = json.loads((desktop/'package.json').read_text(encoding='utf-8'))['version']
entries = {}
supplements = json.loads((desktop/'third-party-license-sources.json').read_text(encoding='utf-8'))
for package in metadata['packages']:
    if package['id'] in metadata['workspace_members']:
        continue
    entries[('Rust', package['name'], package['version'])] = (
        Path(package['manifest_path']).parent, package.get('license'), package.get('license_file'), package.get('authors'), package.get('repository'))
for manifest in (desktop/'node_modules/.pnpm').glob('*/node_modules/**/package.json'):
    # Only package roots, not examples or nested fixtures inside a dependency.
    relative = manifest.relative_to(desktop/'node_modules/.pnpm').parts
    if len(relative) not in (4, 5) or len(relative) == 5 and not relative[2].startswith('@'):
        continue
    package = json.loads(manifest.read_text(encoding='utf-8'))
    if package.get('name') and package.get('version'):
        entries[('npm', package['name'], package['version'])] = (manifest.parent, package.get('license'), None, package.get('author'), package.get('repository'))
lines = [f'THIRD-PARTY NOTICES - Codex Floatboard {version}', '',
         'Generated from the unified locked Cargo graph and installed frozen pnpm packages.',
         'Includes build dependencies; inclusion does not assert linkage in every executable.',
         'Third-party copyrights and licenses remain with their respective authors.', '']
missing, supplemented, standard_only = [], [], []
for (ecosystem, name, version), (directory, license_id, explicit, authors, repository) in sorted(entries.items()):
    files = [p for p in directory.iterdir() if p.is_file() and p.name.lower().startswith(('license', 'copying', 'notice', 'unlicense'))]
    if explicit:
        target = Path(explicit)
        if not target.is_absolute():
            target = directory/target
        if target.is_file() and target not in files:
            files.append(target)
    lines += ['='*72, f'{ecosystem}: {name} {version}', f'SPDX / declared license: {license_id or "see license text"}',
              'Authors (package metadata): '+json.dumps(authors, ensure_ascii=False),
              'Repository (package metadata): '+json.dumps(repository, ensure_ascii=False), '']
    for path in sorted(files):
        lines += [f'--- {path.name} ---', path.read_text(encoding='utf-8', errors='replace').strip(), '']
    if not files:
        key=f'{ecosystem}:{name}@{version}'
        for item in supplements.get(key, []):
            raw=(repo/item['path']).read_bytes()
            if hashlib.sha256(raw).hexdigest()!=item['sha256']:
                raise RuntimeError('Supplemental license checksum differs: '+key)
            lines += ['--- Supplemental license ---', 'Source: '+item['source'], raw.decode('utf-8').strip(), '']
            if 'standard declared license text' in item['source']:
                standard_only.append(key)
        if key in supplements:
            supplemented.append(key)
        else:
            missing.append(key)
(repo/'docs/THIRD-PARTY-NOTICES.txt').write_text('\n'.join(lines)+'\n', encoding='utf-8')
print(json.dumps({'dependencies':len(entries), 'missing_license_text':missing, 'supplemented':supplemented,
                  'standard_text_with_package_authorship':standard_only}, indent=2))
