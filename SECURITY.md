# Security policy

## Reporting a vulnerability

Please don't open a public issue for security problems. Report them privately through
[GitHub's security advisories](https://github.com/southstar21dev/MiniMacPad/security/advisories/new)
for this repository.

Include what you found, how to reproduce it, and which versions are affected. You'll get a reply
within a week; once a fix is released, the advisory is published with credit to you unless you'd
rather stay anonymous.

## Supported versions

Only the latest release receives security fixes.

## Scope

The settings app, the background service and the updater. MiniMacPad runs the actions you
configure, including shell commands, with your own user's permissions; a config file you wrote
doing what it says is not a vulnerability. How updates are verified is described in
[docs/RELEASING.md](docs/RELEASING.md).
