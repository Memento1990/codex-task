"""Exercise a verified signed package without production data, config, or GUI."""
import argparse, hashlib, json, os, socket, sqlite3, subprocess, tomllib, uuid, zipfile
from pathlib import Path
import psutil

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--assets', type=Path, required=True)
parser.add_argument('--evidence', type=Path, required=True)
parser.add_argument('--version', required=True)
parser.add_argument('--no-states', action='store_true')
parser.add_argument('--legacy-receipt', action='store_true')
args = parser.parse_args()
version = args.version
assert __import__('re').fullmatch(r'\d+\.\d+\.\d+', version), 'stable version required'
assets = args.assets.resolve()
base = args.evidence.resolve()
base.mkdir(parents=True, exist_ok=True)
repo = Path(__file__).resolve().parents[3]
subprocess.run(['node', str(repo / 'app/desktop/verify-release.cjs'), str(assets), version,
                str(repo / 'app/desktop/update-public-key.txt')], capture_output=True, check=True,
               creationflags=subprocess.CREATE_NO_WINDOW)
root = Path(os.environ['USERPROFILE']) / 'AppData/Local/CodexFloatboard-Dev/migration-entry' / uuid.uuid4().hex
root.mkdir(parents=True)
evidence = base / ('migration-entry-' + uuid.uuid4().hex[:10])
evidence.mkdir()
tools = root / 'package'
with zipfile.ZipFile(assets / f'Floatboard-update-{version}-windows-x64.zip') as archive:
    archive.extractall(tools)
source = root / '旧版任务.sqlite3'
task = {'id': 'synthetic:session', 'session_id': 'session', 'host_id': 'synthetic',
        'cwd': 'D:/synthetic-entry', 'title': '隔离迁移入口任务', 'status': 'running',
        'runtime': 'unknown', 'revision': 1, 'plan': [], 'blockers': [], 'evidence': []}
with sqlite3.connect(source) as db:
    db.executescript('create table events(id text primary key,task_id text,kind text,at real,payload text);'
                     'create table tasks(id text primary key,payload text);'
                     'create table completed_tasks(id text primary key,source_task_id text,revision integer,payload text);')
    db.execute('insert into tasks values(?,?)', (task['id'], json.dumps(task)))
before = hashlib.sha256(source.read_bytes()).hexdigest()
preferences = root / 'preferences.json'
preferences.write_text(json.dumps({'theme': 'light', 'appearance_revision': 2, 'notifications': False}))
window = root / 'window.json'
window.write_text(json.dumps({'width': 1060, 'height': 740, 'x': 10, 'y': 20, 'pinned': False}))
install = root / 'installed'
config = root / 'registration.json'
flags = {'creationflags': subprocess.CREATE_NO_WINDOW}
with socket.socket() as sock:
    sock.bind(('127.0.0.1', 0))
    port = sock.getsockname()[1]
temp_config = root / 'initial-profile.json'
subprocess.run([str(tools / 'FloatboardCtl.exe'), 'init-test-profile', '--profile', str(temp_config),
                '--root', str(root / 'data'), '--port', str(port), '--agent',
                str(install / 'versions' / version / 'FloatboardAgent.exe')], check=True, **flags)
profile = json.loads(temp_config.read_text())
profile['gui_exe'] = str(install / 'versions' / version / 'CodexFloatboard.exe')
Path(profile['codex_home']).mkdir(parents=True, exist_ok=True)
request = {'version': version, 'work_directory': str(root / 'work'), 'install_root': str(install),
           'tools_directory': str(tools), 'archive': str(assets / f'Floatboard-update-{version}-windows-x64.zip'),
           'manifest': str(assets / 'update-manifest.json'), 'signature': str(assets / 'update-manifest.json.sig'),
           'registration': str(config), 'profile': profile, 'legacy_roots': [str(root / 'old-programs')],
           'sources': [{'label': 'synthetic', 'role': 'baseline', 'database': str(source), 'main_only': False}],
           'state_sources': [{'label': 'preferences', 'kind': 'preferences', 'path': str(preferences)},
                             {'label': 'window', 'kind': 'window', 'path': str(window)}], 'legacy_receipt': None}
without_states = args.no_states
with_legacy = args.legacy_receipt
if without_states:
    request['state_sources'] = []
    request['legacy_roots'] = []
if with_legacy:
    home = Path(profile['codex_home'])
    home.mkdir(parents=True, exist_ok=True)
    old_command = str(root / 'old-programs/FloatboardBridge.exe')
    hook_command = '"' + old_command + '" --hook'
    contribution = {'mcp': {'command': old_command, 'args': ['--mcp']},
                    'hook': {'hooks': [{'type': 'command', 'command': hook_command}]}}
    receipt = root / 'legacy-receipt.json'
    receipt.write_text(json.dumps({'home': str(home), 'status': 'applied', 'contribution': contribution}))
    text = '# preserve unrelated configuration\nmodel="synthetic"\n[mcp_servers.codex_floatboard_dev]\ncommand=' + json.dumps(old_command) + '\nargs=["--mcp"]\n[mcp_servers.unrelated]\nurl="http://127.0.0.1:54321/mcp"\n'
    text += '[[hooks.SessionStart]]\n[[hooks.SessionStart.hooks]]\ntype="command"\ncommand="unrelated-command"\n'
    for event in ['SessionStart','UserPromptSubmit','PreToolUse','PostToolUse','PermissionRequest','Stop','SessionEnd','SubagentStart','SubagentStop']:
        text += '[[hooks.' + event + ']]\n[[hooks.' + event + '.hooks]]\ntype="command"\ncommand=' + json.dumps(hook_command) + '\n'
    (home / 'config.toml').write_text(text)
    request['legacy_receipt'] = str(receipt)
request_file = root / 'request.json'
request_file.write_text(json.dumps(request))
(evidence / 'ownership.json').write_text(json.dumps({'root': str(root), 'profile': profile, 'request': str(request_file)}, indent=2))
results = {'checks': {}, 'root': str(root), 'without_states': without_states, 'legacy_receipt': with_legacy}
def phase(name):
    command = ['pwsh.exe', '-NoProfile', '-File', str(tools / 'migration-install.ps1'),
               '-RequestFile', str(request_file), '-Phase', name]
    if name != 'Preview':
        command.append('-Confirmed')
    result = subprocess.run(command, capture_output=True, **flags)
    (evidence / (name + '.stdout.txt')).write_bytes(result.stdout)
    (evidence / (name + '.stderr.txt')).write_bytes(result.stderr)
    assert result.returncode == 0, (name, result.stderr.decode('utf-8', 'replace'))
    results['checks'][name] = True
def ctl(method, params=None):
    path = root / 'params.json'
    path.write_text(json.dumps(params or {}))
    value = subprocess.run([str(tools / 'FloatboardCtl.exe'), '--profile', str(config),
                            '--method', method, '--params', str(path)], capture_output=True, **flags)
    if value.returncode:
        raise RuntimeError(value.stderr.decode('utf-8', 'replace'))
    return json.loads(value.stdout)
try:
    for name in ['Preview', 'Capture', 'Apply', 'Cutover']:
        phase(name)
    identity = ctl('identity')
    snapshot = ctl('snapshot')
    results['checks']['one_paused_healthy_agent'] = identity['write_gate'] == 'paused' and identity['storage']['status'] == 'ok'
    results['checks']['migrated_task_and_settings'] = any(t['title'] == task['title'] for t in snapshot['tasks']) and (snapshot['options']['theme'] == 'dark' if without_states else snapshot['options']['theme'] == 'light' and ctl('window_load')['width'] == 1060)
    results['checks']['array_contracts_preserved'] = all(isinstance(json.loads((root / 'work' / name).read_text()), list) for name in ['sources.json', 'state-sources.json', 'legacy-roots.json'])
    if with_legacy:
        text = (Path(profile['codex_home']) / 'config.toml').read_text()
        changed = tomllib.loads(text)
        results['checks']['only_owned_legacy_connections_replaced'] = changed['mcp_servers']['codex_floatboard_dev']['url'] == f'http://127.0.0.1:{port}/mcp' and changed['mcp_servers']['unrelated']['url'] == 'http://127.0.0.1:54321/mcp' and 'unrelated-command' in text and 'FloatboardBridge.exe' not in text and '# preserve unrelated configuration' in text
    journal = json.loads((install / 'install-transaction.json').read_text())
    results['checks']['activation_requires_gui_and_oauth'] = journal['phase'] == 'healthy_paused'
    results['checks']['input_unchanged'] = hashlib.sha256(source.read_bytes()).hexdigest() == before
    assert all(results['checks'].values())
finally:
    try:
        ctl('stop', {'confirmed': True})
    except RuntimeError:
        pass
    task_name = 'CodexFloatboard-Test-' + profile['profile_id']
    query = subprocess.run(['schtasks.exe', '/Query', '/TN', task_name], capture_output=True, **flags)
    if query.returncode == 0:
        removed = subprocess.run([str(tools / 'FloatboardCtl.exe'), 'test-task', '--profile', str(config), '--action', 'remove'], capture_output=True, **flags)
        assert removed.returncode == 0, removed.stderr.decode('utf-8', 'replace')
    results['checks']['test_task_removed'] = subprocess.run(['schtasks.exe', '/Query', '/TN', task_name], capture_output=True, **flags).returncode != 0
    owned = []
    for process in psutil.process_iter(['pid', 'exe']):
        try:
            if process.info['exe'] and process.info['exe'].lower().startswith(str(root).lower() + '\\'):
                owned.append(process.pid)
        except (psutil.AccessDenied, psutil.NoSuchProcess):
            pass
    results['checks']['owned_processes_exited'] = not owned
    results['remaining_pids'] = owned
    (evidence / 'results.json').write_text(json.dumps(results, indent=2))
print(json.dumps(results))
