# Agent Fabric

Rust capability discovery and asynchronous delegation built on Controls' existing
Go communication service and process supervisor. Coding harnesses discover
specialists, submit structured work, and communicate directly without a predefined
Factory workflow or Hatchet.

## Start here

Build Fabric and validate the example:

```sh
cargo build --release
target/release/fabric validate fabric.example.yaml
```

Start Controls' standalone `factory-runtime` with an explicit PostgreSQL URL and
private runtime directory, as described in the [operator guide](docs/operations.md).
The example expects its socket at `/tmp/fabric-controls/factory.sock`. Then:

```sh
target/release/fabric serve fabric.example.yaml /tmp/fabric-state
# In another terminal:
target/release/fabric call /tmp/fabric-state/fabric.sock catalog '{}'
target/release/fabric call /tmp/fabric-state/fabric.sock invoke \
  '{"request_key":"example-1","package":"utility","capability":"echo","input":{"task":"inspect"}}'
target/release/fabric call /tmp/fabric-state/fabric.sock wait \
  '{"id":"RETURNED_INVOCATION_ID","seconds":20}'
```

Connect a harness through the stdio command:

```sh
/path/to/fabric/target/release/fabric mcp /tmp/fabric-state/fabric.sock --identity development
```

Load the reusable [Fabric skill](skills/fabric/SKILL.md). Managed invocations receive
an embedded copy automatically; native launchers choose how to load it. MCP tools
cover discovery, structured calls, status, bounded waiting, cancellation, live
registration, and the canonical Controls communication operations.

## Contracts and ownership

`fabric.yaml` version 1 reuses package `name`, `path`, `entrypoint`, `args`, and
`overrides` conventions. Packages supply their own native profiles, tools, and
skills. Paths resolve from configuration; entrypoints stay within their packages.
`environment` maps variable names to names in the host environment, not secret
values. `working_directory`, `elapsed_seconds`, `concurrency`, and
`allow_delegation` control execution. Capabilities declare names, descriptions,
versions, and JSON Schema 2020-12 inputs/outputs with local references only.
Accepted calls pin those contracts and settings.

Fabric owns invocation admission, status, results, and registration leases.
Controls owns identity, credentials, threads, messages, receipts, and process
sessions. Fabric accesses supported APIs rather than Controls tables. All CLI/MCP
submissions go through the same durable admission path. Capacity exhaustion rejects
unaccepted work; there is no hidden queue. Child calls use the worker's existing
identity and permission, with parent/root attribution.

This is not full factory.yaml compatibility. Workflow routes, hooks, variables,
native workflow-runtime requirements, and declarative loops are not supported.
Existing workflow-oriented packages need a Fabric-aware completion contract.
Connected implementations register with renewable leases and use comms mailboxes;
they own their processes and resource enforcement. Local host execution is trusted
and is not a sandbox. See the guides for recovery and supervisor-loss limitations.

## Guides and verification

- [Operator guide](docs/operations.md): setup, MCP attachment, CLI, collaboration,
  lifecycle, and recovery.
- [Package contract](docs/packages.md): invocation context, native integration,
  structured completion, and connected implementations.
- [Verification evidence](docs/verification.md): reproducible commands, native
  Codex MCP smoke test, deterministic integration tests, and known limits.
- [Implementation tracker](issues.md): ordered milestones and completed work.

`cargo test` runs unit tests. The private Controls/PostgreSQL harness explicitly
runs integration tests; its command and optional authenticated native test are in
the verification guide. The fixtures need Python 3; Fabric itself is Rust.
