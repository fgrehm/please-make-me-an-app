# Alternatives Landscape

Date: 2026-03-04

## Overview

Tools that turn websites into desktop apps. The space has a clear split: heavyweight (bundle Chromium) vs lightweight (use system webview or browser). Several projects have died in the last two years. The trend is toward system webviews.

## Active projects

### Pake

The closest alternative to what we're building. Spiritual successor to Nativefier.

- **Status:** Very active, 46K GitHub stars, v3.10.0 (Feb 2026)
- **Engine:** System webview via Tauri/wry (WebKitGTK on Linux, WebKit on macOS, WebView2 on Windows)
- **App size:** 2-5 MB
- **License:** MIT
- **How it works:** CLI that converts a URL to a standalone desktop app binary. `pake-cli` handles the build.
- **Differences from us:** Pake produces compiled binaries (one-time build step). We're config-driven at runtime (no build step, just run the command). Pake doesn't emphasize profile isolation or per-app CSS/JS injection the way we do.

### Electron

Still the 800-pound gorilla. Not going anywhere.

- **Status:** Very active, v40.7.0 (Mar 2026), new major every 8 weeks tracking Chromium
- **Engine:** Chromium (bundled)
- **App size:** 80-150 MB minimum
- **Idle RAM:** 150-300 MB
- **License:** MIT
- **The "bloat" question:** Yes, still bloated. Every app ships its own Chromium. But ecosystem maturity and Node.js integration keep it dominant. VS Code, Slack, Discord, Obsidian all use it. Roughly 60% of cross-platform desktop apps.
- **Differences from us:** Electron is a developer framework for building apps with web tech, not a "wrap this URL" tool. Completely different use case.

### Tauri

The Electron alternative for developers who want smaller apps.

- **Status:** Very active, v2.10.2 (Feb 2026)
- **Engine:** System webview via wry
- **App size:** 2.5-10 MB
- **Idle RAM:** 30-50 MB
- **License:** MIT / Apache 2.0
- **Differences from us:** Tauri is a developer framework (Rust backend, web frontend). We use wry (Tauri's webview library) directly but for a different purpose: wrapping existing websites, not building new apps.

### Neutralinojs

Lightweight Electron alternative using system webviews.

- **Status:** Active, v6.4.0, CLI updated Jan 2026
- **Engine:** System webview (WebKitGTK, WebKit, WebView2)
- **App size:** < 5 MB
- **License:** MIT
- **How it works:** C++ runtime exposing native APIs via WebSocket to a JS frontend. No Node.js.
- **Differences from us:** Developer framework, not a URL wrapper. Smaller ecosystem than Tauri.

### NW.js (formerly node-webkit)

The original, predating Electron (2011).

- **Status:** Active, v0.109.0 (Mar 2026, Chromium 146, Node v25.2.1)
- **Engine:** Chromium (bundled)
- **App size:** 100+ MB
- **License:** MIT
- **Differences from us:** Same story as Electron. Developer framework, bundles Chromium, large binaries.

### Tangram

GNOME-native web app manager. Closest in spirit to what we're building on Linux.

- **Status:** Active (slower pace), v3.4, last commit Nov 2025
- **Engine:** WebKitGTK (system)
- **App size:** Tiny (GJS app)
- **License:** GPLv3
- **How it works:** A GNOME Circle app. Persistent-tab browser where each tab is an independent web app with its own session.
- **Differences from us:** GUI-based (not config-driven), GNOME-specific, tabs in one window rather than separate windows per app. No CSS/JS injection. No CLI.

### Linux Mint Web Apps (webapp-manager)

Simple, built into Linux Mint.

- **Status:** Active, ships with Mint 22.x
- **Engine:** Uses whatever browser you have installed (Firefox, Chromium, etc.)
- **App size:** Tiny (Python script)
- **License:** GPLv3
- **How it works:** Creates `.desktop` entries that launch sites in a browser window with hidden navigation chrome.
- **Differences from us:** No session isolation between apps. No CSS/JS injection. Firefox 133+ broke the "hide nav bar" feature. Depends on installed browser behavior.
- **Note:** Based on Peppermint OS's ICE.

### COSMIC Web Apps

System76's take for their COSMIC desktop.

- **Status:** Active, early stage (COSMIC hit stable Dec 2025)
- **Engine:** System browser
- **License:** GPL
- **Differences from us:** Tied to COSMIC desktop environment. Very early.

### Browser-native approaches

#### Chrome/Edge PWA (`--app=URL`)

Still works. "Install as app" from the address bar creates a standalone window with its own taskbar entry. Zero additional software. Works on Chrome, Edge, Brave, Opera, Vivaldi. **Limitation:** apps share the browser's process tree (not fully isolated), limited customization, no per-app CSS/JS injection.

#### Safari "Add to Dock" (macOS Sonoma+)

Built-in since macOS Sonoma (2023). Creates site-specific browsers using WebKit. Free, zero download. macOS only.

#### PWAsForFirefox

Community extension that creates a modified Firefox runtime per app. Active, tracks every Firefox release (compatible with Firefox 147, Jan 2026). MPL 2.0. **Risk:** works by modifying Firefox internals, which is unsupported and can break with any update.

#### Firefox "Taskbar Tabs"

Mozilla's official effort, introduced in Firefox 143 (Sep 2025). Experimental, **Windows only**, does not implement full PWA spec. Apps retain Firefox UI. Not a serious solution yet.

### Multi-service messengers

These solve a different problem (many services in one window) but overlap with our use case.

| Tool | Status | License | Notes |
|------|--------|---------|-------|
| **Ferdium** | Active (v7.1.1, Oct 2025) | Apache 2.0 | Community fork of Franz, free, no limits. Electron-based (~180 MB). |
| **Franz** | Active (v5.11.0, Sep 2025) | Apache 2.0 | Free tier increasingly hostile (15-second wait penalty, 3 service limit). |
| **Rambox** | Active (v2.5.2) | Proprietary | Open-source Community Edition discontinued. Commercial product now. |
| **WebCatalog** | Active | Proprietary | No Linux support. Free tier limited to 2 apps. $4-5/mo for Pro. |

### Commercial / macOS-only

- **Fluid** (macOS): Long-standing site-specific browser using WebKit2. Still alive but low activity.

## Dead projects

| Project | Archived | What it was | Why it matters |
|---------|----------|-------------|----------------|
| **Nativefier** | Sep 2023 | CLI to wrap URLs as Electron apps | The original "turn URL into desktop app" tool. Archived, recommends browser PWA features instead. |
| **Gluon** | Feb 2024 | Framework using installed browsers via CDP | Unique approach (no webview, no bundled browser). Died after ~14 months. |
| **DeskGap** | Dec 2024 | Electron-like but with OS webviews + Node.js | Tried to be "Electron but lightweight." API was incomplete. |

## Where please-make-me-an-app fits

The gap we fill:

- **Pake** builds binaries. We're runtime/config-driven (no build step, just edit YAML and run).
- **Tangram** is GUI-based and GNOME-specific. We're CLI-first and desktop-agnostic.
- **Linux Mint Web Apps** has no session isolation, no injection, and is breaking with Firefox updates.
- **Browser PWAs** have no per-app CSS/JS injection, limited isolation, and depend on browser-specific behavior.
- **Electron/Tauri/Neutralino** are developer frameworks for building new apps, not wrapping existing sites.

Our niche: **config-driven, CLI-first web app wrapper with profile isolation and page customization, using native webviews.** The closest thing to "dotfiles for your web apps."

## Key trends

1. **System webviews are winning.** Tauri, Pake, Neutralinojs all use them. 2-10 MB vs 80-150 MB.
2. **Nativefier's gap is mostly filled by Pake**, but Pake requires a build step. There's room for a runtime approach.
3. **Browsers themselves are the simplest option**, but they lack customization and proper isolation.
4. **Electron isn't going anywhere.** Ecosystem inertia is real.
5. **The graveyard is growing.** Nativefier (2023), Gluon (2024), DeskGap (2024). Projects in this space either gain critical mass or die within 1-2 years.
