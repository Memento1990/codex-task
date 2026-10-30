"""无 GUI 的 Hook / MCP / Worker 入口。"""
import argparse
import json
import os
import sys

from floatboard.core import data_root
from floatboard.ingest import hook, serve_mcp
from floatboard.runtime import spawn_worker, worker


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--hook", action="store_true")
    parser.add_argument("--mcp", action="store_true")
    parser.add_argument("--worker", action="store_true")
    parser.add_argument("--connect", action="store_true", help="自动预检、备份并接入当前 Codex")
    parser.add_argument("--data-dir")
    args = parser.parse_args()
    root = data_root(args.data_dir)
    if args.connect:
        from floatboard.integration import connect
        import subprocess
        try:
            result = connect(root)
            print(json.dumps(dict(result, success=True, message="接入已就绪，等待 Codex 首次同步；现有任务无需重启。"), ensure_ascii=False))
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            print(json.dumps(dict(success=False, message=str(error)), ensure_ascii=False))
            return 1
    elif args.worker:
        worker(root)
    elif args.mcp:
        if os.environ.get("FLOATBOARD_PREFLIGHT") != "1":
            spawn_worker(root)
        serve_mcp(root, sys.stdin, sys.stdout)
    else:
        result = {}
        try:
            payload = json.loads(sys.stdin.read(1_000_001))
            result = hook(root, payload)
            spawn_worker(root)
        except (ValueError, OSError, TypeError, KeyError):
            pass
        print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    if hasattr(sys.stdin, "reconfigure"):
        sys.stdin.reconfigure(encoding="utf-8")
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    raise SystemExit(main())
