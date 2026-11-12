# Recovery and boundaries

## Lifecycle and recovery

| State | Meaning |
| --- | --- |
| accepted | Durable admission; capacity reserved |
| launching | Launch about to be issued or being observed |
| launch_uncertain | Launch response uncertain; reservation retained; never automatically relaunched |
| running | Implementation active; structured result not yet accepted |
| finishing | Result/error observed or deadline reached; awaiting supervised termination |
| cancelling | Cancellation requested; awaiting confirmation |
| completed | Valid result accepted; terminal session state reported where applicable |
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

The supervisor cannot restore lost PTYs or model memory after its own restart.
An interrupted session report does not prove an orphaned process stopped. Inspect
host state before resuming after supervisor loss. See [verification](verification.md)
for the tested restart boundary and the complete evidence record.
