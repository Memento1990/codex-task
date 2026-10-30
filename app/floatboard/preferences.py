"""少量本机偏好；自启只管理当前用户的本工具项。"""
import json
from pathlib import Path
import subprocess
import sys

from .core import atomic_json

DEFAULTS = dict(auto_start=False, show_taskbar=False, theme="light", notifications=True, notification_seconds=3)
RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
RUN_NAME = "CodexFloatboard"


def validate(value):
    if not isinstance(value, dict):
        raise ValueError("设置格式无效")
    result = {**DEFAULTS, **{key: value[key] for key in DEFAULTS if key in value}}
    if any(type(result[key]) is not bool for key in ("auto_start", "show_taskbar", "notifications")):
        raise ValueError("开关设置无效")
    if result["theme"] not in ("light", "dark"):
        raise ValueError("主题设置无效")
    if type(result["notification_seconds"]) is not int or not 1 <= result["notification_seconds"] <= 30:
        raise ValueError("通知停留时间须为 1–30 秒")
    return result


def load(root):
    try:
        raw = json.loads((Path(root) / "preferences.json").read_text(encoding="utf-8"))
        return {**validate(raw), "startup_command": raw.get("startup_command", "")}
    except (OSError, ValueError, TypeError):
        return {**DEFAULTS, "startup_command": ""}


def startup_command(root):
    executable = Path(sys.executable)
    if getattr(sys, "frozen", False):
        args = [str(executable)]
    else:
        # 开发运行不弹控制台，正式包直接使用 exe。
        executable = executable.with_name("pythonw.exe")
        args = [str(executable), str(Path(__file__).parents[1] / "main.py")]
    return subprocess.list2cmdline([*args, "--data-dir", str(Path(root).resolve())])


def set_startup(enabled, desired, owned):
    import winreg
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, RUN_KEY) as key:
        try:
            current, _ = winreg.QueryValueEx(key, RUN_NAME)
        except FileNotFoundError:
            current = None
        if current is not None and current not in (desired, owned):
            raise ValueError("自启项已由其他配置占用，未覆盖")
        if enabled:
            winreg.SetValueEx(key, RUN_NAME, 0, winreg.REG_SZ, desired)
        elif current is not None:
            winreg.DeleteValue(key, RUN_NAME)
    return current


def save(root, value):
    previous = load(root)
    result = validate(value)
    desired = startup_command(root)
    startup_changed = result["auto_start"] != previous["auto_start"] or result["auto_start"] and desired != previous["startup_command"]
    old_command = None
    if startup_changed:
        old_command = set_startup(result["auto_start"], desired, previous["startup_command"])
    result["startup_command"] = desired if result["auto_start"] else ""
    try:
        atomic_json(Path(root) / "preferences.json", result)
    except OSError:
        if startup_changed:
            set_startup(old_command is not None, old_command or desired, desired)
        raise
    return result
