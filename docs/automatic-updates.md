# 更新协议 2

稳定渠道为 GitHub Memento1990/codex-task。自动检查和下载可分别关闭；安装需要明确确认。DEV 禁止正式更新。

签名清单包含 schema=2、updater_protocol=2、agent_protocol=2、database_schema_min/max、minimum_version、version、platform、url、size、sha256。兼容范围必须明确，不使用“向后兼容”常量。当前仅支持 schema 2。

原始 manifest 字节使用 Ed25519 签名，公钥固定在 update-public-key.txt 并嵌入全部程序。私钥在受保护的仓库外位置，不进入程序、日志和资产。SHA256SUMS 不能替代签名。package-release 检查实际二进制身份；verify-release 独立复核签名和全部资产。

更新事务顺序：停写、备份、正常停止 Agent、同互斥权离线校验、切版本及计划任务、恢复 owned 接入、新 Agent 保持暂停、新 GUI/实际库/OAuth 验证、开放写入。journal 持久化每个阶段，可检查后继续。正常退出不触发失败重启；旧任务先禁用，避免切换时抢启。

新版本接收写入后，不会自动恢复旧库或程序入口。程序回退只能由离线维护确认 schema 兼容性后执行；数据库恢复必须到新目标。遇到旧更新 journal 明确转专用迁移流程，不继续旧回退机制。

1.x 首次迁移使用 [专用安装入口](migration-install.md)。普通更新不承担历史冲突自动裁决，不强杀 Codex 或旧写入者，不改非本产品配置。

发布使用 [publish-release.ps1](../app/desktop/publish-release.ps1)，详见 [发布手册](releasing.md)。正式私钥仅按已登记路径由签名工具读取，不生成替代密钥绕过公钥。
