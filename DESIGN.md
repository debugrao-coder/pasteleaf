# PasteLeaf desktop design contract

Status: engineering preview; full visual acceptance HOLD · Authority: approved implementation plan and `design/tokens.json` · Last reviewed: 2026-10-11

## Product intent

A quiet native clipboard shelf, not a dashboard. Both clients share card hierarchy, action names, state meanings, and design tokens. Paste's desktop card workflow is a reference; PasteLeaf retains its own leaf identity and assets. Use synthetic content in previews and evidence.

## Visual system

`design/tokens.json` is the single machine-readable source. `python3 scripts/generate_design_tokens.py` produces Swift and Rust adapters; `--check` must fail on drift. Do not hand-edit generated adapters or maintain independent client palettes. `DESIGN.md` records intent rather than duplicating token values.

Light and dark themes map the same semantic roles. New installs follow the system. Native system typography is intentional: SF/PingFang on Mac, Segoe UI/Microsoft YaHei on Windows; monospace only for code. Mac resolves semantic colors through the generated AppKit adapter and follows the system accent; cards use readable system content surfaces with type labels and symbols, not colored header slabs. Windows retains the generated leaf palette. Cross-platform consistency means shared hierarchy, geometry, state semantics and workflows—not pixel-identical materials.

4-point rhythm; a compact shelf spans the full available width of the active monitor with no maximum-width cap or outer side gutters. It rests at the bottom of the usable work area, respecting the Dock/taskbar. Panel height, row spacing and shorter landscape cards come from the shared geometry tokens; compactness must not reduce font sizes or hide actions. Content-first cards have source/type/time, bounded preview, then metadata/actions. Long content remains available in a full preview. Motion obeys reduced-motion settings. Transparency has an opaque high-contrast fallback.

## Shared components and states

- Shelf: search + collections + sync entry above horizontal cards; preserve focus, scroll and selection when remote items arrive.
- Card: normal, hover, focused, selected, multi-selected, pending attachment, error. Images never stretch; long names truncate only in compact views and remain readable in preview.
- Search: clear action, Chinese IME-safe, keyboard submit, empty and no-results states, stale response suppression.
- Dialog: clear title, saved vs unsaved distinction, consistent cancel/save, keyboard escape, focus restoration. Destructive and privacy-changing actions name their consequences.
- Settings: appearance, recording, exclusions, shortcuts, login, local capacity, sync capacity. Password fields are masked; no secrets in diagnostic messages.
- Sync: disconnected / locked / paused / pending / published / waiting attachments / peer imported / conflict / error. “Published” means local directory handoff only, not iCloud upload success.
- Conflicts: compare versions, retain chosen version or all, no silent last-write-wins.

## Platform adapters

The v2 app icon is “stacked pages become a leaf”: an emerald folded-leaf/card symbol on a porcelain-mint glass tile. Both clients consume the same alpha-preserving master under `assets/brand/`; the asset manifest records the built-in image generation prompts and checksums. The Mac exporter performs size resampling and ICNS/ICO packaging, not independent brand drawing. The menu-bar leaf remains a monochrome system template for legibility. This is static rendered artwork, not a layered Icon Composer asset. Original v1 icon assets remain unchanged.

Mac uses SwiftUI and AppKit for focus, windows, menus and accessibility. On macOS 26+, navigation and action buttons use real Liquid Glass APIs inside `GlassEffectContainer`. The window itself uses standard under-window vibrancy; content cards, previews and form rows are not glass. Standard AppKit search, SwiftUI buttons, grouped forms and a system segmented settings selector keep platform behavior. macOS 14/15 fall back to native bordered controls and vibrancy. App/system reduced transparency or increased contrast selects opaque materials; app/system reduced motion disables app animation and interactive glass. Show Button Shapes selects explicit bordered controls. A visible checkmark supplements the selected card's system-accent ring. OS availability remains guarded at the call site; pure policy tests do not prove older-OS runtime appearance.

Mac settings uses a single independent, titled native window, not a sheet attached to the compact shelf. Opening, resizing, switching settings tabs or closing it must never change the shelf frame. Its own size and position survive reopening during the session and are clamped to the current display work area. The shortcut control records a focused key combination, displays native modifier symbols, offers a default restore, and distinguishes an unapplied draft from the active shortcut. Synthetic sessions can exercise the control but never register a system-wide shortcut.

Windows retains a Win32 host with Direct2D/DirectWrite rendering and native-accessible controls. Platform fonts, modifier labels and permissions vary; content hierarchy and domain behavior do not.

## Acceptance

Inspect real clients using synthetic records in light/dark themes and all UI states. Windows: 100/150/200% scaling. Mac: Retina, multi-display boundaries. Keyboard and screen-reader labels are required; contrast for normal text is at least 4.5:1. No clipped controls, inaccessible overflow, confusing destructive actions or false success states. A screenshot or successful build is not cross-platform runtime acceptance.
