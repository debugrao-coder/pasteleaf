# PasteLeaf

[![Windows CI](https://github.com/debugrao-coder/pasteleaf/actions/workflows/ci.yml/badge.svg)](https://github.com/debugrao-coder/pasteleaf/actions/workflows/ci.yml)

Windows 10/11 x64 剪贴板历史与收藏工具，使用 Rust、原生 Win32 和 GDI+，无需浏览器运行时。

[下载便携包](https://github.com/debugrao-coder/pasteleaf/releases/latest)

## 使用

解压便携包，双击 `pasteleaf.exe`。复制文字、链接或图片后，按 **Ctrl+Q** 呼出屏幕底部的卡片栏。首次启动若快捷键被占用，可从托盘进入设置更换。

- 左右方向键选择卡片，滚轮浏览；**Enter 或双击**粘贴到呼出前的窗口。Ctrl+1…9 快速粘贴当前页对应卡片。
- 直接输入开始搜索，Ctrl+F 聚焦搜索框；空格预览完整内容，Ctrl+C 仅复制。
- 顶部「+」新建收藏分类，支持名称和颜色；拖动卡片到分类，或通过卡片右键菜单收藏。
- 进入收藏分类后，点击「+ 新建收藏项」、按 **Ctrl+N**，或从分类右键菜单手动录入文字、链接和提示词。支持多行：Enter 换行，**Ctrl+Enter** 或「保存收藏项」保存；取消或 Esc 放弃。暂停记录时仍可添加。
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

数据默认保存在 `%LOCALAPPDATA%\PasteLeaf`：`library.json` 存文字、分类和设置，`images/` 存 PNG。数据仅保存在本机，**未加密**。备份时退出程序，复制整个数据目录。

每次保存前保留上一版有效数据 `library.previous.json`。读取失败不会覆盖原文件，写入失败会提示。最多保留 200 条未收藏历史、500 条总记录；单条文字上限 512 KiB，文字合计 64 MiB，序列化文件 70 MiB；单张图片解码后 RGBA 上限 16 MiB。收藏不会随历史自动清理。

目前支持纯文本和图片，不支持富文本、文件附件、OCR、多选连续粘贴或云同步。启动时不读取剪贴板原有内容，只记录启动后的复制事件。图片可用「图片」搜索。

粘贴前会检查目标窗口和剪贴板。权限更高的程序可能需要手动 Ctrl+V；PasteLeaf 只发送粘贴组合键，不发送回车提交内容。程序未经代码签名。

「便携包」表示无需安装；默认数据仍写入用户的本地应用数据目录。需要隔离数据时，可在启动前设置 `PASTELEAF_DATA_DIR`。

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

原生测试使用暂停记录的独立临时数据目录；串行运行可避免 Windows 消息和快捷键测试相互干扰。测试覆盖手动收藏、多行编辑、保存失败保留草稿、菜单行为、窗口边框和存储边界。

手动验证也应使用隔离目录，例如：

```powershell
$env:PASTELEAF_DATA_DIR = Join-Path $env:TEMP "pasteleaf-demo"
cargo run --locked --example seed_preview -- $env:PASTELEAF_DATA_DIR
cargo run --locked --bin pasteleaf
```

`seed_preview` 会生成暂停记录的演示库；请选择空的隔离目录。`paste_receiver` 示例用于验证粘贴，`hold_hotkey` 用于验证快捷键冲突。重绘图标需要额外安装 Pillow，再运行 `python scripts/generate-icon.py`。

## 许可

PasteLeaf 源码与原创图标采用 [MIT 许可](LICENSE)。第三方依赖保留各自许可，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) 及便携包中的 `third-party/` 目录。
