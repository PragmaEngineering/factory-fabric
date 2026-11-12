# Agent Fabric implementation issues

Issues are tracked below. Complete acceptance criteria before marking an issue done.
Work in order unless a dependency explicitly permits independent progress.

## Objective and boundaries

Fabric lets existing agent sessions discover capabilities, submit structured work
asynchronously, and communicate directly with agents handling that work.

- Fabric-specific code lives here and uses Rust.
- Reuse Controls' Go comms implementation through supported APIs. Do not rebuild
  messaging, routing, identity, persistence, or supervision.
- Preserve Factory workflows. Fabric operates without Hatchet.
- Reuse package conventions and runner adapters where applicable.
- Architecture decision: reuse Controls session supervision for agent processes.
  Rust Fabric owns capability discovery, registration, and invocation management;
  Controls supplies comms and process supervision without starting Hatchet.
- Capabilities may be implemented by agents or deterministic programs.
- Start with one supervised instance per invocation and bounded concurrency.
- Do not require workflows, DAGs, or declarative review loops.

## FAB-001 — Run Controls comms and supervision without the workflow engine

- [x] Complete

Controls commit: `81d0175`. Workflow-free `factory-runtime`, private scope/peer
provisioning API, existing supervisor API, and messaging isolation. Full Controls go test ./... and go vet ./... pass. Standalone mode
requires an explicit PostgreSQL URL; it does not start Hatchet.

Location: primarily `../controls`.

Inspect startup, storage, provisioning, authorization, completion, and supervision
before changing them. Add standalone comms and supervisor startup without initializing Hatchet. Allow
communication scopes and authenticated participants without workflow step attempts.
Separate messaging from workflow-specific effects while preserving Factory behavior.

Acceptance:
- Start and stop comms and supervision without a workflow engine or execution.
- Reach the existing session launch, inspection, output, and stop API in this mode.
- Provision participants through a supported interface.
- Two clients create a thread, send addressed messages, read, acknowledge, and wait.
- Reconnection preserves messages, cursors, and idempotency receipts.
- Scope isolation, participant retirement, and existing workflow tests still pass.

## FAB-002 — Define contracts, fabric.yaml, and the Rust application

- [x] Complete

Rust config loader, JSON Schema 2020-12 validation, catalog CLI, example,
project toolchain, and tests implemented. cargo test and strict clippy pass.

Depends on FAB-001 for final communication boundaries; configuration work may
proceed independently.

Define packages/agents, capabilities, instances, invocations, and communication
scopes. Reuse Controls/Registry conventions and document differences from
factory.yaml. Capabilities declare names, descriptions, versions, and input/output
schemas. Configuration covers launch settings, environment references, working
directories, resource limits, and concurrency. Do not embed credentials.

Acceptance:
- Rust CLI validates an example configuration and displays its capability catalog.
- Example includes multiple capabilities and a deterministic implementation.
- Invalid configuration, duplicate identities, and unsupported schemas fail clearly.
- Document schema dialect, versioning, and pinning contracts on accepted calls.
- No steps, routes, stages, or workflow engine are required.

## FAB-003 — Expose workflow-independent supervised execution

- [x] Complete

Rust Controls/MCP clients, private invocation context, supervised command launch,
structured comms completion, schema enforcement, cleanup, and cross-language
integration harness implemented. Real PostgreSQL/Controls integration tests pass.
Existing session API needed no change. Native workflow adapters require an explicit
Fabric completion adapter; raw command integration is supported first.

Depends on FAB-001 and FAB-002. Location: Controls API and Fabric Rust client.

Inspect and reuse existing supervision and adapters. Expose the smallest supported
API for provisioning, launch, inspection, cancellation, and result collection
without workflow steps. Fabric owns capability invocation state; Controls owns process session state.
Define their mapping and reconciliation without competing authoritative records.

Acceptance:
- Fabric launches a deterministic package through the supported boundary.
- Package receives structured input and scoped comms credentials.
- Structured completion does not require a workflow step_result.
- Launch failure and exit without a result produce explicit failures.
- Fabric never reads Controls tables or depends on Go internal types.
- Document adapter compatibility and required integration changes.

## FAB-004 — Add asynchronous invocation and bounded concurrency

- [x] Complete

Private Rust service, durable invocation records, idempotent submissions, capacity
rejection, process-confirmed cancellation/completion, and restart reconciliation
implemented. Three real Controls/PostgreSQL integration scenarios plus unit tests
and strict clippy pass. Unknown launch outcomes retain capacity and are not retried.

Depends on FAB-003.

Return an invocation ID immediately; persist requests, pinned contracts, status,
results, and errors through the agreed owner. Validate input before dispatch and
output before successful completion. Support idempotency, status/result retrieval,
cancellation, capacity limits, and explicit queue-or-reject behavior. Clarification
messages reference invocations without silently rewriting accepted inputs.

Acceptance:
- Two invocations execute concurrently with independent inputs and results.
- Retried submission keys do not duplicate work; conflicting reuse is rejected.
- Invalid input starts no process; invalid output cannot report success.
- Capacity limits are enforced with clear caller-visible behavior.
- Cancellation reaches supervision; requested and confirmed cancellation are distinct.
- Restart preserves results and accurately reconciles interrupted work.
- Document lifecycle, clarification behavior, and result retention.

## FAB-005 — Register and discover capabilities

- [x] Complete

Unified static/live catalog, connected registration, comms mailboxes, lease renewal,
withdrawal, ownership checks, and pinned external invocation contracts implemented.
Four real Controls integration scenarios pass; strict Rust clippy passes. Controls
adds private identity inspection/authentication, reusing existing peer credentials.

Depends on FAB-002 and FAB-004.

Unify configured on-demand capabilities and authenticated connected implementations
in one catalog. Reuse comms identities. Distinguish definitions, instances,
availability, and capacity. Define registration ownership, stale availability,
disconnect behavior, withdrawal, and contract updates.

Acceptance:
- Inspect descriptions, versions, input/output schemas, and availability.
- Configured packages are discoverable before launching a process.
- Connected implementations register and withdraw through a supported API.
- New registrations appear without changing or restarting the MCP server.
- Stale presence never implies readiness; active calls retain accepted contracts.

## FAB-006 — Connect existing agent sessions through MCP

- [x] Complete

Authenticated session attachment, generic stdio MCP tools, bounded waiting, and
canonical comms-tool forwarding implemented. Official MCP TypeScript client tests
verify asynchronous calls, caller isolation, dynamic discovery, and two-way messages.
All five real Controls/PostgreSQL integration scenarios pass.

Depends on FAB-004 and FAB-005.

Expose generic discovery, description, invocation, status, and result tools. Attach
external operator sessions using scoped identities. Reuse comms operations for
sending, reading, and waiting; decide between forwarding and a companion connection
without introducing another messaging path. Provide bounded waiting or another
explicit mechanism for observing asynchronous completion.

Acceptance:
- Codex connects without being launched in a Factory workflow.
- It discovers a capability, inspects its schema, and submits structured input.
- It continues other work, then retrieves status and validated results.
- It exchanges follow-up messages with the implementation.
- Caller identity, scope, and invocation permissions are enforced.
- Adding capabilities does not require new MCP tool definitions.

## FAB-007 — Support agent-to-agent capability delegation

- [x] Complete

Managed workers delegate under their own comms identities with pinned permission,
parent/root attribution, and shared capacity limits. Reviewer/specialist process
fixtures exchange questions and answers directly, then return through the caller.
Integration tests cover successful delegation, denied permission, and a one-slot
capacity rejection handled without deadlock. All six integration scenarios pass.

Depends on FAB-006.

Allow participating agents to discover and invoke permitted capabilities directly.
Record caller, parent invocation where relevant, and result recipient. Define
further-delegation permission using existing authorization. Apply shared capacity
limits to agent-originated calls.

Acceptance:
- Reviewer invokes a specialist while its own invocation remains active.
- Reviewer and specialist exchange messages directly.
- Specialist returns to reviewer; reviewer returns to Codex.
- Unauthorized delegation is rejected and attribution remains inspectable.
- Capacity policy handles child invocations without silent deadlock; test it.
- Codex need not relay peer traffic and no workflow engine participates.

## FAB-008 — Create the Fabric skill and operator documentation

- [x] Complete

Reusable skill, operator/package guides, and runnable reviewer/specialist example
added. The binary embeds and provisions the skill per managed invocation. Skill
validator, both example configurations, Rust tests, strict clippy, and all six
integration scenarios pass. Global harness configuration was not modified.

Depends on FAB-006 and FAB-007.

Teach discovery, contract inspection, opportunistic delegation, structured calls,
follow-up communication, and asynchronous result retrieval. Cover capacity errors,
uncertain-write retries, clarification, cancellation, and evaluation of returned
results. Reuse canonical communication instructions rather than duplicating rules.

Acceptance:
- Document startup, configuration, connection, and end-to-end delegation.
- Skill encourages natural delegation without prescribing a workflow.
- Explain that context isolation does not isolate filesystem edits; document explicit
  working directories or worktrees for concurrent editing when appropriate.
- Document repository ownership boundaries and current limitations.

## FAB-009 — Verify the complete integration

- [x] Complete

Eight deterministic integration scenarios and one real Codex MCP smoke test pass.
Full Controls tests/vet, Rust tests/strict clippy, skill validation, example
configuration checks, and release build pass. See docs/verification.md for commands,
versions, observed outcomes, and explicit limitations. Final fault checks also
corrected stale comms endpoint recovery and definite-launch identity cleanup.

Depends on FAB-001 through FAB-008.

Use deterministic fixtures for protocol/lifecycle tests, then a real coding-session
demonstration. Retain versions, configuration, commands, and outcomes.

Acceptance:
- Start standalone comms and Rust Fabric without Hatchet.
- Discover multiple capabilities from an existing Codex session.
- Execute concurrent calls and retrieve schema-validated results.
- Demonstrate clarification, follow-up messages, and reviewer-to-specialist delegation.
- Exercise invalid schemas, duplicate requests, process failures, cancellation,
  capacity exhaustion, disconnects, and restart reconciliation.
- Verify existing Factory workflows still function.
- No replacement communication framework or workflow engine was introduced.

## Deferred

Declarative loops, orchestration policies, persistent worker pools, session reuse,
automatic retries, federation, distributed deployment, and rewriting comms in Rust.

## FAB-010 — Simplify local operation

- [x] Complete

`fabric up [fabric.yaml]` owns the foreground local stack: private PostgreSQL,
Controls, and Fabric. One build script produces adjacent binaries. Local socket
and state paths have defaults; CLI calls can omit the socket. Ctrl-C/SIGTERM
stop the stack; project state survives restart. External-service mode is retained.
The operator guide and README lead with this path; recovery details are separate.

Verification: `python3 tests/local-stack.py` covers first start, duplicate
rejection, structured execution, Ctrl-C, persisted result recovery, and SIGTERM.
Rust tests/clippy and Controls runtimehost tests/vet pass.

## FAB-011 — Remove required co-location

- [x] Complete

Controls exposes authenticated network host operations and its existing
participant-authenticated communication MCP endpoint. Fabric's RPC service and
MCP/CLI clients accept authenticated endpoints. Remote access uses verified HTTPS
through a deployment TLS proxy; local origins use authenticated loopback HTTP.
The local `up` command uses the same endpoint APIs and records private discovery.
Explicit Unix transport remains for compatibility, rather than being required.

Inputs/context/prompts/skills are staged through Controls, package/working paths
are explicit execution-host paths, and environment references resolve on that
host. Packages/checkouts are installed there independently. Workers receive their
participant credential and a reachable Fabric endpoint, not service admin tokens.

Verification: separate-service HTTPS integration (including official MCP SDK and
peer delegation), managed local lifecycle/restart, all eight existing runtime
integrations, Rust tests/clippy, full Controls tests/vet, focused network auth and
workspace tests, and skill validation. See `docs/network.md`.

## FAB-012 — Audit agent invocation boundaries

- [x] Complete

Audit recorded in `docs/assignment-contract.md`; existing comms identity, messages,
supervision, registry leases, and workflow effects are retained.

## FAB-013 — Specify one agent-facing MCP assignment contract

- [x] Complete

Implement in order against `docs/assignment-contract.md`; verify observable behavior
and commit the completed issue before continuing.

## FAB-014 — Declare protocol and runtime bootstrap in YAML

- [ ] Complete

Implement in order against `docs/assignment-contract.md`; verify observable behavior
and commit the completed issue before continuing.

## FAB-015 — Implement shared Controls assignment MCP and SDK

- [ ] Complete

Implement in order against `docs/assignment-contract.md`; verify observable behavior
and commit the completed issue before continuing.

## FAB-016 — Migrate readiness to the shared assignment contract

- [ ] Complete

Implement in order against `docs/assignment-contract.md`; verify observable behavior
and commit the completed issue before continuing.

## FAB-017 — Migrate native agents and simplify operation

- [ ] Complete

Implement in order against `docs/assignment-contract.md`; verify observable behavior
and commit the completed issue before continuing.
