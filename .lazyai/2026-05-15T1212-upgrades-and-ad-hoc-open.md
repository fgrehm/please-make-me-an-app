---
status: open
---

# upgrades-and-ad-hoc-open

<!--
We want to (a) do a round of dependency upgrades on the Rust crate and (b) add support for opening ad-hoc URLs without a YAML config file.

Already decided in chat:
- Ad-hoc command shape: `pmma open --url <url>` (config positional becomes optional).
- Ad-hoc mode supports: ephemeral profile (no persistence) + a `--backend` flag.
- Upgrades and the feature ship as multiple commits.

Target artifacts:
- Cargo.toml (dep bumps), possibly src/* for breaking API changes.
- src/main.rs (Cli::Open variant), src/config.rs (default config for ad-hoc), and whatever profile/app glue is needed.

Open questions are about the upgrade scope/strategy and the exact ad-hoc behavior (name derivation, defaults merge, lock handling, flag surface).
-->

---

## Turn 1 (agent) - @claude-opus-4-7

Two tracks below. Pick a path on each and I'll proceed.

## Track A: dependency upgrades

Current pins (from `Cargo.toml`):

- `wry 0.54` + `tao 0.34` — move in lockstep, biggest churn risk (window/event API has shifted in newer versions).
- `tray-icon 0.21` — event channel API changed in recent releases.
- `image 0.25`, `clap 4`, `directories 6`, `ureq 3`, `notify-rust 4`, `serde_yaml_ng 0.10`, `gtk 0.18` — likely minor bumps.
- `edition = "2021"` — could move to 2024 (mechanical, separate concern).

I haven't been able to run `cargo update` / check the registry yet (devcontainer wasn't up at the time, and you stopped me before re-trying). To give you a real list I need to `crib up` and run `cargo update --dry-run` + `cargo tree`.

**Q1.** Strategy:
- (a) Conservative: `cargo update` for semver-compatible bumps only, one commit. Low risk, may leave wry/tao/tray-icon on old majors.
- (b) Full bump: separate commit per major upgrade (wry+tao together, tray-icon, image if needed, others). More churn, may need API fixes.
- (c) Let me start the container, gather the actual outdated list, and post it back here before you decide.

**Q2.** Edition 2024 — bump now or skip?

## Track B: ad-hoc open

Decisions locked in: `pmma open --url <url>`, ephemeral profile, `--backend` flag, multiple commits.

Still ambiguous:

**Q3.** Trigger condition. Current `Open` requires positional `config: PathBuf`. Options:
- (a) Make `config` optional. If absent and `--url` is present, run ad-hoc. If both present, override URL within config (current behavior). My recommendation.
- (b) Keep `config` required, add a separate `--url-only` flag.
- (c) Add a separate subcommand (e.g. `pmma open-url`). But you already vetoed that in the previous question.

**Q4.** Window title / app name. No config means no `name`. Options:
- (a) Derive from URL host: `web.whatsapp.com` -> window title "web.whatsapp.com".
- (b) Hardcode "pmma" or similar.
- (c) Add a `--title <str>` flag.

**Q5.** Ephemeral data dir.
- Use `tempfile::TempDir`, auto-cleaned on exit.
- Skip the flock entirely (multiple ad-hoc windows to the same URL are fine).
- OK to add `tempfile` as a runtime (non-dev) dependency? It's already a dev-dep.

**Q6.** Defaults merge. `config::load` currently merges `defaults.yaml` (XDG). For ad-hoc:
- (a) Synthesize a bare `AppConfig` from URL + backend flag, then run the same defaults merge path so adblock/clipboard/tray settings still apply. My recommendation.
- (b) Bare config, no defaults merge.

**Q7.** Backend flag values. `--backend webview|browser` (let existing resolver pick a Chromium binary), or be explicit with `webview|brave|chrome|chromium`?

**Q8.** Other knobs for ad-hoc beyond `--debug`? E.g. `--user-agent`, window size, `--profile` (named ad-hoc that persists). My recommendation: ship minimal, tell users to write a config if they want more.

---

## Turn 2 (human)

> ## Track A: dependency upgrades
>
> **Q1.** Strategy:
> - (c) Let me start the container, gather the actual outdated list, and post it back here before you decide.

Yes, I want to upgrade all that we can, IIRC we were not able to update something due to some incompatibility, pls double check that.

> **Q2.** Edition 2024 — bump now or skip?

No idea, I'm not a rust guy, need help deciding

> ## Track B: ad-hoc open
>
> **Q3.** Trigger condition. Current `Open` requires positional `config: PathBuf`. Options:
> - (c) Add a separate subcommand (e.g. `pmma open-url`). But you already vetoed that in the previous question.

I think this is better TBH

> **Q4.** Window title / app name. No config means no `name`. Options:
> - (a) Derive from URL host: `web.whatsapp.com` -> window title "web.whatsapp.com".

This

> **Q5.** Ephemeral data dir.
> - Use `tempfile::TempDir`, auto-cleaned on exit.
> - Skip the flock entirely (multiple ad-hoc windows to the same URL are fine).
> - OK to add `tempfile` as a runtime (non-dev) dependency? It's already a dev-dep.

SGTM

> **Q6.** Defaults merge. `config::load` currently merges `defaults.yaml` (XDG). For ad-hoc:
> - (a) Synthesize a bare `AppConfig` from URL + backend flag, then run the same defaults merge path so adblock/clipboard/tray settings still apply. My recommendation.
> - (b) Bare config, no defaults merge.

No idea, suggestion?

> **Q7.** Backend flag values. `--backend webview|browser` (let existing resolver pick a Chromium binary), or be explicit with `webview|brave|chrome|chromium`?

Support flag, default to webview

> **Q8.** Other knobs for ad-hoc beyond `--debug`? E.g. `--user-agent`, window size, `--profile` (named ad-hoc that persists). My recommendation: ship minimal, tell users to write a config if they want more.

Yes, minimal

---

## Turn 3 (agent) - @claude-opus-4-7

Container is up, ran `cargo outdated --root-deps-only`. Outdated list is short:

| Crate       | Current | Latest |
|-------------|---------|--------|
| clap        | 4.6.0   | 4.6.1  |
| libc        | 0.2.184 | 0.2.186|
| notify-rust | 4.14.0  | 4.17.0 |
| tao         | 0.34.8  | 0.35.2 |
| tray-icon   | 0.21.3  | 0.24.0 |
| wry         | 0.54.4  | 0.55.1 |

Everything else (image, directories, ureq, serde_yaml_ng, gtk) is already on the latest semver-compatible version. Good news.

## Re: the past incompatibility

Checked the changelogs for the three majors:

- **wry 0.55**: no breaking changes affecting us. Removed `WebViewAttributes`, `WebView::new`, `WebView::new_as_child` from public API — we already use `WebViewBuilder` (the recommended path), so this is a no-op for us. No WebKitGTK version bump, no MSRV bump. https://github.com/tauri-apps/wry/blob/dev/CHANGELOG.md
- **tao 0.35**: breaking changes are all Android/iOS lifecycle related (scenes, multi-window on mobile). Zero impact on a Linux-only project. https://github.com/tauri-apps/tao/blob/dev/CHANGELOG.md
- **tray-icon 0.22/0.23/0.24**: changelog explicitly says no breaking changes. 0.24 made GTK an optional feature on Linux (still enabled by default), so we may want to leave the default features alone. https://github.com/tauri-apps/tray-icon/blob/dev/CHANGELOG.md

I couldn't find a Cargo.toml comment or commit that pinned wry/tao to old versions intentionally — the git history shows the initial v0.1.0 release set these and no one revisited. If you remember a specific build error from a prior attempt, let me know and I'll keep an eye out; otherwise I think the pins are just from "haven't gotten around to it" rather than a known incompatibility.

**Recommendation**: bump everything. Three commits:
1. `chore(deps): bump patch/minor (clap, libc, notify-rust)` — boring.
2. `chore(deps): upgrade wry 0.54 -> 0.55 and tao 0.34 -> 0.35` — paired since wry pulls tao transitively.
3. `chore(deps): upgrade tray-icon 0.21 -> 0.24` — separate because it pulls a different muda version.

I'll run `cargo build` + `cargo test` + `cargo clippy` between each to catch any surprises the changelogs didn't mention.

## Re: Q2 (edition 2024)

Rust editions are roughly "language flavor years." Going from 2021 to 2024 mostly tightens a few lints and changes some defaults; the migration is automated (`cargo fix --edition`) and the diff is usually small.

For a small CLI project with no public library API, the upside is "slightly more modern defaults" and the downside is "an extra commit of noise." My recommendation: **skip for now**, bump it the next time we have a reason to touch every file anyway. Doesn't affect any feature work.

## Re: Track B updates

> **Q3**: separate subcommand — got it.

So the shape becomes:

```
pmma open <config> [--profile X] [--url Y] [--debug]
pmma open-url <url> [--backend webview|browser] [--debug]
```

I'd call it `open-url` for symmetry. Open to `try`, `tmp`, `quick`, `adhoc` if you have a preference.

> **Q4**: derive title from URL host — got it.

> **Q5**: TempDir + skip flock + add tempfile as runtime dep — got it.

> **Q6**: I asked you and you bounced it back. My pick: **(a) merge defaults**. Rationale: if you've configured `defaults.yaml` to turn on adblock + clipboard polyfill globally, you almost certainly want those for an ad-hoc URL too. Otherwise ad-hoc would feel weirdly stripped-down vs your normal apps. Going with (a) unless you object.

> **Q7**: "support flag, default to webview" — agreed. Just to nail down values: `--backend webview|browser` (matches the existing `backend:` config field which already accepts `webview`, `browser`, `brave`, `chrome`, `chromium`). I'll accept all the same values the YAML accepts for consistency, and default to `webview`. OK?

> **Q8**: minimal, ship it.

## Final ask before I start

**Q9.** Subcommand name: `open-url` (my pick), or something else?
**Q10.** Defaults merge: confirm (a) — merge `defaults.yaml` into the synthesized ad-hoc config?
**Q11.** Backend flag: accept all the same values as the YAML `backend:` field (webview/browser/brave/chrome/chromium), default webview. OK?
**Q12.** Commit ordering: do you want the three dep bumps landed first (and merged/squashed however you prefer) before I start on the feature, or interleave?

---

## Turn 4 (human)

> Everything else (image, directories, ureq, serde_yaml_ng, gtk) is already on the latest semver-compatible version. Good news.

I mean, can we go further than what you found? like be more aggressive in terms of what we upgrade? If already done just skip this

> ## Re: Q2 (edition 2024)
>
> For a small CLI project with no public library API, the upside is "slightly more modern defaults" and the downside is "an extra commit of noise." My recommendation: **skip for now**, bump it the next time we have a reason to touch every file anyway. Doesn't affect any feature work.

Nah, lets modernize, we can abort if it's a ton of work

> ## Re: Track B updates
>
> > **Q3**: separate subcommand — got it.
>
> I'd call it `open-url` for symmetry. Open to `try`, `tmp`, `quick`, `adhoc` if you have a preference.

`open-url`

> > **Q6**: I asked you and you bounced it back. My pick: **(a) merge defaults**. Rationale: if you've configured `defaults.yaml` to turn on adblock + clipboard polyfill globally, you almost certainly want those for an ad-hoc URL too. Otherwise ad-hoc would feel weirdly stripped-down vs your normal apps. Going with (a) unless you object.

I actually forgot we had support for those defaults TBH

> > **Q7**: "support flag, default to webview" — agreed. Just to nail down values: `--backend webview|browser` (matches the existing `backend:` config field which already accepts `webview`, `browser`, `brave`, `chrome`, `chromium`). I'll accept all the same values the YAML accepts for consistency, and default to `webview`. OK?

OK

> ## Final ask before I start
>
> **Q12.** Commit ordering: do you want the three dep bumps landed first (and merged/squashed however you prefer) before I start on the feature, or interleave?

bumps / edition first, then feature, all in a single branch to be reviewed by copilot

---

## Turn 5 (agent) - @claude-opus-4-7

> can we go further than what you found?

Ran `cargo outdated` unfiltered. One real finding:

- **`gtk 0.18` is marked UNMAINTAINED** by gtk-rs (suggested replacement: `gtk4`). However we can't migrate to gtk4 — wry's Linux backend is built on webkit2gtk-4.1 which is gtk3-based. wry has not migrated to webkit2gtk-6/gtk4 yet. Our `gtk = "0.18"` matches what wry/tao pull in transitively, which is intentional. So we're stuck on gtk3 until wry moves. Leaving it.

Everything else on the registry is current. No hidden newer majors.

## Locked plan

Single branch, commits in order:

1. `chore(deps): bump patch/minor versions` — clap 4.6.0→4.6.1, libc 0.2.184→0.2.186, notify-rust 4.14→4.17
2. `chore(deps): upgrade wry 0.54 -> 0.55 and tao 0.34 -> 0.35`
3. `chore(deps): upgrade tray-icon 0.21 -> 0.24`
4. `chore: migrate to Rust 2024 edition` (run `cargo fix --edition`; abort and back out if diff is huge)
5. Feature commits for `pmma open-url`:
   - `feat(config): add ad-hoc config constructor` (synthesize AppConfig from URL + backend, merge defaults)
   - `feat(profile): support ephemeral data dirs` (TempDir, no flock)
   - `feat(cli): add open-url subcommand`
   - Maybe a small docs commit to README/examples.

Then push as draft PR for Copilot review.

Starting now.
