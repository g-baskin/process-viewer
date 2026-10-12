# Process Viewer

A macOS desktop app (Tauri 2 + Rust) that shows what's running on your Mac,
what each process is, which program is running it, and lets you stop it.

## Views
- **Apps**: your open apps (Dock apps, then menu bar apps), each
  with its app helpers folded underneath and their CPU/memory added up.
  Below them, other background programs, plus macOS system processes when
  "System processes" is ticked.
- **Tree**: every process under the process that launched it.
- **All processes**: a flat sortable list.
- **Startup**: startup items, i.e. programs set to start automatically
  (launch agents from LaunchAgents and system services from LaunchDaemons,
  Apple's own left out), whether they're on and running, and a button to turn
  each one off or on.
  System services normally need your password each time; "Stop asking for my
  password…" (top of the Startup tab) asks once and installs the startup helper
  (/Library/PrivilegedHelperTools/com.dellcbyer.processviewer.startup-helper)
  plus a sudoers rule (/etc/sudoers.d/process-viewer) allowing only that helper.
  The startup helper only switches third-party plists in /Library/LaunchDaemons
  whose Label matches. "Turn this off" removes both. Why: see
  [docs/adr/0001](docs/adr/0001-startup-helper-with-sudoers-rule.md).

Every list lets you pick several rows: ⌘-click adds or removes one, ⇧-click
or ⇧-arrow picks a range, ⌘A picks all, Esc goes back to one. Several
processes can then be quit or force quit together (one password prompt for
"as admin"), and several startup items turned on or off together (one
password prompt for all the system services, or none with the startup helper).

Terms used in code, UI and docs are defined in [CONTEXT.md](CONTEXT.md).

The detail pane shows live CPU and memory graphs (last 3 minutes, app +
helpers for apps); the status bar shows total CPU.

Click a row to see a plain-English description, the app it belongs to, the full
"launched by" chain (click any link to jump to it), program file (Show in
Finder), command line, user, and running time.

## Stopping processes
- **Quit / End process** asks politely (apps can offer to save).
- **Force quit** stops it immediately.
- **Force quit as admin…** appears for processes owned by root or another
  user, and asks for your password via macOS.
- Core macOS processes (kernel_task, launchd, WindowServer, loginwindow, …)
  and this app itself are protected and can't be stopped.
- The process's start time is checked before stopping it, so a reused PID is
  never killed by mistake.

Keys: ⌘F search, ⌘R refresh, ↑/↓ move, ←/→ fold/unfold, ⌘⌫ quit selected.

## Run / build
```sh
npm install
npm run dev      # run in development
npm run build    # makes src-tauri/target/release/bundle/macos/Process Viewer.app and a .dmg
```
`src-tauri/target/debug/process-app --dump` prints one snapshot as JSON, and
`--dump-startup` prints the startup items as JSON (debugging).

## Test
```sh
cd src-tauri && cargo test
npm test   # front-end selection logic
```

The app is ad-hoc signed (no Apple Developer ID), so on another Mac the first
open needs right-click → Open. If `npx tauri build` hangs, run
`./node_modules/.bin/tauri build`.

Requires Rust 1.90+ (sysinfo is pinned to 0.37 for that), Node, Xcode CLT.

Without Full Disk access, macOS hides the program path of processes owned by
root/other users; those show as "hidden by macOS".

## Licence
MIT; see [LICENSE](LICENSE).
