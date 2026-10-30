import sys
import uuid

import pytest

from floatboard import preferences


def test_defaults_and_corrupt_settings_recovery(tmp_path):
    assert preferences.load(tmp_path) == {**preferences.DEFAULTS, "startup_command": ""}
    (tmp_path / "preferences.json").write_text('{"theme":"unknown"}', encoding="utf-8")
    assert preferences.load(tmp_path)["theme"] == "light"
    (tmp_path / "preferences.json").write_text('[]', encoding="utf-8")
    assert not preferences.load(tmp_path)["show_taskbar"]


@pytest.mark.parametrize("value", [dict(notification_seconds=0), dict(notification_seconds=31), dict(notification_seconds=True), dict(theme="blue"), dict(notifications="true"), dict(auto_start=1), dict(show_taskbar=None)])
def test_reject_invalid_settings(value):
    with pytest.raises(ValueError):
        preferences.validate(value)


def test_save_restores_preferences_without_touching_startup(tmp_path, monkeypatch):
    monkeypatch.setattr(preferences, "set_startup", lambda *_: pytest.fail("未切换自启不应修改注册项"))
    preferences.save(tmp_path, dict(theme="dark", notification_seconds=10, notifications=False, show_taskbar=True))
    saved = preferences.load(tmp_path)
    assert saved["theme"] == "dark" and saved["notification_seconds"] == 10
    assert saved["show_taskbar"] and not saved["notifications"]


@pytest.fixture
def isolated_run(monkeypatch):
    import winreg
    key = "Software\\CodexFloatboardTest-" + uuid.uuid4().hex
    monkeypatch.setattr(preferences, "RUN_KEY", key)
    monkeypatch.setattr(preferences, "RUN_NAME", "IsolatedTestOnly")
    try:
        yield winreg, key
    finally:
        try:
            winreg.DeleteKey(winreg.HKEY_CURRENT_USER, key)
        except FileNotFoundError:
            pass


@pytest.mark.skipif(sys.platform != "win32", reason="Windows 当前用户注册项")
def test_real_isolated_autostart_roundtrip_and_conflict(tmp_path, isolated_run):
    winreg, path = isolated_run
    command = preferences.startup_command(tmp_path)
    saved = preferences.save(tmp_path, dict(auto_start=True))
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
        assert winreg.QueryValueEx(key, preferences.RUN_NAME)[0] == command
    assert saved["startup_command"] == command and saved["auto_start"]
    preferences.save(tmp_path, dict(auto_start=False))
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path, 0, winreg.KEY_ALL_ACCESS) as key:
        with pytest.raises(FileNotFoundError):
            winreg.QueryValueEx(key, preferences.RUN_NAME)
        winreg.SetValueEx(key, preferences.RUN_NAME, 0, winreg.REG_SZ, "owned-by-another-config")
    before = (tmp_path / "preferences.json").read_bytes()
    with pytest.raises(ValueError, match="未覆盖"):
        preferences.save(tmp_path, dict(auto_start=True))
    assert (tmp_path / "preferences.json").read_bytes() == before
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
        assert winreg.QueryValueEx(key, preferences.RUN_NAME)[0] == "owned-by-another-config"


@pytest.mark.skipif(sys.platform != "win32", reason="Windows 当前用户注册项")
def test_startup_rolls_back_when_save_fails(tmp_path, isolated_run, monkeypatch):
    winreg, path = isolated_run
    monkeypatch.setattr(preferences, "atomic_json", lambda *_: (_ for _ in ()).throw(OSError("模拟存储失败")))
    with pytest.raises(OSError):
        preferences.save(tmp_path, dict(auto_start=True))
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
        with pytest.raises(FileNotFoundError):
            winreg.QueryValueEx(key, preferences.RUN_NAME)
    assert not (tmp_path / "preferences.json").exists()


def test_startup_quotes_paths_and_forwards_original_data_dir(tmp_path, monkeypatch):
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    monkeypatch.setattr(sys, "executable", r"C:\Program Files\CodexFloatboard\CodexFloatboard.exe")
    value = preferences.startup_command(tmp_path / "data with spaces")
    assert value.startswith('"C:\\Program Files\\CodexFloatboard\\CodexFloatboard.exe"')
    assert '--data-dir "' in value and 'data with spaces"' in value
