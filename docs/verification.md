# Integration verification

Verified on 2026-11-11 in the local development environment.

## Environment

- Fabric: Rust 1.88.0, pinned project toolchain and committed Cargo.lock.
- Controls: Go 1.26.10; standalone runtime from this checkout.
- Official MCP TypeScript client: installed Controls workspace dependency.
- Node.js: 25.8.1; Python 3 for deterministic package fixtures.
- Native Codex CLI: 0.163.0-alpha.10.2, existing configured model/authentication.
- PostgreSQL: private temporary instance managed by embedded-postgres v1.34.0.

The harness uses a dedicated database, short temporary Unix socket paths, and
fresh runtime directories. It configures an invalid Hatchet mode deliberately;
any accidental workflow-engine initialization fails. No global harness settings,
production data, or application repositories are modified. Temporary services and
processes are stopped after testing.

## Reproduce

From Fabric:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

From Controls, with its Go dependencies and npm workspace dependencies installed:

```sh
go test ./...
go vet ./...
go run /path/to/fabric/tests/host/main.go /path/to/fabric
```

The last command starts private PostgreSQL and standalone Controls, then explicitly
runs the Rust integration tests. Ordinary cargo test leaves those host-dependent
tests ignored. Use an authenticated Codex CLI for the separate native test:

```sh
FABRIC_TEST_SUITE=native \
  go run /path/to/fabric/tests/host/main.go /path/to/fabric
```

The native test uses the existing configured model and credentials. It adds its
Fabric MCP server through command-line config overrides, runs with a read-only
sandbox in a temporary directory, and does not persist a Codex session. It sends
no messages outside the local test services. This command performs model work;
the deterministic integration suite does not.

## Observed results

All checks below passed:

| Check | Evidence |
| --- | --- |
| Unit tests | Configuration, schema rejection, duplicate package rejection, workflow exclusion |
| Standalone Controls | Scope/identity provisioning, authenticated MCP, sessions, invalid Hatchet mode |
| concurrent_durable_invocations_and_cancellation | Overlapping session intervals, idempotent calls, conflicting keys, capacity rejection, cancellation, retained results |
| supervised_completion_and_failures | Structured result, invalid output, launch failure, process exit without completion |
| deadlines_invalid_output_and_process_failure | Enforced elapsed deadline, stopped process, explicit implementation failure |
| comms_listener_reconnect_retains_results | Rediscovery after stale listener endpoint; cross-scope membership rejection |
| live_registration_and_pinned_contracts | Live registration, invocation, contract update during work, ownership checks, withdrawal, lease expiry, retired identity |
| official_mcp_client_interoperability | Official SDK discovery/invocation/wait, caller isolation, generic tool stability, two-way follow-up messages |
| service_restart_observes_existing_session | Fabric process killed and restarted; existing supervised call completes without duplicate launch |
| direct_peer_delegation_permissions_and_capacity | Reviewer invokes specialist, exchanges direct messages, receives result; denied permission and one-slot exhaustion handled without deadlock |
| native_codex_uses_fabric_mcp | Real Codex session discovers utility.echo, invokes it through MCP, retrieves the completed result, and returns JSON independently verified against Fabric's durable record |
| readiness_uses_assignment_mcp_without_context_files | Real Registry classifier receives input/configuration and completes through assignment MCP without context files |
| native_registry_agent_uses_assignment_mcp_and_yaml_runtime | Real Registry launcher, native preparation, and YAML provider/model selection using a model-free Pi fixture |
| Existing Factory regressions | Full Controls go test ./... and go vet ./... |
| Skill | skill-creator quick_validate.py; skill embedded and provisioned in managed invocations |

The current deterministic suite has ten integration cases, including real
Registry readiness and a native Registry launcher with a model-free Pi fixture.
The original FAB-009 verification also passed one real Codex MCP scenario; that
paid-model operator smoke was not rerun for the shared-assignment migration.
Rust tests/strict clippy, demo validation, and the release build pass.

## Scope of this evidence

The native test proves the operator-to-MCP-to-supervisor-to-comms-to-result path.
Reviewer and specialist fixtures exercise actual separate processes and identities
but deliberately use deterministic implementations; these are protocol tests, not
evidence of model review quality. The Go Registry packages now use shared assignment MCP. The model-free Pi fixture
checks runtime preparation and protocol behavior, not live model review quality.

Restart testing covers Fabric observation recovery while Controls remains active.
Controls does not transparently restore lost PTYs or model memory after supervisor
loss. Its interrupted session state is not proof that an orphaned host process
stopped. Inspect host process state before resuming work after supervisor loss.
Capacity is based on admitted calls and supervisor-reported state; external peers
own their processes and resource enforcement.

Unknown launch outcomes remain reserved and are never automatically relaunched;
manual investigation is required if no session can be recovered. Automatic
resolution of this ambiguity, remote deployment, process sandboxing, worker pools,
and a Rust rewrite of comms remain outside this implementation.

Native configuration references:
[official MCP configuration](https://learn.chatgpt.com/docs/extend/mcp?surface=cli)
and [non-interactive Codex](https://learn.chatgpt.com/docs/non-interactive-mode).
The installed CLI help was also checked before running the test.

## Local stack lifecycle

After `./scripts/build.sh`, run `python3 tests/local-stack.py`. This uses a private
temporary project and the actual paired binaries. It verifies managed PostgreSQL
and Controls startup, duplicate-start rejection, structured invocation, Ctrl-C,
result recovery after restart, and SIGTERM shutdown. It needs no database URL
or model credentials.

## Separate service HTTPS integration

After `./scripts/build.sh`, run `python3 tests/network-stack.py`. It starts
Controls and Fabric in separate processes/state directories, with separate HTTPS
proxies and a private CA. It checks verified TLS, host credential rejection,
host-side invocation file staging and environment lookup, reviewer-to-specialist
delegation, worker privilege boundaries, and official MCP SDK discovery, calls,
registration, caller isolation, and follow-up messages through the network adapter.
It does not provision multiple physical machines; the test exercises the network
and filesystem ownership boundaries using isolated directories on one machine.
`remote_paths_belong_to_the_execution_host` separately verifies that configurations
can refer to paths absent from the Fabric machine. Local lifecycle tests use
authenticated TCP and check that PostgreSQL stops with the managed stack.

## Shared assignment migration (FAB-012 through FAB-017)

The integration harness now runs ten cases, including the real Registry readiness
program over assignment MCP and a real architect launcher with YAML-selected Pi
preparation. The native protocol fixture is model-free: it verifies profile/model
selection, assignment lookup and completion without making a paid model call.
Controls also runs readiness through its actual Factory command runner, checking
that legacy input/config/prompt files are absent. Registry tests preserve its
legacy replay behavior and verify common output/error handling.

The separate-service HTTPS test additionally covers MCP-only readiness and a
native worker delegating a child through `fabric mcp --assigned`, preserving
parent identity without an operator credential or context file. Pure protocol
tests cover ownership rejection, output validation, immutable provisioning,
unchanged completion replay, conflicting completion rejection and Factory errors.
Native Pi adapter tests cover both common assignment MCP and legacy delivery.
