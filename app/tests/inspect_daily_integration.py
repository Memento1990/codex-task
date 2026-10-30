"""只读查询新 App Server 发现的 Hook，不运行模型轮次，不放行信任。"""
import json
from pathlib import Path
import queue
import subprocess
import threading
import time

ROOT = Path(__file__).parents[1]
CODEX = r"C:\Users\Memento\AppData\Local\OpenAI\Codex\bin\08d187884f2330d1\codex.exe"


def main():
    process = subprocess.Popen([CODEX, "app-server"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, cwd=ROOT, creationflags=subprocess.CREATE_NO_WINDOW)
    incoming = queue.Queue()
    def read():
        for line in iter(process.stdout.readline, b""):
            try:
                incoming.put(json.loads(line))
            except ValueError:
                pass
    threading.Thread(target=read, daemon=True).start()
    def send(value):
        process.stdin.write((json.dumps(value) + "\n").encode("utf-8"))
        process.stdin.flush()
    begin = time.time()
    result = None
    try:
        send(dict(id=1, method="initialize", params=dict(clientInfo=dict(name="floatboard-readonly-check", version="0.1.0"))))
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            try:
                message = incoming.get(timeout=.2)
            except queue.Empty:
                continue
            if message.get("id") == 1 and "result" in message:
                send(dict(method="initialized"))
                send(dict(id=2, method="hooks/list", params=dict(cwds=[str(ROOT)])))
            if message.get("id") == 2:
                result = message
                break
    finally:
        process.terminate()
        process.wait(timeout=5)
    owned = []
    def walk(value):
        if isinstance(value, dict):
            if "eventName" in value and "FloatboardReporter.exe" in json.dumps(value):
                owned.append({k: value.get(k) for k in ["eventName", "enabled", "trustStatus", "statusMessage", "source", "sourcePath"]})
            for child in value.values():
                walk(child)
        elif isinstance(value, list):
            for child in value:
                walk(child)
    if result:
        walk(result.get("result"))
    record = dict(at=time.strftime("%Y-%m-%dT%H:%M:%S%z"), duration_s=round(time.time() - begin, 2), method="hooks/list", no_model_turn=True, no_trust_bypass=True, response_received=result is not None and "result" in result, hook_count=len(owned), hooks=owned)
    (ROOT / "docs/evidence/daily-hook-discovery.json").write_text(json.dumps(record, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(record, ensure_ascii=True))
    return 0 if record["response_received"] and len(owned) == 9 else 1


if __name__ == "__main__":
    raise SystemExit(main())
