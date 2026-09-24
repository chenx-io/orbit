# Orbit

🚀 All-in-one open-source API testing toolkit

[English](README.md) · [简体中文](README.zh-CN.md)

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![CI](https://github.com/chenx-io/orbit/actions/workflows/ci.yml/badge.svg)](https://github.com/chenx-io/orbit/actions/workflows/ci.yml)

Orbit is a modern, all-in-one API testing toolkit that unifies **API debugging**, **scenario automation**, and **performance load testing**. Design requests, compose complex business scenarios, simulate high-concurrency traffic, and validate API responses — all in one place.

No heavy client dependencies. Built for developers and QA engineers.

## Features

- **Multi-protocol API debugging** — HTTP, gRPC, WebSocket, TCP, UDP, SSE, and GraphQL.
- **Scenario automation** — chain requests with loops, conditions, groups, and variables.
- **Performance load testing** — constant, ramping, and arrival-rate VU models with thresholds and SLA gates.
- **Distributed load testing** — scale out with a Controller + Agents over gRPC.
- **Mock server** — spin up local mocks from your test plans.
- **Import & export** — Postman, OpenAPI, cURL, HAR, JMeter, k6, and more.
- **Postman-compatible scripting** — `pm.*` pre/post request scripts powered by QuickJS.

## Installation

One-line install (installs `orbit-cli`, including the distributed agent):

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/chenx-io/orbit/main/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/chenx-io/orbit/main/install.ps1 | iex
```

Install the desktop app as well:

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/chenx-io/orbit/main/install.sh | bash -s -- --app

# Windows (PowerShell)
irm https://raw.githubusercontent.com/chenx-io/orbit/main/install.ps1 -OutFile install.ps1; ./install.ps1 -All
```

Pin a version or choose a custom install directory:

```bash
curl -fsSL https://raw.githubusercontent.com/chenx-io/orbit/main/install.sh | bash -s -- --version v0.1.0 --dir ~/tools/orbit
irm https://raw.githubusercontent.com/chenx-io/orbit/main/install.ps1 -OutFile install.ps1; ./install.ps1 -Version v0.1.0 -Dir "$env:LOCALAPPDATA\Orbit\bin"
```

> Downloaded archives are verified against `SHA256SUMS` from the GitHub release.
> To point the script at a different repository, set the `ORBIT_REPO` environment variable.

## Quick Start

Create a test plan `plan.yaml`:

```yaml
name: Quick start
scenarios:
  - name: Get example
    executor:
      type: sequential
      iterations: 1
    steps:
      - type: request
        name: GET /get
        request:
          method: GET
          url: https://httpbin.org/get
        checks:
          - type: status
            value: 200
```

Run it:

```bash
orbit run plan.yaml
```

## Building from Source

### Prerequisites

| Dependency | Notes |
| --- | --- |
| [Rust](https://rustup.rs/) | stable toolchain (`rustup default stable`) |
| [Node.js](https://nodejs.org/) 18+ and [pnpm](https://pnpm.io/) | for the desktop app (`corepack enable` or `npm i -g pnpm`) |
| Linux | `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev patchelf` |
| macOS | Xcode Command Line Tools (`xcode-select --install`) |
| Windows | WebView2 (bundled with Win10/11) + [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) |

### Build orbit-cli

```bash
git clone https://github.com/chenx-io/orbit.git
cd orbit
cargo build --release -p orbit-cli
```

The binary is `target/release/orbit` (`orbit.exe` on Windows). Run `orbit --help` to see all commands (`orbit run`, `orbit agent`, ...).

### Build the desktop app

```bash
cd web
pnpm install
pnpm tauri:build
```

Installers are written to `web/src-tauri/target/release/bundle/` (`.deb` / `.rpm` on Linux, `.dmg` on macOS, NSIS `.exe` on Windows).

## License

Orbit is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE) (AGPL-3.0-or-later).
