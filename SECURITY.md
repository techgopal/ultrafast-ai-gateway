# Security Policy

## Supported versions

Ultrafast v2 is in alpha (`2.0.0-alpha.x`). Fixes go into the latest alpha
release on `main`; older alphas and v1 (tag `v1-final`) get none.

## Reporting a vulnerability

Do not open a public issue. Use GitHub's private vulnerability reporting on
this repository (Security tab, "Report a vulnerability"), or write to the
maintainer at techgopal.io@gmail.com. Include the version, what you did and
what happened. This is a one-person project: expect a first answer within a
week, and a fix or a decision on whether it is in scope after that.

## In scope

- Authentication and authorization: sign-in, sessions, access tokens, virtual
  keys, roles, and the model, route and key access rules.
- Leaks of provider credentials, the master key, keys or tokens through the
  API, logs, backups or errors.
- The `/v1` proxy, the admin API under `/api`, `/metrics`, and the embedded
  console (for example cross-site scripting or request forgery).
- Bypass of rate limits, budgets or the trusted-proxy handling.

## Out of scope

- Findings that need `--insecure-cookies`, a trusted proxy list that includes
  networks clients can reach, or access to the data directory or master key.
- The providers' own APIs, and denial of service by an authenticated caller
  within the limits an admin chose.
- Phase 2 features that do not exist yet (guardrails, single sign-on).
