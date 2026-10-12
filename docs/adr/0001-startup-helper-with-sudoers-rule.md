---
status: accepted
date: 2026-10-12
---

# Switch system services through a startup helper allowed by a sudoers rule

## Context and Problem Statement

Turning a system service (a third-party plist in /Library/LaunchDaemons) on or
off needs root. Asking for the admin password on every switch is tedious, but
any password-free route gives this app's user account a standing way to run
something as root. The app is ad-hoc signed, with no Apple Developer ID.

## Considered Options

* Ask for the admin password every time (AppleScript `with administrator privileges`).
* SMAppService / SMJobBless privileged helper daemon talking over XPC.
* An opt-in startup helper installed to /Library/PrivilegedHelperTools plus a
  sudoers rule (/etc/sudoers.d/process-viewer) that lets only this user run
  only that helper without a password.

## Decision Outcome

Chosen option: the opt-in startup helper plus sudoers rule, with password-each-time
kept as the default. SMAppService and SMJobBless need a Developer ID-signed app
and helper, which this project doesn't have. The helper is root-owned so the
user can't change it, and it only acts on third-party plists in
/Library/LaunchDaemons whose Label matches. "Turn this off" removes both the
helper and the rule.

## Consequences

* Good: once set up, switching a system service needs no password, and the
  setup is optional and can be undone.
* Bad: this adds a root-run tool and a sudoers entry to the Mac. Any bug in the
  helper's checks is a local privilege escalation, so its argument checks
  (privhelper.rs) need extra care in review.
* Bad: moving to SMAppService later means a migration that removes the old
  helper and rule from existing installs.
