# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 3.x     | Yes       |
| 2.x     | Security fixes only, while practical |
| < 2.0   | No        |

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security reports.

Prefer one of:

1. [GitHub Security Advisories](https://github.com/s00d/tauri-plugin-serialplugin/security/advisories/new) (private report)
2. Email the maintainer listed on [npm](https://www.npmjs.com/package/tauri-plugin-serialplugin-api) / GitHub profile for `s00d`

Include:

- Affected version(s) and platform (desktop OS / Android)
- Impact (e.g. unexpected port access, crash, data exposure)
- Minimal reproduction or PoC if you have one
- Whether you are OK being credited

You should get an acknowledgement within a few days. Coordinated disclosure is preferred; we will work with you on a fix and release timeline.

## Scope notes

This plugin talks to local serial / USB serial devices. Reports about:

- ACL / capability misconfiguration in *consumer apps* (missing `allow-*` permissions)
- Bugs in third-party USB chipset drivers outside this repo

…are often application or hardware issues rather than vulnerabilities in the plugin itself — still feel free to report if unsure.
