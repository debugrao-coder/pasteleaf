# PasteLeaf v2 · Windows native client

Windows 10/11 x64 client, written in Rust with a Win32 host, Direct2D rendering and DirectWrite typography. It shares the v2 SQLite/encryption/synchronization core with the Mac client. No browser runtime is embedded.

## Build

Recommended: a Visual Studio Developer PowerShell with Rust MSVC and the Windows SDK (`rc.exe` on PATH):

```powershell
cargo build -p pasteleaf-windows --release --target x86_64-pc-windows-msvc
cargo test -p pasteleaf-windows --target x86_64-pc-windows-msvc
```

The icon and Per-Monitor-v2 application manifest are embedded by `build.rs`. GNU cross-builds require MinGW-w64 GCC, AR and windres. A successful cross-build proves compilation/linking, not Windows runtime behavior.

## Isolated visual verification

```powershell
.\pasteleaf-windows.exe --demo --theme light
.\pasteleaf-windows.exe --demo --theme dark
powershell -ExecutionPolicy Bypass -File .\clients\windows\scripts\verify-ui.ps1 -Exe .\pasteleaf-windows.exe -Theme light
```

Each demo uses a fresh temporary v2 database and synthetic records. It does not register a global hotkey, read or write the system clipboard, import existing history, join iCloud, or change login startup. Do not substitute demo evidence for real clipboard/synchronization tests. The UI verification script creates actual screenshots and an explicitly scoped result only when run on an interactive Windows desktop.

## Daily-use behavior

- Default data: `%LOCALAPPDATA%\PasteLeaf\v2`. `--data-dir PATH` selects an isolated alternate library. A named mutex prevents duplicate instances using one data directory.
- A fresh installation is paused until **开始记录** is explicitly selected. The UI warns that passwords and tokens can be recorded. Excluded applications are opt-in process filenames such as `example.exe`.
- `Ctrl+Q` toggles the bottom panel. Configure a modifier plus letter, number or F1–F24 in settings. Conflicts are reported without taking another app's registration.
- Native BUTTON/EDIT/STATIC controls retain standard UI Automation providers. Cards and actions are owner-drawn with the shared design tokens; focus and hover remain visible. Search is IME-aware and debounced; stale responses are discarded.
- Arrow keys select, Space previews, Enter pastes and Ctrl+Enter pastes plain text. Ctrl-click performs multiselection. Text selections join with newlines; file selections form one file drop. Mixed selections form a user-driven queue: press the configured shortcut plus Shift, or select **下一项**, once per item. The client never sends Return.
- Before each clipboard write, the worker fetches every selected record through core `get`, validating attachment hashes and rejecting deleted/unavailable records. Queue items are revalidated separately and retained for retry when a write fails. Input rejected by UIPI or failed target restoration remains copied for manual paste; successful input dispatch is not proof that the target consumed it.
- Text, CF_HTML, base64-preserved RTF, PNG/JPEG images and ordinary file contents are supported. Images are limited to 16 MP, 16,384 pixels per side and a 128 MiB decode allocation. Files retain their bytes, not source-machine paths. Reparse points, directories and unavailable cloud files are rejected by the shared core.
- Native forms cover settings, categories, tags, content preview, synchronization, conflict comparison/resolution, destructive confirmations and quota protection. File/folder selection uses the system picker. Short displays have a vertical form scrollbar; the card strip has bounded 64-record paging.
- Sync status distinguishes local publication from peer import. Visible history quietly refreshes for remote imports without replacing the system clipboard. Encrypted iCloud Drive transport latency remains dependent on iCloud.

## Verification boundaries

Portable presentation/core-worker/image tests can run on macOS. Native Direct2D rendering, 100/150/200% DPI, IME, UI Automation interaction, elevated-target paste rejection, login startup, file-drop interoperability and real Windows ↔ Mac iCloud round trips still require Windows runtime verification. Screenshots alone do not establish accessibility or complete visual acceptance.

Windows uses the opaque compatibility surface; a native translucent compositor path is not yet delivered. Cards can be dragged to visible collection tabs after the system drag threshold; Escape or capture loss cancels, and a drop never also clicks/pastes. Category movement also has menu/keyboard controls. Category-tab ordering is not part of the v2 protocol. Do not claim final Paste replacement or pixel-perfect acceptance until those gaps and the runtime checklist are closed.
