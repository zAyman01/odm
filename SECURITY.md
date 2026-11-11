# Security Policy

## Supported versions

odm is in early development. Only the latest release and the `main` branch receive security fixes.

## Reporting a vulnerability

Do not open a public issue for a security problem.

Report it privately at <https://github.com/zAyman01/odm/security/advisories/new>.

Include what you found, how to reproduce it, and the version or commit you tested. You can expect a first reply within 7 days.

## Scope notes

- odm plugins are ordinary programs that run with the user's full permissions. This is by design and documented; a plugin doing harm is not a vulnerability in odm.
- The daemon's API is protected by a token and listens only on the local machine by default. Ways to reach the API without the token, or from a website open in the user's browser, are in scope.
