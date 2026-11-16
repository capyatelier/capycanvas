# File thumbnails for drawings

[Design history](README.md)

> Research record: how `.capy` drawings can show their artwork in file managers
> and open dialogs on each host, and what each route costs. The sources were
> current in November 2026; recheck the [open questions](#revisit-when) first.

No portable mechanism exists. Every host draws file thumbnails from its own
extension point, and every open dialog uses the same source as the host's file
manager: no picker lets an application supply thumbnails for the files it is
browsing. The format needs no change. The optional `preview.png` member
([package contract](../reference/capy-package.md#preview-and-file-outcomes)) is
an sRGB PNG of at most 512 pixels per side, so a thumbnailer only locates that
member through the ZIP central directory, applies the contract's preview checks
and returns the PNG. It never decodes artwork, renders or touches the GPU. One
reader in `layer-core::package`, wrapped thinly per host, serves every route
below.

## By host

| Host | Extension point | File manager | Open dialog |
| --- | --- | --- | --- |
| Windows | `IThumbnailProvider` COM DLL | Yes | Yes, and the picker must request thumbnail view |
| macOS | Quick Look Thumbnail Extension | Yes | Yes |
| iOS, iPadOS | Quick Look Thumbnail Extension | Yes (Files) | Yes |
| Linux, native package | `.thumbnailer` file plus a command | GNOME, Xfce, KDE | GNOME and KDE; not other desktops |
| Linux, Flatpak | None | No | No |
| Android | Only the app's own `DocumentsProvider` | No | Only inside that provider's location |
| Web | The OS dialog | n/a | Where the native app's extension is installed |

### Windows

Explorer and the open dialog use a thumbnail handler: an in-process COM server
implementing `IThumbnailProvider` and `IInitializeWithStream`, which Windows
isolates in a surrogate process by default
([thumbnail providers](https://learn.microsoft.com/windows/win32/shell/thumbnail-providers),
[building handlers](https://learn.microsoft.com/en-ie/Windows/Win32/shell/building-thumbnail-providers)).

- **Registration.** The installer can register the handler per user without
  elevation, as [blp-thumb-win-rs](https://github.com/WarRaft/blp-thumb-win-rs)
  does. The MSIX declares it with
  [`desktop2:ThumbnailHandler`](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-desktop2-thumbnailhandler)
  under its existing file type association and a COM surrogate server. The
  portable ZIP registers nothing.
- **Rust.** `windows-rs` implements the interfaces; [CBXShell-rs](https://github.com/Clickin/CBXShell-rs)
  and the [Stirling-PDF handler](https://github.com/Stirling-Tools/Stirling-PDF/tree/main/frontend/src-tauri/thumbnail-handler)
  are working examples; the latter catches panics so a bad file cannot crash
  Explorer.
- **ARM64.** Explorer and its surrogate are native ARM64, so the ARM64 package
  needs a native ARM64 DLL, not ARM64EC ([Windows guide](../development/windows.md)).
- **Open dialog.** `apps/layer-windows/DocumentView.cpp` uses the Windows App
  SDK `FileOpenPicker` without setting `ViewMode`, which defaults to `List`, where
  no file shows a thumbnail. Opening drawings should set
  [`PickerViewMode::Thumbnail`](https://learn.microsoft.com/windows/apps/develop/files/using-file-folder-pickers).

### macOS, iOS and iPadOS

A Thumbnail Extension (`QLThumbnailProvider`) lists `art.capycanvas.project`,
which both `Info.plist` files already export, in its supported content types
([QuickLookThumbnailing](https://developer.apple.com/documentation/quicklookthumbnailing)).
Finder, the Files app, `NSOpenPanel` and `UIDocumentPickerViewController` all
use it. Each platform needs its own extension target.

- **Only route.** macOS 15 no longer loads `.qlgenerator` plugins
  ([Eclectic Light](https://eclecticlight.co/2024/10/31/how-sequoia-has-changed-quicklook-and-its-thumbnails/),
  [Krita bug 493474](https://bugs.kde.org/show_bug.cgi?id=493474)). The
  `QuickLook/Thumbnail.png` convention applies only to directory packages, and a
  `.capy` drawing is a single file.
- **No in-panel preview.** The macOS app is sandboxed
  (`apps/layer-apple/macOS/App/CapyCanvas.entitlements`), so `NSOpenPanel` runs
  out of process and the app cannot read a file before the person confirms it.
  An accessory view cannot preview the selection.
- **Testing.** The Files app keeps stale thumbnails, and an iOS 16 report
  describes per-file thumbnails failing for custom types
  ([FB11255032](https://openradar.appspot.com/FB11255032)).

### Linux

A `.thumbnailer` file in `/usr/share/thumbnailers` names a command and the MIME
types it handles. Nautilus runs it in a Bubblewrap sandbox that mounts only
`/usr`, so the command must be installed there
([3mfthumb notes](https://github.com/themanyone/3mfthumb)). Thunar's Tumbler reads
the same files ([Tumbler](https://docs.xfce.org/xfce/thunar/4.14/tumbler)), and
KIO falls back to them for MIME types without a KDE plugin since KDE Frameworks
6.6 ([KIO thumbnailer support](https://planet.kde.org/akseli-lahtinen-aks-2024-08-08-kio-thumbnailer-support)).

`gtk::FileDialog` uses the file chooser portal by default in GTK 4.22, so the
dialog depends on the desktop:

| Portal backend | Dialog | Thumbnails |
| --- | --- | --- |
| GNOME 47 and later | Nautilus | Installed thumbnailers and valid cached thumbnails ([niri #3765](https://github.com/niri-wm/niri/issues/3765)) |
| KDE | KDE file dialog | KIO plugins and `.thumbnailer` files; loading is unreliable ([KDE bug 464143](https://bugs.kde.org/show_bug.cgi?id=464143)) |
| `xdg-desktop-portal-gtk` | GTK 3 chooser | No thumbnail view |
| None (in-process GTK 4 chooser) | GTK 4 chooser | Only thumbnails already in the shared cache; it never generates them ([`gtkfilethumbnail.c`](https://gitlab.gnome.org/GNOME/gtk/-/raw/main/gtk/gtkfilethumbnail.c)) |

- **Flatpak cannot export thumbnailers.** Flatpak exports only applications,
  icons, D-Bus services, search providers, KRunner plugins, MIME packages and
  metainfo. Exporting thumbnailers is [flatpak#4923](https://github.com/flatpak/flatpak/issues/4923),
  open since 2022; its latest direction is
  [XDG intents](https://blog.sebastianwick.net/posts/xdg-intents-update/),
  which has not shipped.
- **Separate host package.** Flatpak users get thumbnails only from a small
  native package that installs the `.thumbnailer` file and its command under
  `/usr`. `packaging/arch/capycanvas-git` can include both directly.
- **Rejected: writing the cache on save.** Nautilus shows any cached thumbnail
  whose stored modification time matches, even without a thumbnailer. Writing
  `~/.cache/thumbnails` from the Flatpak needs the file's host path, and the
  Documents portal's [`GetHostPaths`](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Documents.html)
  is unavailable inside the sandbox. It would also need home and cache
  filesystem access, cover only files saved on that computer, and KDE ignores
  it ([KDE bug 402721](https://bugs.kde.org/show_bug.cgi?id=402721)).
- **MIME detection prerequisite.** Thumbnailers match by MIME type.
  `apps/layer-linux/art.capycanvas.CapyCanvas.xml` matches `CAPY` at offset 0,
  but packages begin with `PK\003\004`, with `mimetype` at offset 30 and
  `application/vnd.capycanvas` at offset 38, so detection relies on the `*.capy`
  glob alone. An OpenRaster-style nested match fixes renamed files.

### Android

The system picker (`ACTION_OPEN_DOCUMENT` in
`apps/layer-android/app/src/main/java/art/capycanvas/Documents.kt`) asks each
document's provider for thumbnails. The local storage provider offers them only
for `image/*` types
([`FileSystemProvider`](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/core/java/com/android/internal/content/FileSystemProvider.java)),
and no other hook exists. The app could publish its own
[`DocumentsProvider`](https://developer.android.com/guide/topics/providers/create-document-provider)
location whose drawings set `FLAG_SUPPORTS_THUMBNAIL` and serve `preview.png`,
and open the picker there with `EXTRA_INITIAL_URI`. It covers only drawings
stored through that provider, while drawings are now saved wherever the person
chooses.

### Web

Browsers show the OS dialog, so Web users see thumbnails only where the native
app's extension is installed. On Linux, Chromium uses the portal when present
([portal dialog](https://chromium.googlesource.com/chromium/src/+/main/ui/shell_dialogs/select_file_dialog_linux_portal.cc));
Firefox outside Flatpak uses the GTK 3 chooser unless configured otherwise
([Firefox setting](https://support.mozilla.org/questions/1589917)).

## Recommendation

1. Add the shared `preview.png` reader with bounded, panic-free failure.
2. Ship the Windows handler with the installer and MSIX, and request thumbnail
   view when opening drawings.
3. Ship the macOS and iOS Thumbnail Extensions.
4. Add the Linux thumbnailer to native packages and correct the MIME magic.
   Offer Flatpak users the separate host package.
5. Leave Android until a `DocumentsProvider` is wanted for its own sake.

An in-app open screen that draws recent drawings from their `preview.png`
would cover every host, including Android and Flatpak, but it is a separate
feature decision.

## Revisit when

- Flatpak ships thumbnailer export or an XDG thumbnailer intent: the Flatpak
  route opens and the host package becomes unnecessary.
- The Windows App SDK picker is checked on a VM: confirm the open dialog honours
  `ViewMode`.
- KIO's unreliable dialog thumbnails are fixed upstream.
- Android adds a thumbnail hook for non-media files in its storage providers.
