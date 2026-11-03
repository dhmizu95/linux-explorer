# Linux Explorer — Feature Specification

Goal: a file manager for Fedora and Ubuntu that **looks, behaves and feels like Windows 11 File Explorer**,
written in Rust.

## How the spec was derived (clean-room behavioural reverse engineering)

`explorer.exe` was **not** disassembled or decompiled. Its licence forbids it, and it would not help anyway:
the "feel" lives in observable layout and behaviour, not in Win32 shell internals that don't exist on Linux.
The reference was captured from a live Windows 11 (build 26200) Explorer window instead:

| Tool / source | What it gave us |
|---|---|
| **UI Automation tree dump** (`System.Windows.Automation`, `docs/re/uia_dump.ps1`) | Exact widget hierarchy, automation IDs and pixel geometry: tab strip (48 px), address bar + breadcrumb (40 px) with per-segment chevron split buttons, search box (`FileExplorerSearchBox`), nav pane `SysTreeView32` (~287 px), Details header (32 px) with `System.ItemNameDisplay / DateModified / ItemTypeText / Size` columns and per-column filter drop-downs, row pitch 37 px, status bar (31 px) with item count + `ViewMode_Details` / `ViewMode_LargeIcons` toggles |
| **Registry** `HKCU\…\Explorer\Advanced` | The full set of Folder Options (Hidden, HideFileExt, ShowSuperHidden, AutoCheckSelect, LaunchTo, ShowStatusBar, ShowInfoTip, ShowTypeOverlay, IconsOnly, HideDrivesWithNoMedia, SeparateProcess, …) |
| Documented keyboard shortcuts, AQS search syntax and grouping buckets | Shortcut map, search filters, the "Today / Yesterday / Earlier this week …" and "Tiny / Small / Medium …" group names |

Each Windows concept is then mapped to its Linux/freedesktop equivalent (Recycle Bin → XDG Trash spec,
drives → `/proc/self/mountinfo` + `statvfs`, file associations → `.desktop` + `mimeapps.list`, clipboard →
`x-special/gnome-copied-files`, Quick access → own pins + GTK bookmarks, Recent → `recently-used.xbel`).

Legend: ✅ implemented & tested · 🟡 partial · ⬜ not yet

---

## 1. Window chrome & layout
| # | Feature | Linux mapping | Status |
|---|---|---|---|
| 1.1 | Tabs: open, close, middle-click close, "+" new tab, Ctrl+T/Ctrl+W/Ctrl+Tab/Ctrl+Shift+Tab, Ctrl+1..9, tab context menu (close others, duplicate), drop files on a tab | — | ✅ |
| 1.1b | Drag tabs to reorder or tear off into a new window | — | ⬜ |
| 1.2 | Back / Forward / Up / Refresh buttons; back/forward history drop-down (recent locations) | — | ✅ |
| 1.3 | Breadcrumb address bar: clickable segments, chevron per segment lists sub-folders, click empty area → editable text path, autocomplete of folder names | `~`, `$VAR`, `file://` URIs accepted | ✅ |
| 1.4 | Search box "Search <folder>" — live recursive search of current folder | own walker thread | ✅ |
| 1.5 | Command bar: New ▾, Cut, Copy, Paste, Rename, Share(n/a), Delete, Sort ▾, View ▾, Filter, "…" overflow (Undo, Select all/none/invert, Properties, Options, Open in Terminal) | — | ✅ |
| 1.6 | Navigation pane (resizable, lazy folder tree): Home, pinned folders, This PC → drives, Network, Recycle Bin, File System root | XDG user dirs, mounts | ✅ |
| 1.7 | Preview pane (Alt+P): images, text/code, folder summary | — | ✅ |
| 1.8 | Details pane (Alt+Shift+P): icon, name, type, size, dates, attributes | — | ✅ |
| 1.9 | Status bar: "N items", "M items selected  X MB", view toggles bottom-right | — | ✅ |
| 1.10 | Light / Dark theme following the system, Win11 palette (accent blue, `#CCE8FF` selection) | `gsettings color-scheme` / `GTK_THEME` | ✅ |
| 1.11 | Compact view (reduced row spacing) | — | ✅ |
| 1.12 | Window title = current folder name; remembers window size | — | ✅ |
| 1.13 | Mica/acrylic translucency | compositor-dependent | ⬜ |

## 2. Views
| # | Feature | Status |
|---|---|---|
| 2.1 | Extra large / Large / Medium / Small icons, List, Details, Tiles, Content (Ctrl+Shift+1…8) | ✅ |
| 2.2 | Ctrl + mouse-wheel zoom cycles view sizes | ✅ |
| 2.3 | Details columns: Name, Date modified, Type, Size (bin: Original Location, Date Deleted; search: Folder); drag to resize, double-click grip to auto-size, click to sort with the Win11 chevron | ✅ |
| 2.3b | Column chooser (Date created, Owner, Permissions…) via header right-click | ⬜ |
| 2.4 | Sort by Name / Date modified / Type / Size / Date created, Ascending/Descending; folders first (folders last when descending — Explorer behaviour) | ✅ |
| 2.5 | Natural ("logical") name ordering like `StrCmpLogicalW` (file2 < file10, case-insensitive) | ✅ |
| 2.6 | Group by Name (A–H, I–P, Q–Z, 0–9, Other), Type, Date modified (Today, Yesterday, Earlier this week, Last week, Earlier this month, Last month, Earlier this year, A long time ago), Size (Empty, Tiny, Small, Medium, Large, Huge, Gigantic), collapsible groups | ✅ |
| 2.7 | Image thumbnails in icon views (background decoding, cache) | ✅ |
| 2.8 | Per-folder view memory (view mode/sort/group remembered per path) | ✅ |
| 2.9 | Item check boxes (Folder option) | ✅ |
| 2.10 | Show/hide file name extensions, hidden items (dot-files) | ✅ |
| 2.11 | Column filter drop-downs on header (filter by type / date bucket / size bucket) | 🟡 (Type filter via "Filter" menu) |
| 2.12 | Video/PDF thumbnails | ⬜ (needs ffmpeg/poppler) |
| 2.13 | "Home" page: Quick access tiles + Recent files list | ✅ |
| 2.14 | "This PC" page: user folders + Devices and drives with capacity bars (red > 90 %) | ✅ |
| 2.15 | Recycle Bin page: Name, Original location, Date deleted, Size; Restore, Empty | ✅ |
| 2.16 | Empty-folder message "This folder is empty." | ✅ |

## 3. Selection & keyboard
| # | Feature | Status |
|---|---|---|
| 3.1 | Click, Ctrl+click toggle, Shift+click range, Ctrl+Shift range-add | ✅ |
| 3.2 | Rubber-band (marquee) selection in empty space, Ctrl to add | ✅ |
| 3.3 | Arrow keys / Home / End / PgUp / PgDn move focus; Shift extends; grid-aware in icon views | ✅ |
| 3.4 | Type-ahead: typing letters jumps to the next matching item | ✅ |
| 3.5 | Ctrl+A select all, Select none, Invert selection | ✅ |
| 3.6 | Enter open, Backspace / Alt+← back, Alt+→ forward, Alt+↑ up | ✅ |
| 3.7 | F2 rename (name selected without extension), Tab/Shift+Tab to rename next/previous | ✅ |
| 3.8 | F3 / Ctrl+F / Ctrl+E search; Alt+D / Ctrl+L / F4 address bar; F5 refresh; F11 full screen (F6 jumps to the address bar rather than cycling all panes) | 🟡 |
| 3.9 | Ctrl+N new window; Ctrl+Shift+N new folder; Alt+Enter properties; Shift+F10 context menu (the Menu key is not exposed by egui) | 🟡 |
| 3.10 | Ctrl+Shift+C copy as path; Ctrl+Shift+E expand nav pane to current folder | ✅ |

## 4. File operations
| # | Feature | Linux mapping | Status |
|---|---|---|---|
| 4.1 | New folder ("New folder", "New folder (2)" …) then inline rename | — | ✅ |
| 4.2 | New ▸ Text Document / empty file / templates from `~/Templates` | XDG_TEMPLATES_DIR | ✅ |
| 4.3 | Rename inline with validation (empty, `/`, duplicates) and extension-change warning | — | ✅ |
| 4.4 | Cut / Copy / Paste, paste into selected folder, "- Copy" naming when pasting into same folder | — | ✅ |
| 4.5 | System clipboard interop with Nautilus/Dolphin/Nemo | `x-special/gnome-copied-files`, `text/uri-list` via `wl-copy`/`xclip` | ✅ |
| 4.6 | Delete → Recycle Bin (no prompt, Win11 default) ; Shift+Delete → permanent delete with confirmation | XDG Trash (`trash` crate) | ✅ |
| 4.7 | Background copy/move/delete with progress dialog: % bar, items, bytes, speed, current file, Pause / Cancel | worker threads | ✅ |
| 4.8 | Conflict dialog "Replace or Skip Files": Replace, Skip, Keep both, "Do this for all conflicts" | — | ✅ |
| 4.9 | Move across file systems (copy + delete), preserves permissions, mtime, symlinks | — | ✅ |
| 4.10 | Undo / Redo (Ctrl+Z / Ctrl+Y) for rename, move, copy, new item, delete-to-bin | — | ✅ |
| 4.11 | Drag & drop inside the window (onto folders, nav pane, breadcrumb, tabs); Ctrl = copy, Shift = move; default move on same FS, copy across FS | — | ✅ |
| 4.12 | Drop files from other apps into the window | winit file-drop (X11/XWayland; winit has no Wayland file drop yet) | 🟡 |
| 4.13 | Drag files out to other apps | needs XDND/Wayland DnD source | ⬜ |
| 4.14 | Compress to ZIP file; Extract All (zip native, tar.* via `tar`) | — | ✅ |
| 4.15 | Create shortcut | symlink | ✅ |

## 5. Open / integration
| # | Feature | Linux mapping | Status |
|---|---|---|---|
| 5.1 | Double-click opens with default app | `mimeapps.list` + `.desktop`, fallback `xdg-open` / `gio open` | ✅ |
| 5.2 | Open with ▸ list of apps for the MIME type, "Choose another app" | `/usr/share/applications`, `~/.local/share/applications`, `mimeinfo.cache` | ✅ |
| 5.3 | Run executables / scripts (asks before executing) | exec bit | ✅ |
| 5.4 | Open in Terminal | ptyxis, gnome-terminal, kgx, konsole, xfce4-terminal, tilix, alacritty, kitty, wezterm, xterm | ✅ |
| 5.5 | Pin to / Unpin from Quick access | own JSON + GTK bookmarks import | ✅ |
| 5.6 | Recent files on Home | `~/.local/share/recently-used.xbel` | ✅ |
| 5.7 | Unmount / eject removable drives from This PC and the nav pane | `udisksctl` | 🟡 (unmounted volumes are not listed for mounting yet) |
| 5.8 | Network locations (SMB/SFTP URIs) | gvfs mounts under `/run/user/$UID/gvfs` listed; connect via `gio mount` | 🟡 |
| 5.9 | Auto-refresh when folder changes on disk | stat polling (1 s) | ✅ |
| 5.10 | Set as default file manager | `.desktop` with `inode/directory` MIME | ✅ |
| 5.11 | Shell extensions / context-menu handlers from other apps | — | ⬜ |
| 5.12 | OneDrive / cloud status overlays | — | ⬜ (out of scope) |

## 6. Properties & options
| # | Feature | Status |
|---|---|---|
| 6.1 | Properties: General (type, opens with, location, size, size on disk, contains N files / M folders computed in background, created/modified/accessed, Read-only, Hidden) | ✅ |
| 6.2 | Properties: Permissions/Security (owner, group, rwx matrix for owner/group/others, apply → chmod) | ✅ |
| 6.3 | Multi-selection properties (combined size and counts) | ✅ |
| 6.4 | Drive properties (used/free/capacity, file system type, device) | ✅ |
| 6.5 | Folder Options: Open Explorer to (Home / This PC), single vs double click, show hidden, hide extensions, check boxes, show preview handlers, confirm delete, theme, compact view | ✅ |
| 6.6 | Settings persisted in `~/.config/linux-explorer/settings.json` | ✅ |

## 7. Search
| # | Feature | Status |
|---|---|---|
| 7.1 | Substring and wildcard (`*.rs`, `report??.pdf`), case-insensitive | ✅ |
| 7.2 | AQS-style filters: `kind:folder/image/video/music/document/archive`, `ext:.pdf`, `size:large` / `size:>10MB`, `modified:today/yesterday/thisweek/lastweek/thismonth/thisyear` | ✅ |
| 7.3 | Results streamed while searching, show "Location" column, cancellable | ✅ |
| 7.4 | Content (full-text) search | ⬜ (would need an indexer such as tracker/localsearch) |

## 8. Packaging, quality
| # | Feature | Status |
|---|---|---|
| 8.1 | `.desktop` file, icon, `install.sh` (user or system) | ✅ |
| 8.2 | `.deb` (cargo-deb) and `.rpm` (cargo-generate-rpm) metadata | ✅ |
| 8.3 | 63 core tests (sorting, grouping, naming, ops engine, trash, undo, search, archive, desktop entries) + 7 UI tests, including an end-to-end keyboard test that drives the real UI headlessly (egui_kittest) and a render-every-surface test that fails on missing glyphs | ✅ |
| 8.4 | Built & tested on Ubuntu 24.04 (WSL2, run with WSLg) and Fedora 45 (container: tests, release build, install.sh, desktop-file-validate) | ✅ |
| 8.5 | Headless self-screenshot mode (`--screenshot out.png`) for visual regression | ✅ |
| 8.6 | Accessibility tree (AccessKit) for screen readers | 🟡 (standard widgets are exposed; custom-painted file items are not labelled yet) |
| 8.7 | Localisation | ⬜ |
