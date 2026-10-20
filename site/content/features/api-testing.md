---
title: API testing and replay suites for CI
description: Turn captured requests into replay suites with assertions, run them headless in CI and read JUnit reports. Includes Postman collection import.
h1: Turn captured traffic into API tests
image: suites
image_alt: A replay suite run showing ten requests passed
---

The fastest way to write an API test is to do the thing once in the app and record what happened. ProxyZip turns a selection of captured requests into a **replay suite** and runs it again whenever you like.

## Build a suite

1. Capture the flow you care about: log in, add to cart, check out.
2. Press `Cmd+A` (or select the rows you want), right-click and choose **Add to a suite…**
3. Add checks. A request can assert on its status and body.
4. Use `{{vars.name}}` in URLs, headers and bodies, and define environments such as `staging`.
5. Pass values between steps: extract a token with a `jq` path and use it in the next request.

A suite is a single `.pzsuite` file, so it belongs in version control next to the code.

## Run it in CI

```bash
proxyzip run checkout.pzsuite --env staging --junit report.xml
```

`proxyzip run` exits with `1` when a request fails and writes a JUnit report that every CI system can display. The [suites guide](../../docs/suites/) shows a GitHub Actions job.

## Bring your own

- Import a **Postman collection** and run it with the same command
- Check responses against an [OpenAPI file](../mock-api-server/) with `proxyzip validate`
- Point a suite at a [mock server](../mock-api-server/) while the real API is being built

Read the [replay suites guide](../../docs/suites/).
