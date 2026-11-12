# Agent Fabric

Rust capability discovery and asynchronous delegation built on Controls comms and
session supervision. Factory workflows remain optional and separate.

```sh
cargo test
cargo run -- validate fabric.example.yaml
cargo run -- catalog fabric.example.yaml
```

See [issues.md](issues.md) for implementation status.

## Configuration contract

`fabric.yaml` version 1 reuses package `name`, `path`, `entrypoint`, `args`, and
`overrides` conventions. Paths resolve from the configuration file; entrypoints
must stay inside their package. Packages can supply their own native profiles,
tools, and skills. This is not full factory.yaml compatibility: workflow routes,
hooks, variables, native runtime requirements, and declarative loops are rejected.
Native adapter preparation will need an explicit supported boundary; raw session
launch alone does not prepare Pi profiles.

Capabilities use JSON Schema 2020-12, local references only. Each accepted call
pins its version, schemas, and package settings; changes apply to future calls.
`environment` maps target variable names to names in the host environment; secret
values belong in the environment, never YAML. `overrides` is package-interpreted
JSON. Invocation and process session states have separate owners: Fabric owns
invocations, Controls owns sessions. Neither is a workflow step.

## Supervised execution

Start Controls' `factory-runtime` with an explicit PostgreSQL URL (see its
standalone-runtime.md). Set `controls_socket` in YAML to that host's private
socket. Then run one capability:

```sh
cargo run -- run fabric.example.yaml utility echo '{"hello":"world"}' /tmp/fabric-state
```

The example uses Python 3 and sends its result through Controls comms. A process
exit or printed JSON is not completion. Every invocation gets private input,
configuration, prompt, and communication-context files. `FABRIC_CONTEXT_PATH`
identifies the context, including the result schema and recipient. Packages send
`fabric.result` with data `{invocation_id, output}` or `{invocation_id, error}`.
Follow-up messages use the same thread and connection. Credential files are owner
only. The host supervises commands; Fabric checks the capability result contract.

Initial compatibility: command packages that implement this Fabric completion
contract. Existing workflow-oriented agent packages must adopt this contract;
their `step_result` is deliberately rejected. Native Pi profile preparation is
not implied by raw session launch and remains unsupported here until explicitly
integrated. Package-owned launchers may prepare their own native configuration.

Run cross-language integration tests with private PostgreSQL and a fresh standalone
Controls host (from the Controls checkout):

```sh
go run /path/to/fabric/tests/host/main.go /path/to/fabric
```

The harness removes its private runtime and stops its processes after testing.
Ordinary `cargo test` runs offline unit tests; integration tests are explicitly
marked as requiring this host.

## Asynchronous service

```sh
fabric serve fabric.yaml /absolute/private/fabric-state
fabric call /absolute/private/fabric-state/fabric.sock invoke \
  '{"request_key":"review-1","package":"utility","capability":"echo","input":{}}'
fabric call /absolute/private/fabric-state/fabric.sock get '{"id":"INVOCATION_ID"}'
fabric call /absolute/private/fabric-state/fabric.sock cancel '{"id":"INVOCATION_ID"}'
```

Submission returns immediately. Capacity exhaustion rejects unaccepted work;
there is no implicit queue or retry. Reuse request keys only with unchanged input.
Status and results are durable in Fabric's SQLite database; Controls remains the
owner of process state and communications. Results are retained until the owner
removes the state directory. Do not remove state while work is active.

The service reserves capacity until process termination is confirmed. Completion
passes through `finishing`; cancellation passes through `cancelling`. On restart,
Fabric resumes observation of known sessions without relaunching them. An uncertain
launch becomes `launch_uncertain`, retains its capacity reservation, and is never
automatically repeated. Inspect the Controls sessions and resolve uncertain work
before reusing its capacity. Stopping Fabric leaves supervised work with Controls;
restart with the same state/configuration to reconcile it. Transport failures are
reported while reconciliation continues. Accepted inputs/contracts remain pinned.

## Connected implementations

An authenticated comms peer can register `{name, capabilities, lease_seconds,
allow_delegation}` through Fabric's `register` operation. Leases are 1–300 seconds;
renew by registering again with the same identity/name. The response includes a
mailbox thread. Read it through Controls comms to receive `fabric.invoke` messages
containing the pinned contract, input, invocation thread, and result recipient.
Send `fabric.result` on that invocation thread. No extra messaging protocol or
process launcher is introduced for connected implementations.

`catalog` and `describe` include configured and connected capabilities. Expired
or withdrawn registrations remain inspectable as offline. An unavailable identity
is unknown, not ready. Another identity cannot update or withdraw a registration.
Changing registration contracts affects future calls only. Withdrawal/lease expiry
fails unfinished calls explicitly; it cannot prove an external process stopped.
For external calls, cancellation waits for the implementation's completion/error
acknowledgement; lease expiry reports failure with unknown process state. Connected
implementations own their execution environments and must implement their own
resource enforcement; Fabric bounds admitted invocations, not external processes.

## Connect an existing coding session

Run `fabric mcp /absolute/private/fabric-state/fabric.sock --identity my-session`
as a stdio MCP server in your harness. Keep the same identity name to retrieve
that session's prior invocations after reconnecting; distinct names keep callers
separate. Omitting the name creates a fresh identity. The stdio process attaches
to the running service; it does not start a workflow or launch another operator.

The generic tools are `agents_list`, `agents_describe`, `agents_invoke`,
`invocations_get`, `invocations_wait`, `invocations_cancel`, `agents_register`, and
`agents_withdraw`. Existing comms tools are exposed with a `comms_` prefix; their
schemas come directly from Controls. New capabilities appear through discovery
without adding tools. Results are in `structuredContent.result` and text content.
All mutations must use their documented idempotency keys.

CLI `call` uses a stable local `cli` identity. Managed packages automatically use
their identity from `FABRIC_CONTEXT_PATH`; use `fabric mcp SOCKET --context PATH`
to give a native agent that same scoped MCP connection. No credential is passed
on the command line. Fabric authenticates these credentials through Controls and
checks invocation ownership; it does not maintain a second identity authority.
The owner-only Unix socket remains a trusted host interface, not a sandbox.

## Direct delegation

Set `allow_delegation: true` on a package to let its running instances invoke
other capabilities. Fabric infers the parent from the caller's existing identity;
managed workers cannot bypass this policy by registering another capability.
Connected workers specify `parent_id` and need delegation enabled on that accepted
parent contract. Operator sessions may submit independent work directly.

Child calls record the caller, parent, and root operator. The root operator can
inspect descendants, but need not relay messages or results. Workers must finish
required child work before completing their own invocation; parent completion does
not automatically cancel children. Capacity applies to every admitted call. If a
parent occupies the last slot, its child call fails immediately with
`capacity_exhausted`; there is no hidden queue that can deadlock the parent.
These are protocol permissions within a trusted host, not host-process isolation.

Read the [operator guide](docs/operations.md), [package contract](docs/packages.md),
and reusable [Fabric skill](skills/fabric/SKILL.md). The skill is embedded in the
binary and provisioned for every managed invocation; native launchers decide how
to load it into their harness.
