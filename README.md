# PasteLeaf

> **v2 工程预览**：原生 macOS、重构后的 Windows 客户端、SQLite 永久历史和 v2 加密同步已在此分支实现。构建入口见 [Native v2 开发](docs/V2-DEVELOPMENT.md)，具体通过项与缺口见 [验收记录](docs/V2-VERIFICATION.md)，共享视觉契约见 [DESIGN.md](DESIGN.md)。真实 Windows 运行与 Mac ↔ Windows iCloud 往返未验收前，不视为 Paste 的完整替代。旧数据和旧同步库不自动升级。

## v1 Windows 使用说明

以下说明适用于保留的 v1 客户端；构建旧版时为 Cargo 命令加 `-p pasteleaf`，不要与 v2 数据格式混用。

[![Windows CI](https://github.com/debugrao-coder/pasteleaf/actions/workflows/ci.yml/badge.svg)](https://github.com/debugrao-coder/pasteleaf/actions/workflows/ci.yml)

Windows 10/11 x64 剪贴板历史与收藏工具，使用 Rust、原生 Win32 和 GDI+，无需浏览器运行时。

[下载便携包](https://github.com/debugrao-coder/pasteleaf/releases/latest)

## 使用

解压便携包，双击 `pasteleaf.exe`。复制文字、链接或图片后，按 **Ctrl+Q** 呼出屏幕底部的卡片栏。首次启动若快捷键被占用，可从托盘进入设置更换。

- 左右方向键选择卡片，滚轮浏览；**Enter 或双击**粘贴到呼出前的窗口。Ctrl+1…9 快速粘贴当前页对应卡片。
- 直接输入开始搜索，Ctrl+F 聚焦搜索框；空格预览完整内容，Ctrl+C 仅复制。
- 顶部「+」新建收藏分类，支持名称和颜色；拖动卡片到分类，或通过卡片右键菜单收藏。
- 进入收藏分类后，点击「+ 新建收藏项」、按 **Ctrl+N**，或从分类右键菜单手动录入文字、链接和提示词。支持多行：Enter 换行，**Ctrl+Enter** 或「保存收藏项」保存；取消或 Esc 放弃。暂停记录时仍可添加。
- 收藏卡片右键可「编辑标签…」，新建收藏也可填写。中英文逗号分隔，最多 8 个标签、每个最多 24 个字符；搜索匹配正文和标签。卡片显示小型类型图标，标签过多时显示 `+N`，悬停可查看完整标签；复制粘贴仍只输出原始内容。
- 右键分类可重命名或删除。删除分类保留内容，清空历史保留收藏。
- 设置支持浅色、深色和跟随系统，默认深色。卡片、输入框、预览、窗口边缘和所有菜单跟随主题；菜单可用方向键和 Enter 操作，→ 进入子菜单，← 或 Esc 返回，点击菜单外收起。分类较多时可滚动查看。
- 自定义全局快捷键支持 Ctrl、Alt、Shift、Win 与字母、数字、Space、F1–F11。Ctrl+V 保留给粘贴；快捷键冲突或保存失败时保留原设置。
- 开机启动默认关闭。设置开启后，登录 Windows 时进入托盘；暂停状态、分类和外观会保留到下次启动。

| 快捷键 | 操作 |
| --- | --- |
| Ctrl+Q（可自定义） | 显示或收起卡片栏 |
| Ctrl+, | 设置 |
| Ctrl+F | 搜索 |
| Ctrl+左右 | 切换分类 |
| Ctrl+N | 在当前收藏分类新建收藏项 |
| Ctrl+Shift+N | 新建分类 |
| Ctrl+T | 暂停或继续记录 |
| Esc | 关闭菜单、取消编辑或收起卡片栏 |

托盘左键打开，右键可设置、暂停或退出。开机启动记录当前程序路径，移动程序后需重新开启；Windows 任务管理器或组织策略也可能禁用启动项。

## 数据与限制

数据默认保存在 `%LOCALAPPDATA%\PasteLeaf`：`library.json` 存文字、分类、设置和同步队列，`images/` 存 PNG。本地数据**未加密**；iCloud Drive 加密同步默认关闭。不要将本地工作数据目录直接放入 iCloud Drive。备份时退出程序，复制整个数据目录。

每次保存前保留上一版有效数据 `library.previous.json`。读取失败不会覆盖原文件，写入失败会提示。最多保留 200 条未收藏历史、500 条总记录；单条文字上限 512 KiB，文字合计 64 MiB，普通序列化文件 70 MiB，含同步队列时 256 MiB；单张图片解码后 RGBA 上限 16 MiB。收藏不会随历史自动清理。

目前支持纯文本和图片，不支持富文本、文件附件、OCR、多选连续粘贴。启动时不读取剪贴板原有内容，只记录启动后的复制事件。图片可用「图片」搜索。

粘贴前会检查目标窗口和剪贴板。权限更高的程序可能需要手动 Ctrl+V；PasteLeaf 只发送粘贴组合键，不发送回车提交内容。程序未经代码签名。

「便携包」表示无需安装；默认数据仍写入用户的本地应用数据目录。需要隔离数据时，可在启动前设置 `PASTELEAF_DATA_DIR`。

## iCloud Drive 加密同步

两台 Windows 电脑均需安装、登录 iCloud，并启用 iCloud Drive。PasteLeaf 不直接登录 Apple 账号，也无法确认文件已经到达云端；状态「已写入同步文件夹」仅表示本机文件交接完成，传输情况以 iCloud 客户端为准。建议在资源管理器中将专用同步文件夹设为「始终保留在此设备上」，参见 [Apple 的下载保留说明](https://support.apple.com/en-gb/guide/icloud-windows/icw8531ad6b7/icloud)。本功能不提供 iPhone、iPad 或 Mac 版 PasteLeaf。

1. 按 **Ctrl+,** → **iCloud Drive 同步**，在 iCloud Drive 目录中选择专用空文件夹，或使用自动建议的位置。
2. 设置至少 12 个字符的独立同步密码，两次输入须一致。默认仅收藏，可选择全部记录。点击「创建并启用」，检查实际范围和待上传数量后确认。关闭或取消不会上传正文；若已生成空库描述，再次创建可继续完成设置。
3. 等待 iCloud Drive 将文件下载到另一台电脑，在该电脑选择对应文件夹，输入相同密码，点击「加入并启用」。加入时沿用同步库的范围，并先合并已有记录。相同正文但来源身份不同的记录会分别保留。

自动建议位置为已有的 `%USERPROFILE%\iCloudDrive\PasteLeaf Sync`（兼容 `iCloud Drive` 目录名）。若 iCloud 使用自定义位置，请通过「选择文件夹」指定；未发现目录时不会创建假冒的 iCloud 根目录。更新程序不会自动启用同步、移动文件或更改已有连接。若此前连接过 OneDrive，请先断开，再在 iCloud Drive 中创建新库；旧云端文件保留，仍需自行管理。

收藏正文、标签、分类、分类颜色及图片参与同步；主题、快捷键、窗口设置及捕获暂停状态只留本机。默认记住本机密钥，可关闭；关闭后每次重启需输入密码才能同步，本地记录仍可使用。同步暂停期间仍可编辑，恢复后发送待同步变更。

手动删除会传播到其他设备。清空历史和自动淘汰仅清理当前电脑，重复扫描不会使这些记录重新出现。仅收藏模式下取消收藏，各设备已有正文保留为本地历史。分类和记录都有稳定标识；同名不同身份的分类显示短后缀。并发修改进入「冲突」，可逐版查看正文、标签及图片，选择保留此版或全部保留；删除和修改冲突不会静默丢弃修改。

范围修改属于整个同步库。切回仅收藏会停止后续历史同步，已有本地历史和云端加密操作文件不会自动清除。图片尚未下载或本机容量不足时，记录保持待导入；下载完成或腾出空间后重试。

同步文件采用 Argon2id（64 MiB、3 次、单通道）和 XChaCha20-Poly1305；描述文件只公开协议、随机标识及密钥派生参数。记住的密钥由 Windows 当前用户 DPAPI 保护，放在本地配置中，不进入同步目录；密码不保存。**本地原始记录仍未加密。** 同步密码无法在线找回；更换密码需创建新库，旧库不会自动删除。

本地修改与待发送操作一起原子保存；后台合并等待 2 秒，持续修改最长等待 10 秒，平时每 10 秒扫描。文件不完整、密码错误、认证失败、未知协议或写入失败都会保留原数据，重试间隔最多 60 秒。关闭程序时未发送完的队列会在下次启动后继续。

首版保留云端不可变操作历史和删除墓碑，不自动清理。同步目录达到 2 GiB 后停止新增发布，保留待发送队列；本地仍可使用，必要时创建新同步库。已确认的旧版本正文会从本机同步元数据中压缩为因果标记，未发送、冲突及待导入版本仍保留。

### 恢复与验证边界

- 暂停或断开不会删除 iCloud Drive 文件。断开移除本机保存的密钥和目录连接；稳定标识保留，重新加入同一库不会因为断开而复制所有记录。程序生成的上一版数据备份不保留密钥，恢复该备份后需重新输入同步密码。
- 保存失败时保留输入，排除权限或空间问题后重试；首次创建被中断且尚未发布操作时，可再次选择创建。不要手动删除操作或图片文件，旧设备依赖它们完成合并。
- 回退程序前退出 PasteLeaf，并备份完整本地目录。旧程序不理解同步队列，回退后不要让旧版本继续参与同一同步库；恢复新版本时使用对应的新版本数据备份。
- 测试使用隔离的 `PASTELEAF_DATA_DIR`、临时同步目录和合成内容。两份本机设备状态可以验证合并协议，但不能替代真实 iCloud Drive 客户端的跨电脑往返验证。

## 从源码构建

需要 Windows x64、[Rust MSVC 工具链](https://www.rust-lang.org/tools/install)，以及 Visual Studio C++ Build Tools 和 Windows SDK。仓库通过 `rust-toolchain.toml` 固定 Rust 1.99.0，通过 `Cargo.lock` 固定依赖；Windows SDK 的 `rc.exe` 用于嵌入图标和清单。

在仓库根目录运行：

```powershell
cargo fmt --all --check
cargo fetch --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --release --all-targets -- --test-threads=1
cargo build --locked --release --bin pasteleaf
python scripts/package.py
python scripts/package.py --verify dist/pasteleaf-0.1.0-windows-x64.zip
```

构建产物为 `target/release/pasteleaf.exe`，静态链接 C 运行库。运行时无需 Rust 或开发 SDK。

打包需要 Python 3.11 或更新版本，只使用 Python 标准库。它从本地 Cargo 缓存离线收集锁定依赖的许可，生成便携 ZIP、包内 `SHA256SUMS` 和 ZIP 的 `.sha256` 文件。`cargo fetch --locked` 先准备锁定依赖缓存；许可缓存缺失时打包会失败，不会联网替换许可。CI 在 `windows-latest` 上执行同样的检查并上传便携包；固定工具链与依赖可复现构建步骤，但不承诺不同 Windows SDK 下生成的二进制逐字节一致。

## 开发验证

原生测试使用暂停记录的独立临时数据目录；串行运行可避免 Windows 消息和快捷键测试相互干扰。测试覆盖手动收藏、标签、多行编辑、保存失败保留草稿、菜单行为、窗口边框、存储边界和加密同步。同步使用两个独立设备状态及合成内容验证，真实跨电脑往返仍待验证。

手动验证也应使用隔离目录，例如：

```powershell
$env:PASTELEAF_DATA_DIR = Join-Path $env:TEMP "pasteleaf-demo"
cargo run --locked --example seed_preview -- $env:PASTELEAF_DATA_DIR
cargo run --locked --bin pasteleaf
```

`seed_preview` 会生成暂停记录的演示库；请选择空的隔离目录。`paste_receiver` 示例用于验证粘贴，`hold_hotkey` 用于验证快捷键冲突。重绘图标需要额外安装 Pillow，再运行 `python scripts/generate-icon.py`。

## 许可

PasteLeaf 源码与原创图标采用 [MIT 许可](LICENSE)。第三方依赖保留各自许可，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 及便携包中的 `third-party/` 目录。
