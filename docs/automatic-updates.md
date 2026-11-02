# 自动更新协议与发布操作

## 策略

稳定渠道固定使用GitHub Memento1990/codex-task正式Release。后台启动20秒后检查，之后每6小时检查；默认自动检查、自动下载，可关闭。安装需要明确确认，仅退出当前看板，不结束Codex、Reporter或采集器。关闭面板不取消下载。无生产签名公钥的开发构建明确不支持自动更新。

## 首次升级

0.8.1没有更新客户端，需要手动下载一次采用新目录结构的0.9.0。完整解压后运行根目录CodexFloatboard.exe（Rust启动入口），不要直接为versions中的GUI创建固定快捷方式。根目录包含current.json与versions/<version>。开机自启指向固定入口。旧便携平铺目录不原地覆盖。

## 更新安全

原始manifest字节以Ed25519签名，signature为base64文本。公钥通过构建环境FLOATBOARD_UPDATE_PUBLIC_KEY内置到所有核心二进制。签名私钥绝不放仓库、应用包、日志或release资产。SHA256SUMS供人工核对，不能代替签名。

manifest字段：schema=1,version,platform=windows-x86_64,minimum_version,updater_protocol=1,data_compatibility=backward-compatible,url,size,sha256。仅支持正式更高版本，拒绝不兼容协议和不可逆数据迁移。下载限大小，限定官方仓库URL及官方资产重定向。解压采用根文件名白名单，不接受目录、路径穿越、符号链接或重复文件，限制总解压量。

安装helper再次验证清单签名和归档hash，不信任UI传入路径。先解压版本目录，再等待旧GUI正常退出（不kill），以journal切换current.json。新GUI完成主窗口snapshot后返回带nonce健康确认。启动失败或缺少确认则恢复旧入口；新进程未退出时不强制结束，不并发启动旧GUI。数据目录完全独立，不随应用回退覆盖数据库。旧Reporter版本保留，不能删除活动会话占用的文件。安装准备失败留下的.staging目录目前保留供诊断，不自动清理应用版本目录；后续可在全部相关进程退出后人工检查清理。缓存下载文件则按版本与hash寻址复用，重启复验恢复ready，并清理受控命名的旧缓存。

## 密钥准备（仅发布维护者）

1. 构建FloatboardRelease离线工具。用 `FloatboardRelease keygen <受保护的仓库外路径>` 创建32字节随机私钥，记录输出公钥。
2. 验证该外部文件的Windows ACL仅允许维护者/必要系统管理员读取，备份到离线安全存储。工具不会替代操作系统ACL策略。
3. 构建前设置 `FLOATBOARD_UPDATE_PUBLIC_KEY` 为输出公钥；签名时额外设置 `FLOATBOARD_UPDATE_PRIVATE_KEY_FILE` 指向私钥文件。
4. 所有GUI/Reporter/Updater/Launcher必须使用相同公钥构建；测试用fixture密钥不能用于生产发行。
5. GitHub Actions中可将私钥放受保护environment secret，限制tag及审批；不在普通PR构建暴露签名环境。

本机已按用户确认创建生产密钥，私钥位于当前用户LOCALAPPDATA下CodexFloatboard/signing/update-ed25519.key，目录ACL仅当前用户和SYSTEM。仓库app/desktop/update-public-key.txt只含公钥，build-native自动读取并拒绝与环境变量冲突。发布打包逐个运行GUI/Reporter/Updater/Launcher的--build-identity，核对实际版本、公钥和协议，不接受旧key或旧版本二进制。请维护者将私钥备份至安全离线位置；不要将整个用户控制目录上传。

## 发布包

`app/desktop/package-release.ps1 -TargetDirectory <release目标> -OutputDirectory <全新输出目录> -Version 0.9.0 -SignUpdate`。

生成普通便携zip、更新专用zip、manifest、signature与SHA256SUMS。FloatboardRelease.exe仅发布端使用，不进入产品包。输出目录已存在时拒绝覆盖，避免无意替换既有Release资产。签名工具验证私钥与构建公钥匹配。发布前在隔离环境验证从旧版本完整升级、篡改拒绝与失败恢复，再将五项资产作为同一正式Release上传。

## GitHub匿名限流

共享代理IP可能耗尽GitHub匿名API每小时配额。客户端不要求用户Token；API限流时尝试同一官方仓库latest/download的签名清单入口，仍需数字签名验证，绝不改用第三方镜像。旧Release没有签名清单时应明确报告暂时无法检查，不把403/404当作已是最新版。

## 构建提醒

GUI正式构建使用Tauri CLI（build-native release）；不要直接cargo build当作发行GUI。直接cargo build若未启用tauri/custom-protocol会仍打开devUrl而非嵌入前端，必须启用该特性并实测原生窗口。打包身份校验只验证版本/key/protocol，不能替代真实GUI启动验收。

## 安全与运行边界

系统代理、GitHub限流与网络不可达不阻塞任务采集。自动更新不代替Codex重新加载或Hook信任；应用更新不得静默改接入声明。控制目录不是抵御同用户恶意代码的安全边界，HTTPS与更新签名也不替代Windows代码签名。
