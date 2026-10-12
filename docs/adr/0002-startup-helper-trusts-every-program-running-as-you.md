---
status: accepted
date: 2026-10-12
---

# Keep the startup helper, and say plainly that every program running as you can use it

Supersedes 0001.

## Context and Problem Statement

ADR 0001 chose an opt-in startup helper plus a sudoers rule so system services
can be switched without a password. A security review (12 October 2026)
pointed out a consequence 0001 did not record: the sudoers rule trusts the
user account, not Process Viewer. Any program running as that user can call
the helper with no password, so malware running as the user could silently
turn off third-party system services, including antivirus, VPN and
company security agents. Apple's own services are never reachable.

The same review found that setup passed the helper and the rule through the
user's temp folder, where a program running as the user could swap them
while the password prompt was open.

## Considered Options

* Keep the helper as is and record the consequence.
* Keep the helper, and refuse to switch known security products.
* Remove the helper and ask for the password every time.
* SMAppService / SMJobBless (still needs a Developer ID this project lacks).

## Decision Outcome

Chosen option: keep the helper, state the consequence where the choice is
made, and close the setup gap. Password-each-time stays the default; the
helper is installed only when the user ticks "Don't ask for my password
again" or clicks "Stop asking for my password…", and both now say that other
programs on the Mac could use it too. A deny list of security products was
rejected: it can never be complete and would give a false sense of safety.

Setup now carries the helper and the rule inside the admin command itself
(base64) and decodes them into a fresh root-only folder, so nothing in the
user's temp folder can be swapped.

## Consequences

* Good: switching a system service needs no password once set up; setup is
  optional, can be undone, and no longer goes through user-writable files.
* Bad: any program running as the user can turn third-party system services
  on or off without a password, including security software. Users of
  company-managed Macs should leave the helper off.
* Bad: this adds a root-run tool and a sudoers entry to the Mac. Any bug in
  the helper's checks is a local privilege escalation, so its argument checks
  (privhelper.rs) need extra care in review.
* Bad: moving to SMAppService later means a migration that removes the old
  helper and rule from existing installs.
