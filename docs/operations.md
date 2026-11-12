# Operating Fabric

## Start the services

Build Controls' `cmd/factory-runtime` and Fabric's Rust binary:

```sh
# In Controls
 go build -o bin/factory-runtime ./cmd/factory-runtime
# In Fabric
 cargo build --release
```

Run Controls in one terminal with an operator-supplied PostgreSQL URL:

```sh
FACTORY_DATABASE_URL="$YOUR_DATABASE_URL" \
  /path/to/controls/bin/factory-runtime --root /tmp/fabric-controls
```

Standalone startup applies Controls migrations but does not start Hatchet or a
database installation. Use a dedicated database/runtime directory when running
independently of Factory. See Controls' `docs/standalone-runtime.md` for the
private host API and its ownership rules.

In another terminal, from the Fabric repository:

```sh
target/release/fabric validate fabric.example.yaml
target/release/fabric serve fabric.example.yaml /tmp/fabric-state
```

The example points at `/tmp/fabric-controls/factory.sock`. Use absolute socket
paths in real configuration. Services run in the foreground. Stop Fabric with
Ctrl-C; its Controls-supervised processes remain available for reconciliation.
Restart Fabric with the same state, name, and Controls socket. Stop Controls with
Ctrl-C to shut down its supervised processes too. State/history are retained.

## Attach a coding harness

Configure a stdio MCP server using this command and arguments:

```json
{
  "command": "/path/to/fabric/target/release/fabric",
  "args": ["mcp", "/tmp/fabric-state/fabric.sock", "--identity", "development"]
}
```

Load `skills/fabric/SKILL.md` using the harness's skill/instruction mechanism.
This repository does not modify global harness configuration. A stable identity
name recovers earlier calls; use distinct names for independent operators.
The stdio adapter can disconnect without stopping the daemon or its work.

Call `agents_list`, inspect a capability, then submit it with `agents_invoke`.
Continue other work and retrieve the result through `invocations_get` or
`invocations_wait`. Use `comms_*` tools for messages, threads, participants,
acknowledgements, and bounded waiting. Their schemas come from Controls.

The CLI can exercise the same service without MCP setup:

```sh
target/release/fabric call /tmp/fabric-state/fabric.sock catalog '{}'
target/release/fabric call /tmp/fabric-state/fabric.sock invoke \
  '{"request_key":"example-1","package":"utility","capability":"echo","input":{"task":"inspect"}}'
target/release/fabric call /tmp/fabric-state/fabric.sock wait \
  '{"id":"RETURNED_INVOCATION_ID","seconds":20}'
```

CLI calls share the local `cli` identity. Inside an assigned package, the CLI uses
`FABRIC_CONTEXT_PATH` instead and retains the package's permissions.

## Collaborating example

`fabric.demo.yaml` adds a reviewer fixture that invokes the utility specialist,
sends it a question directly, receives its answer, and returns a combined result.
The fixtures are deterministic so the example tests the integration independently
of model behavior. Build the debug binary and make it available to its packages:

```sh
cargo build
export PATH="$PWD/target/debug:$PATH"
fabric serve fabric.demo.yaml /tmp/fabric-demo
# From another terminal with the same binary on PATH:
fabric call /tmp/fabric-demo/fabric.sock invoke \
  '{"request_key":"review-1","package":"reviewer","capability":"review","input":{}}'
```

Wait for that ID. The result contains the specialist output, direct peer answer,
and child ID. The root operator can inspect the child; its caller is the reviewer,
not the root operator. Set concurrency to 1 to see explicit child-capacity rejection;
set `allow_delegation: false` to see permission rejection. The regression fixtures
handle those failures without deadlock.

## Lifecycle and recovery

| State | Meaning |
| --- | --- |
| accepted | Durable admission; capacity reserved |
| launching | Launch about to be issued or being observed |
| launch_uncertain | Launch response uncertain; reservation retained; never automatically relaunched |
| running | Implementation active; structured result not yet accepted |
| finishing | Result/error observed or deadline reached; awaiting supervised termination |
| cancelling | Cancellation requested; awaiting confirmation |
| completed | Valid result accepted; supervised termination confirmed where applicable |
| failed | Explicit failure, invalid result, exit without result, or external lease/deadline expiry |
| cancelled | Supervised termination or external completion acknowledged after cancellation |

`observation_error` reports interrupted observation separately from an execution
failure. Existing sessions are recovered by invocation/session identity on restart.
A launch uncertainty without a recoverable session needs operator investigation;
Fabric deliberately retains its reservation rather than guessing whether work ran.
No automatic side-effecting retry or persistent worker pool is implemented.

Results and contracts remain in the private SQLite state directory without
automatic expiry. Controls retains communication and session records in PostgreSQL.
Only one Fabric process may own a state directory. Do not remove state while calls
are active. Boundaries are trusted local APIs, not a process sandbox. External
implementations own their processes; an expired lease cannot prove they stopped.

## Repository ownership

- Controls: communication scopes, identities, credentials, threads, messages,
  receipts, delivery, sessions, captured process output, process stop/state.
- Fabric: capability catalog, registration leases, configuration, invocation
  admission/contracts/results, caller/parent attribution, MCP adapter, skill.
- Packages: native model/provider/tool configuration and capability behavior.

Native workflow-oriented packages need a Fabric-aware launcher and completion
contract. Controls' workflow Pi preparation is not automatically applied by raw
session launch. Do not assume factory.yaml runtime settings work unchanged.
