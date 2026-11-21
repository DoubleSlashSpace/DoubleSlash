# Documentation guide

These documents describe the repository implementation. A feature being present
in source does not establish that it passed a packaged-client or hardware check.
Open work belongs in the [backlog](../backlog.md); recorded manual results belong
in [acceptance](ACCEPTANCE.md) or the specific guide linked there.

| Need | Read |
|---|---|
| Install, connect, settings, troubleshooting, build | [Project README](../README.md) |
| Components, transport, encryption and storage boundaries | [Architecture](ARCHITECTURE.md) |
| Desktop and Android feature surfaces | [Client feature reference](CLIENT_FEATURES.md) |
| Android toolchain, JNI, APKs and device debugging | [Android](ANDROID.md) |
| Back up, restore, move an identity, device-routing limits | [Devices and backups](DEVICES_AND_BACKUPS.md) |
| Run a supernode, listeners, room policy, portal hosting | [Supernode operator guide](SUPERNODE.md) |
| Deploy/manage Linux hosts, CLI/TUI, clusters and desktop administration | [Supernode manager](supernode-manager.md) |
| Remaining Android administration and cloud-provider design | [Administration plan](SUPERNODE_CLIENT_ADMIN_PLAN.md) |
| Address lookup protocol | [DDDNS v1](DDDNS.md) |
| UI tokens, components and interaction rules | [Design system](../DESIGN.md) |
| Portal demos, SDK and browser/QUIC validation | [Portal app guide](../games/README.md) |
| Manual checks and evidence | [Acceptance](ACCEPTANCE.md) |
| Prioritized unfinished work | [Backlog](../backlog.md) |
| Attack surfaces and encryption limits | [Threat model](THREAT_MODEL.md) |
| Private vulnerability reporting | [Security policy](../SECURITY.md) |
| Local data, capture and optional services | [Privacy policy](../PRIVACY.md) |
| Use and optional DDNS service terms | [Terms](../TERMS.md) |
| Notices and package verification | [Dependency licensing](LICENSING.md) |
| Windows MSIX build and Store acceptance limits | [Microsoft Store packaging](MICROSOFT_STORE.md) |
| Deferred cryptography proposal | [Post-quantum assessment](POST_QUANTUM.md) |
| Contributor invariants and validation commands | [Agent contract](../agents.md) |

## Maintaining the documentation

Verify behavior at its consuming code path. Keep setup in the project README,
architecture in the architecture guide, and remaining work in the backlog.
Link to those owners instead of copying status tables into several guides.
Label proposals and historical results, and retain the build, environment and
limits of recorded evidence. Never promote a past test count into current sign-off.

From the repository root, run:

```sh
node scripts/check_documentation.mjs
git diff --check
```

The checker validates local Markdown links, target capitalization and heading
anchors across repository-owned Markdown. It excludes vendored codec/patch
documentation and local assistant configuration. It does not check external
websites, source-code identifiers, behavior, or legal sufficiency. Upstream
README/changelog files and retained licensing evidence are maintained separately
from the project's user and developer guides.
