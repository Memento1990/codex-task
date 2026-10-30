"""只比较用户可理解的业务变化，不把运行心跳变成消息。"""
import copy

TASK_STATUS = dict(running="进行中", blocked="有卡点", waiting="待确认", completed="已完成", cancelled="已取消", unreported="未汇报")
STEP_STATUS = dict(pending="待执行", inProgress="进行中", completed="已完成", blocked="受阻")


class ChangeDetector:
    def __init__(self):
        self.previous = None

    def update(self, tasks):
        current = {task["id"]: task for task in tasks if task["status"] != "unreported" or task["plan"]}
        notices = []
        if self.previous is not None:
            for identifier, task in current.items():
                old = self.previous.get(identifier)
                lines = []
                tab = 0
                if old is None or old["status"] in ("completed", "cancelled") and task["status"] not in ("completed", "cancelled") and old["title"] != task["title"]:
                    lines.append("新增任务 · " + TASK_STATUS[task["status"]])
                    for item in task["blockers"]:
                        if item["open"]:
                            lines.append("新增卡点 · " + item["title"])
                            tab = 1
                else:
                    if old["status"] != task["status"]:
                        lines.append("任务 · " + TASK_STATUS[old["status"]] + " → " + TASK_STATUS[task["status"]])
                    before = {item["id"]: item for item in old["plan"]}
                    for item in task["plan"]:
                        previous = before.get(item["id"])
                        if previous is None:
                            lines.append("新增清单 · " + item["title"])
                        elif previous["status"] != item["status"]:
                            lines.append(item["title"] + " · " + STEP_STATUS[item["status"]])
                    before = {item["id"]: item for item in old["blockers"]}
                    for item in task["blockers"]:
                        previous = before.get(item["id"])
                        if previous is None and item["open"] or previous and previous["open"] != item["open"]:
                            lines.insert(0, ("新增卡点 · " if previous is None else "卡点待处理 · " if item["open"] else "卡点已解除 · ") + item["title"])
                            tab = 1
                    present = {item["id"] for item in task["blockers"]}
                    for item in old["blockers"]:
                        if item["open"] and item["id"] not in present:
                            lines.insert(0, "卡点已解除 · " + item["title"])
                            tab = 1
                if lines:
                    shown = lines[:3]
                    if len(lines) > 3:
                        shown.append("另有 " + str(len(lines) - 3) + " 项变化")
                    notices.append(dict(task_id=identifier, title=task["title"] or "任务更新", lines=shown, tab=tab))
        # 只保留业务比较字段，避免复制动态和事件历史。
        self.previous = {identifier: copy.deepcopy({key: task[key] for key in ("id", "title", "status", "plan", "blockers")}) for identifier, task in current.items()}
        return notices
