# Security Policy

## Supported Versions

CrabMate is under active development. We currently provide security fixes for:

| Version | Supported |
| --- | --- |
| `main` (latest commit) | :white_check_mark: |
| Older releases / tags | :x: |

If a vulnerability only affects older code paths, maintainers may still backport fixes at their discretion.

## Reporting a Vulnerability

Please do **not** open a public issue for suspected vulnerabilities.

Use one of the following private channels:

- GitHub Security Advisory: open a private report via repository **Security** tab.
- Email maintainers: `adustofsnow@163.com`

When reporting, include:

- Affected version / commit (`git rev-parse HEAD` output if possible)
- Reproduction steps or PoC
- Impact assessment (confidentiality / integrity / availability)
- Any suggested mitigation

## Response Expectations

- Initial acknowledgement: within **3 business days**
- Triage and severity assessment: within **7 business days**
- Fix timeline: depends on severity and release risk; critical issues are prioritized

If the report is accepted, maintainers will coordinate disclosure timing and credit preferences with the reporter.

## Sensitive Data Handling

To protect users and maintainers:

- Do not post real API keys, tokens, private keys, cookies, or full auth headers.
- Redact logs, traces, and request/response payloads before sharing.
- If credentials may be exposed, rotate/revoke them immediately.

This repository follows strict secret redaction and logging guidance in:

- `.cursor/rules/secrets-and-logging.mdc`

