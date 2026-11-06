# Security policy

Breezone controls heating and air conditioning and may be reachable from the Internet (through a
tunnel), so security reports are taken seriously.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through
[GitHub security advisories](https://github.com/EmmanuelVinas/breezone/security/advisories/new),
with the affected version, how to reproduce it and its impact. You should get an answer within a
week; a fix is released as soon as possible and you are credited if you wish.

## Supported versions

Only the latest release receives security fixes.

## Good practices for users

- Never expose port 3000 of the Airzone WebServers: their local API has no authentication.
- Reach Breezone from outside through HTTPS only (e.g. Cloudflare Tunnel).
- Use strong passwords and revoke lost devices in *Settings › Devices*.
- Keep the image up to date: `docker compose pull && docker compose up -d`.
