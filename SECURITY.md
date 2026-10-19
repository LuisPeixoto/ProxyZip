# Security policy

ProxyZip decrypts network traffic and manages a private certificate authority, so we take security reports seriously.

## Supported versions

Only the latest release receives security fixes while the project is below 1.0.

## Reporting a vulnerability

**Please do not open a public issue.** Use GitHub's private reporting instead:

<https://github.com/LuisPeixoto/ProxyZip/security/advisories/new>

Include, where possible:

- A description of the problem and its impact
- Steps to reproduce, or a proof of concept
- Affected versions and operating systems

You will get an acknowledgement as soon as the maintainer can review it. We will keep you informed while the issue is
fixed, and credit you in the release notes unless you prefer to stay anonymous.

## What is in scope

- Exposure or mishandling of the CA private key
- The proxy accepting connections from unintended networks by default
- Bypass of certificate verification toward upstream servers when the user did not opt out
- Memory safety, request smuggling or parsing flaws in the proxy engine
- Injection or script execution in the desktop interface through captured content

## Design notes

- The proxy listens on loopback (`127.0.0.1`) unless you enable LAN connections.
- The CA private key is stored in the user data directory with owner-only permissions (Unix).
- Upstream certificates are verified by default. Disabling verification is an explicit setting and the CLI flag is
  named `--insecure`.
- ProxyZip has no telemetry and makes no network connections other than the traffic you route through it.

## Responsible use

ProxyZip is a debugging tool. Using it to intercept traffic you are not authorized to inspect may be illegal.
