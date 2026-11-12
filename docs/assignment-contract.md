# Shared agent assignment contract

## Audit (FAB-012)

Controls already owns authenticated participants, thread membership, durable messages,
receipts, session supervision, and Factory result effects. These remain authoritative.
Fabric's registry already supports leased connected implementations; it must not be
replaced by a second registry. Factory's runner already binds input/configuration
and selects a native adapter. Reuse these boundaries.

The current incompatibilities are at the process/agent boundary:

| Concern | Factory today | Fabric today | Target |
| --- | --- | --- | --- |
| Assignment | input/config/prompt files | staged input/config/context files | authenticated MCP lookup |
| Connection | FACTORY_MCP_* variables | FABRIC_CONTEXT_PATH document | endpoint + participant credential only |
| Result | SDK discovers controller; step_result | fabric.result to configured peer | assignment_complete |
| Runtime | adapter selected from YAML | raw command only | explicit YAML protocol/runtime selection |
| Native profile | prepared by Controls | implicit launcher defaults | YAML settings applied by runtime |
| State | workflow result/effect | invocation result/state | common receipt; owner interprets result |

`task-readiness/run.mjs` requires Factory runtime variables before calling its
reusable evaluator. `result-client.mjs` assumes a controller-kind participant.
`pi-rpc-session.mjs` assumes workflow-specific completion and runtime-status
payloads. Fix those shared boundaries; do not fork each agent implementation.

Provider keys remain deployment secrets referenced by YAML. They are not sent in
assignment documents or model prompts. Project files remain on the execution host.
No workflow engine, new identity system, message broker, or implicit package sync.

## Implementation sequence

1. Audit and record reuse boundaries.
2. Specify the authenticated MCP assignment operations and durable retry semantics.
3. Make protocol/runtime bootstrap an explicit package setting in YAML.
4. Implement the shared Controls operations, SDK, and Factory/Fabric producers.
5. Migrate readiness and prove both producers use the same implementation.
6. Migrate the shared Pi adapter; update skill, discovery, operation docs and tests.
