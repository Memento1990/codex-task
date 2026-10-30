import importlib
import json
from pathlib import Path

import pytest
from floatboard.core import Store
from test_core import event, report
from desktop_bridge import DesktopBridge


@pytest.fixture
def bridge(tmp_path, monkeypatch):
    monkeypatch.setenv('CODEX_HOME', str(tmp_path/'codex'))
    original = importlib.import_module('floatboard.preferences').startup_command
    store = Store(tmp_path)
    store.ingest(event('progress', report=report(title='迁移兼容验收')))
    adapter = DesktopBridge(tmp_path, tmp_path/'CodexFloatboard.exe')
    try:
        yield adapter, store
    finally:
        importlib.import_module('floatboard.preferences').startup_command = original
        if adapter.store: adapter.store.close()
        store.close()


def test_secondary_snapshot_does_not_consume_main_notifications(bridge):
    adapter, store = bridge
    assert not adapter.call('snapshot', {'notify':True})['notices']
    store.ingest(event('progress', report=report(revision=1, status='waiting')))
    assert not adapter.call('snapshot', {'notify':False})['notices']
    value = adapter.call('snapshot', {'notify':True})
    assert len(value['notices'])==1 and value['tasks'][0]['status']=='waiting'
    assert 'session_token' not in json.dumps(value)
    assert not adapter.call('snapshot', {'notify':True})['notices']


def test_desktop_window_state_preserves_legacy_fields(bridge):
    adapter, _ = bridge
    adapter.call('window_save', dict(pinned=False,selected='legacy',tab=2))
    adapter.call('window_save', dict(width=920,height=680,x=25,y=30,unexpected='ignored'))
    value = adapter.call('window_load',{})
    assert value==dict(pinned=False,selected='legacy',tab=2,width=920,height=680,x=25,y=30)


def test_desktop_options_do_not_touch_startup_when_disabled(bridge):
    adapter, _ = bridge
    value=adapter.call('preferences',dict(auto_start=False,show_taskbar=False,theme='dark',notifications=False,notification_seconds=5))
    assert value['theme']=='dark' and value['startup_command']==''
    assert adapter.call('snapshot',{'notify':True})['options']['notification_seconds']==5


def test_desktop_preview_is_scoped_and_unknown_commands_rejected(bridge):
    adapter, _ = bridge
    value=adapter.call('preview',{})
    assert set(value)=={'changes'} and len(value['changes'])==10
    assert 'updated' not in value and 'session_token' not in json.dumps(value)
    with pytest.raises(ValueError,match='未知'):adapter.call('arbitrary-shell',{})


def test_bridge_disconnect_returns_success_when_no_change(bridge,monkeypatch):
    adapter, _ = bridge
    monkeypatch.setattr('desktop_bridge.apply_preview',lambda *args:None)
    assert adapter.call('disconnect',{})==dict(success=True,changed=False)
