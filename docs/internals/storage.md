# Where the app keeps its files

[Editor internals](README.md)

Every client sorts what it stores into five kinds, and each platform gives each
kind its own folder. Shared Rust names the stores inside those folders, in
[`StorageRoots`](../../crates/layer-host/src/storage.rs). Artwork the painter
saves goes wherever they choose, never into these folders.

## Kinds of files

| Kind | Stores | Lifetime |
| --- | --- | --- |
| Config | Preferences (`settings.json`), export presets | Kept; backed up. Reading never fails; unreadable fields keep their defaults. |
| Data | Workspaces, toolbars, palettes and tool memory (`workspaces/`), imported color profiles (`color-profiles/`), imported papers (`papers/`) | Kept; backed up. Never deleted automatically. |
| State | Restartable editing sessions, including drawings never saved to a file (`sessions/`); GTK's last folder per file dialog (`file-dialogs/`) | Kept on this device; not backed up. |
| Cache | The latest copied image other apps paste from (`clipboard/`) | Regenerated or replaced; the platform may clear it. |
| Temp | Copies of opened `.capy` files, parked drawing tabs, pasted images being read | Gone when the app stops. |

Temporary files are anonymous: a file is created in the temp folder and loses
its name as soon as it is open (unlinked on Unix, `DELETE_ON_CLOSE` on
Windows), so a crash leaves nothing behind. `layer_core::temp_files` creates
them; each host sets the folder once at startup, and opening a `.capy` file
fails if it has not. The few temporary files that need a name, such as a pasted
image a decoder reads, are removed when their reader finishes. GTK, Android and Windows also empty the
temp folder when the process that owns the installation starts; Apple clears the
app's `tmp` itself.

The workspace store holds the painter's palettes and custom workspaces. Startup
replaces it only when this build cannot read it, and keeps a copy of the old
store first; see [workspace ownership](workspace-ownership.md#startup-always-adopts-a-workspace).

Native workspace connections enable SQLite defensive mode and disable trusted
schema before reading stored records. SQLite limits values and complete rows to
the 128 MiB workspace package size plus 64 KiB for row identity and framing, and
SQL text to 64 KiB.

External workspace packages remain untrusted. Packages are limited to 128 MiB,
and prepared writes are validated before they enter storage. Stored component
graphs retain their reference hash, depth and schema checks, and schema
inspection preserves newer versions before allowing a reset. Imported ICC and
effect source bytes remain untrusted when interpreted, even after storage in an
app-owned container. External photo decoding on Linux, Windows and Android uses the
confined decoder described in the [architecture](../architecture.md#files-and-other-host-services);
its temporary input, output and diagnostics are anonymous files in the existing
temp folder. Managed paper reloads continue to use their local PNG reader.

Shader pipelines remain in process and in the driver's own caches. The app does
not load or save opaque driver pipeline-cache blobs from its storage folders.

The paper library keeps each imported paper as `<id>.png`, a gray PNG named by
the SHA-256 of its samples, with its picker preview in `<id>.preview` and names
and order in `index.json`. PNGs and the index use the shared native atomic writer
([`papers.rs`](../../crates/layer-host/src/papers.rs)). A lost preview is
rebuilt from its PNG. Drawings embed the papers they use, so deleting a paper
changes no drawing. Paper images and the index use the shared native atomic
publisher, which flushes a private sibling before replacing the destination.

## Folders on each platform

| Kind | Linux | Windows installer | Windows Store | Android | macOS and iPadOS | Web |
| --- | --- | --- | --- | --- | --- | --- |
| Config | `$XDG_CONFIG_HOME/capycanvas` | `%APPDATA%\CapyAtelier\CapyCanvas` | Package `LocalState` | `filesDir`; settings in `SharedPreferences` | `Application Support/<bundle id>` | `localStorage` (preferences) and IndexedDB `capy-color-preferences` |
| Data | `$XDG_DATA_HOME/capycanvas` | `%LOCALAPPDATA%\CapyAtelier\CapyCanvas` | Package `LocalState` | `filesDir` | `Application Support/<bundle id>` | IndexedDB `capycanvas.workspaces`, `capy-color-preferences` |
| State | `$XDG_STATE_HOME/capycanvas` | `%LOCALAPPDATA%\CapyAtelier\CapyCanvas` | Package `LocalState` | `noBackupFilesDir` | `Application Support/<bundle id>/State`, excluded from backup | IndexedDB `capy-session-restart` |
| Cache | `$XDG_CACHE_HOME/capycanvas` | `%LOCALAPPDATA%\CapyAtelier\CapyCanvas\Cache` | Package `LocalCache` | `cacheDir` | `Caches/<bundle id>` | Service worker cache |
| Temp | `$XDG_CACHE_HOME/capycanvas/temp` | `%TEMP%\CapyCanvas` | Package `TempState` | `cacheDir/temp` | The app's `tmp` | Origin private file system |

Hosts find these folders through the platform's API (GLib, Known Folders or
`ApplicationData`, `Context`, `FileManager`), never by reading environment
variables themselves. Under Flatpak the same GLib calls resolve to
`~/.var/app/<app id>/`. Uninstalling a Windows Store, Android or Apple app
deletes its folders; the Windows installer and Linux packages leave them.

The web client asks the browser to keep its storage when it first stores a
changed drawing that has never been saved to a file, and keeps the leave warning
while such a drawing is open and the browser has not agreed; see
[Web](../development/web.md).

## Development builds

Only packaged release builds use the release identity. Every other build has
its own, so it never opens an installed app's files or windows:

| Platform | Release | Development |
| --- | --- | --- |
| Linux | `art.capycanvas.CapyCanvas`, folders `capycanvas` | `art.capycanvas.CapyCanvas.Devel`, folders `capycanvas-devel` |
| Windows | `CapyCanvas` folders, or the Store package | `CapyCanvas-Dev` folders |
| Android | `art.capycanvas.editor` | `art.capycanvas.dev` |
| macOS and iPadOS | `art.capycanvas.CapyCanvas` | `art.capycanvas.CapyCanvas.dev` |

The Linux and Windows host crates select the release identity with their
`release-identity` Cargo feature, which only the packaging scripts enable.

## Test storage

`CAPY_STORAGE_DIR` replaces every platform folder with subfolders of one private
folder: `config`, `data`, `state`, `cache` and `temp`. A relative name is a folder
inside the platform's temporary folder, so tests of sandboxed apps can name one
without knowing the app's container. Tests, fixtures and runners use it; nothing
else selects storage. GTK test builds store nothing
without it, and use its workspaces and sessions only when the test has created
`data/workspaces` or `state/sessions`.

## Adding a store

Decide which kind of file it is from the table above, name it in
`StorageRoots`, and add it to this page. User-created content belongs in data,
so that it is backed up and never discarded with a cache or a session.
