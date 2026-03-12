# Research: Embedding WebKitGTK Runtime Libraries

Date: 2026-03-04

## Question

Can we statically link or embed WebKitGTK into the binary so users don't need system WebKitGTK installed?

## Short answer

No, not practically.

## Why static linking doesn't work

WebKitGTK has been attempted as a static build ([WebKit Bug #181695](https://bugs.webkit.org/show_bug.cgi?id=181695)), and someone did hack it to compile. But the CMake build system doesn't officially support static output ([Bug #183595](https://bugs.webkit.org/show_bug.cgi?id=183595)), and the practical problems are severe:

- **Massive dependency tree.** WebKitGTK depends on GTK, GLib, Cairo, Pango, GStreamer, libsoup, ICU, libxml2, libxslt, SQLite, and dozens more. Each has its own transitive dependencies.
- **Multi-process architecture.** WebKit spawns separate processes (WebProcess, NetworkProcess). Static linking means linking all WebKit code into all executables, multiplying binary size 3-5x.
- **Dynamic plugin loading.** GLib, GStreamer, and other dependencies use plugin architectures that fundamentally assume dynamic loading (`dlopen`).
- **License issues.** Many dependencies are LGPL, which imposes specific requirements around static linking.
- **Binary size.** The result would be hundreds of megabytes.

The [DeniseEmbeddableWebKit](https://github.com/ijsf/DeniseEmbeddableWebKit) project attempted a single-process embeddable WebKit but is now unmaintained.

## Distribution options that solve the problem differently

| Method | Self-contained? | Size | WebKitGTK handling | Trade-offs |
|--------|----------------|------|--------------------|------------|
| **.deb** | No | Small | System dependency | User must have WebKitGTK installed |
| **Flatpak** | Via shared runtime | Moderate | GNOME runtime includes it | Best practical option for desktop Linux |
| **AppImage** | Mostly | ~70+ MB | Bundled (with known bugs) | glibc version lock-in, [Tauri has bundling bugs](https://github.com/tauri-apps/tauri/issues/12463) |
| **Snap** | Yes | Large | Bundled | Ubuntu-centric, slower startup |

### Flatpak (recommended)

The GNOME 46+ runtime includes `libwebkit2gtk-4.1`, which is exactly what wry uses. Users install the Flatpak, the runtime provides WebKitGTK, and the app binary stays small. Tauri has [official Flatpak documentation](https://tauri.app/distribute/flatpak/).

Key consideration: apps install to `/app` not `/usr`, which can require path adjustments.

### AppImage

Bundles all dependencies into a single executable file. Tauri supports this but has [known issues with `libwebkit2gtkinjectedbundle.so` path resolution](https://github.com/tauri-apps/tauri/issues/12463). You must build on the oldest distro you want to support (glibc compatibility).

### Snap

Fully self-contained but Ubuntu-centric. Uses AppArmor for sandboxing, which [causes issues on Fedora/RHEL/Arch](https://www.glukhov.org/post/2025/12/snap-vs-flatpack/) (SELinux conflict). Some distros (Linux Mint) have removed Snap support.

## Alternative browser engines

| Engine | Maturity for wry | Self-contained? | Full web compat? | Size overhead |
|--------|-----------------|-----------------|-------------------|---------------|
| **CEF (cef-rs)** | Active dev, no wry integration yet | Yes | Yes (Chromium) | ~90+ MB |
| **WPE WebKit** | Not supported in wry | Same deps as WebKitGTK minus GTK | Yes | Similar |
| **Servo** | Experimental, incomplete | Potentially | No (many missing APIs) | Unknown |

### CEF (Chromium Embedded Framework)

The most promising future alternative. The [tauri-apps/cef-rs](https://github.com/tauri-apps/cef-rs) crate is actively developed (v143.3.0, 77 published versions). Integration with Tauri/wry is [being tracked](https://github.com/tauri-apps/wry/issues/1064) but not ready yet. Would add ~90+ MB (Chromium engine) but produce a truly self-contained binary.

The Tauri team has hinted CEF support may come as a separate (possibly commercial) offering, though they hope to "at least make it available on Linux for everyone."

### WPE WebKit

The [official WebKit port for embedded platforms](https://wpewebkit.org/), maintained by Igalia. No GTK dependency, smaller footprint, but wry doesn't support it and it has the same massive dependency tree minus GTK itself.

## Recommendation for this project

1. **Now:** Accept the system WebKitGTK dependency. It works, it's simple.
2. **Distribution:** Use Flatpak as the primary method. GNOME runtime provides WebKitGTK.
3. **Secondary:** Offer AppImage for users without Flatpak.
4. **Future:** Watch cef-rs for when embedded Chromium becomes viable with wry.
