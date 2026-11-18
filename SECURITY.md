# Security policy

## Supported versions
Rusted is in alpha. Only the latest commit on `main` gets fixes.

## Reporting a vulnerability
Please don't open a public issue for security problems. Use
[Report a vulnerability](https://github.com/RustedMC/Rusted/security/advisories/new) instead.

Useful things to include:
- what an attacker can do (crash the server, run commands, read files, ...)
- how to reproduce it, ideally with the Bedrock version you used
- the commit you tested on

You'll get a reply within a week. Once a fix is on `main`, the advisory is published with
credit to you, unless you'd rather stay anonymous.

## Keep in mind
- `identity.pem` is the server's private key. Never share it or commit it.
- `ops.txt` gives full control of the server, only list people you trust.
