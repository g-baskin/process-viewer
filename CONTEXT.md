# Glossary

**Process** — one running program instance, identified by PID plus start time.

**App** — a process macOS lists as a regular app with a Dock icon.
Code kind: `app`.

**Menu bar app** — an app that runs in the menu bar or the background with no Dock icon.
Code kind: `menubar`. Avoid: background app.

**App helper** — a process that belongs to an app (macOS reports the app as responsible for it).
Code kind: `helper`. Avoid: using "helper" alone for the startup helper.

**Background program** — a process not owned by any app and not part of macOS, e.g. a vendor daemon.
Code kind: `background`. Avoid: service, background service.

**macOS system process** — a process that is part of macOS or owned by root or a system account.
Code kind: `system`. Avoid: using "system" for startup items.

**Protected process** — a core macOS process (or this app) that can never be stopped.

**Startup item** — a launchd plist set to start a program automatically; Apple's own are left out.
Avoid: login item (macOS uses that for System Settings › Login Items).

**Launch agent** — a startup item that runs when a user logs in.
Scope `user` (just you) or `all-users` (anyone who logs in).

**System service** — a startup item that starts with the Mac and runs as root.
Scope `system-service`. Avoid: daemon (in UI), "system" alone.

**Scope** — which group a startup item belongs to: `user`, `all-users` or `system-service`.

**Startup helper** — the small root tool that, once installed, switches system services on and off without a password.
Avoid: root helper, privileged helper, "helper" alone.
