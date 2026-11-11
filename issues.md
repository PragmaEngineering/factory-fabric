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

- [ ] Complete

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

- [ ] Complete

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

- [ ] Complete

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

- [ ] Complete

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

- [ ] Complete

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

- [ ] Complete

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
