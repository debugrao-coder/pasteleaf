# Native v2 开发与验收

状态：工程预览，替代验收 HOLD。权威来源：本仓代码、测试与双端运行结果。最近核查：2026-10-12。

## 构建边界

v2 从 `codex/icloud-sync` 的 `4827212` 开始开发。仓库根的 `src/` 保留 v1 Windows 实现，用 `cargo ... -p pasteleaf` 明确选择；默认 workspace member 是 `pasteleaf-core`，避免在 Mac 意外构建旧 Win32 入口。v2 Windows 客户端是 `pasteleaf-windows`，Mac 原生客户端位于 `clients/macos`。

```sh
cargo fetch --locked
python3 scripts/generate_design_tokens.py --check
cargo test -p pasteleaf-core --locked --release --all-targets -- --test-threads=1
cargo build -p pasteleaf-core --locked --release
python3 scripts/test_v2_abi.py -v
bash clients/macos/build.sh
bash clients/macos/test.sh
python3 scripts/package_v2.py --platform macos --artifact dist/PasteLeaf.app
```

Mac 源码当前使用 Xcode 27 / Swift 6.4 SDK 构建，最低部署目标仍为 macOS 14；CI 使用 GitHub 的 `xcode-27` Apple Silicon runner。Windows 使用 MSVC 与 Windows SDK。打包脚本需要 Python 3.11+。依赖许可证从 Cargo.lock 对应的本机缓存收集并校验；缓存缺失时先执行 `cargo fetch --locked`，不下载来源不明的替代许可证。Mac 工程预览默认本地 ad-hoc 签名；若用户已明确配置专用本机证书，构建使用固定证书签名。两者都不是 Apple Developer ID 公证，不代表已通过其他机器的 Gatekeeper 验证。

本地 ad-hoc 签名的 designated requirement 绑定本次构建的 cdhash，更新后不能保证沿用辅助功能授权。设置页只报告当前进程系统检查结果，在页面出现、返回前台、显示期间以及手动“重新检查”时刷新；点击授权按钮不算授权成功。若系统列表已有旧版本，请由用户在系统设置中移除旧项并重新添加 `/Applications/PasteLeaf.app`，必要时重启应用。不得直接编辑 TCC、重置其他应用权限，或使用仅匹配 bundle identifier 的弱签名要求来绕过授权。稳定跨版本身份需要配置专用代码签名证书，不可冒充 Apple Developer ID。`clients/macos/sign_app.py` 只消费用户已授权配置的本机公开证书指纹，不生成证书、不改信任：默认读取 `~/Library/Application Support/PasteLeaf Signing/identity.json`（`identity_sha1`、`keychain`），或使用 `PASTELEAF_SIGNING_CONFIG` 指向的文件。正式 bundle 的 requirement 同时匹配 bundle identifier 和证书 leaf 指纹；配置存在但失效时停止，不偷偷退回 ad-hoc。Demo/Test 仍使用隔离身份和 ad-hoc 签名。私钥仅保存在钥匙串，不进入仓库。

Mac 验证先构建 ARM64 核心再执行原生测试，避免链接旧的 target-specific 静态库。`PASTELEAF_DIST_DIR` 可指定独立构建目录；未设置时输出到 `dist/`。不要覆盖仍在运行的测试应用。

主面板遵循共享几何参数：铺满当前显示器的可用宽度，底部避开 Dock/任务栏，无独立的最大宽度限制。紧凑高度和卡片高度在 `design/tokens.json` 中维护，字号不随面板变矮而缩小；设置与预览仍保留适合阅读的尺寸。

Windows 正式构建路径在 Windows x64 + Rust MSVC + C/C++ Build Tools 上：

```powershell
cargo test -p pasteleaf-windows --locked --release
cargo build -p pasteleaf-windows --locked --release
python scripts/package_v2.py --platform windows --artifact target/release/pasteleaf-windows.exe
```

Mac 上的 GNU 交叉构建只作为工程预览，不能替代 Windows 原生运行验收。

## 数据与隐私

- 原有 Paste 和旧 PasteLeaf 的工作目录不作为测试目录。不读取 Paste 历史，不改它的开机启动或快捷键。
- 使用独立 v2 数据目录。首次启动需主动开始记录；启用后不自动排除密码或敏感应用，用户可暂停或设置应用排除。
- 本地 SQLite 与附件未加密。iCloud 同步负载使用 Argon2id + XChaCha20-Poly1305；记住密钥通过 Keychain/DPAPI，不保存同步密码。
- 历史不按条数/时间淘汰。超过容量停止新增并显示原因；永久保留不是无限容量承诺。
- v2 不写入旧 v1 同步格式。迁移先备份，再导入独立库；旧库保留。回滚使用旧程序及旧数据副本，不能让旧程序打开 v2 数据。
- 连接前的 `sync_inspect` 只读检查所选目录及其 `PasteLeaf-v2` 子目录，不接收密码、不创建文件、不连接同步。旧 `pasteleaf-v1.json` 目录不能直接加入 v2：双端升级后，由首台设备明确创建独立 v2 库，第二台等待文件下载完成后加入；旧云端历史不会自动迁入。加入不存在/旧版本/未下载的库应返回明确错误，不能先创建空目录再给出泛化 I/O 报错。
- 文件附件同步实际字节，不同步另一台设备无法使用的源路径。目录、链接及未下载的占位文件不在首版支持范围。
- 富文本的 `rtf` 字段是原始字节的 Base64；`html` 为 UTF-8。预览不得执行脚本或请求外网资源。

## C ABI 与线程契约

`pl_api_version` 返回 2。`pl_open(root)` 创建不透明实例；`pl_request(handle,json)` 返回 UTF-8 JSON，调用方必须用 `pl_free` 释放；退出用 `pl_close`。所有结果使用 `{ok:true,data:...}` 或 `{ok:false,error:{code,message}}`，用户数据不得进入日志。绝不从 C ABI 抛出 Rust panic。

UI 命令放在串行后台队列，主线程只更新显示。列表请求有代次标记，旧响应不能覆盖新搜索；后台同步合并当前库，不替换成旧快照。持久化失败不返回成功。

## 验收不能合并

1. Rust 核心与 C ABI 的合成测试：数据、格式、认证加密、冲突和回放。
2. Mac 真实原生界面：截图、键盘、权限拒绝、目标窗口粘贴、多屏。
3. Windows 真实原生界面：100/150/200% 缩放、键盘、剪贴板、UI Automation。
4. iCloud 实际两台设备：专用合成库双向发送，记录两端导入时间及回执，补测断网重连。

`published` 只表示写入本地同步目录；不能显示“已上传 iCloud”。设备回执只能证明对应设备已导入。没有两台真实设备结果时，同步与替代验收继续 HOLD。截图与报告只使用合成内容。
