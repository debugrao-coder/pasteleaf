# PasteLeaf desktop behavior contract

Status: implementing · Authority: approved plan · Last reviewed: 2026-10-11

- All content actions are committed by the shared Rust core before success is presented. UI previews cannot masquerade as saved state.
- New installs require an explicit recording start. Once enabled there are no automatic sensitive-app exclusions; manual exclusions and pause are available. Native clipboard restrictions are never bypassed.
- Global hotkeys restore the original target. Without permission, provide copy/manual-paste fallback. Never synthesize Return. Sequential paste advances only on a user action.
- Mac shortcut changes reserve the proposed combination before persisting, then release the old reservation only after save succeeds. Conflicts or storage failures retain the previous shortcut. Escape, focus loss, switching settings tabs and closing settings cancel only the active recorder, not unrelated work. Settings is an independent window; it cannot enlarge or relocate the bottom shelf.
- Local-only clear is different from replicated delete. User confirmation states which scope is affected; old encrypted operation history is not automatically erased.
- History is permanent until a user action. Quota exhaustion pauses new writes; no silent eviction. An unavailable attachment cannot be pasted as a nonexistent source path.
- Search/category filters, selected item IDs and scroll position survive updates. IME composition must not trigger shortcuts, submit or premature filtering.
- Clipboard writes by this client and remote imports must not become new captured entries. File capture copies bytes; paths exposed to UI are local only.
- Sync setup names the selected directory, all-record scope and unencrypted-local/encrypted-transfer distinction. Setup/connection cancellation does not activate sync. Never show passwords in logs/toasts.
- V2 uses a separate sync library. V1 is read-only migration input with backup and identity preservation. No bidirectional mirror into an old v1 library.
- Conflicts remain visible until explicitly resolved; retry does not duplicate operations. Partial cloud downloads wait rather than corrupt a valid local item.
- UI Automation/VoiceOver expose meaningful names, selected/disabled state and errors. Every drag action has a keyboard/menu alternative.

## Verification boundaries

Synthetic in-process tests validate core behavior, not real cloud arrival. A Windows cross-build validates compilation/linking, not rendering, paste targets or accessibility. Actual iCloud verification requires Mac and Windows with a dedicated synthetic library and observed peer receipts. Until then, replacement readiness is HOLD.

## Canonical UI Map

Native desktop adapters deliberately replace browser primitives. Search/filter state is transient and potentially sensitive: it is not put into URLs or telemetry. OS-owned select menus and scrollbars are accepted; native language and keyboard conventions apply. No calendar or date input is in this release.

| Capability | Canonical owner | Source of truth | Allowed variants | Verification |
|---|---|---|---|---|
| Table Selection | Mac `AppModel`; Windows `ViewModel` | Selected stable IDs and user-driven queue contract | Loaded-card selection, additive/range selection | Native model tests; real keyboard checks |
| Select/Listbox | SwiftUI Picker/Menu; Win32 native controls | Shared domain values plus OS interaction | Native theme and category menus | Mac popup inspection; Windows runtime HOLD |
| Form | Mac `SettingsView`/`DetailViews`; Windows screen builder | Core API validation, explicit masked secret fields | Settings, category/tag editor, sync setup | Invalid-input tests; native field/focus review |
| Scrollbar | SwiftUI ScrollView and Win32 shelf navigation | DESIGN.md and OS accessibility settings | Horizontal shelf; vertical long details | No hidden overflow; keyboard/scale matrix |
| Toast | `AppModel.announce/fail`; Win32 status region | Saved locally is not delivered to peer | Transient acknowledgement; persistent error | No modal error flood; text besides color |
| CRUD | Shared `pasteleaf-core::Engine` | SQLite transaction, immutable sync operation | Capture, favorite/tag edit, replicated delete, local-only hide | Core regression suite; UI success/failure checks |

`premium-ui.json` and the premium static audit check declarations and selected source patterns, not native runtime accessibility. The `crudFullFlow` evidence is the shared-core integration suite, not a claim of Windows screen acceptance.
