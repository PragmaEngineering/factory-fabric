---
name: fabric
description: Discover capabilities, delegate structured work, and communicate with agents through an available Agent Fabric MCP connection. Use for opportunistic collaboration without a configured Factory workflow.
---

# Fabric

Use Fabric within the user's task and the harness's delegation policy. Delegate
when a discovered specialist, independent check, or separable task is likely to
help. Keep small tasks local when delegation adds more overhead than value.

## Discover and invoke

Use `agents_list` to find capabilities and availability. Inspect the relevant
contract with `agents_describe`; do not assume a name, input shape, or result shape.
An on-demand capability can be available before an instance is running. A connected
implementation needs an unexpired lease; discovery does not guarantee admission.

Call `agents_invoke` with the discovered package/capability, structured input, and
a unique `request_key`. Provide the task context and evidence the capability needs.
The accepted response supplies an invocation ID; keep it while doing independent
work. Invocations pin the accepted version and schemas.

Use `invocations_get` for a snapshot or `invocations_wait` for a bounded wait.
Successful submission is not completed work. A completed result satisfies the
schema, but still needs assessment against the user's objective and relevant checks.
MCP values appear in `structuredContent.result` and the text content.

## Communicate

The invocation's `running.thread_id` identifies its conversation. Existing Controls
operations are exposed as `comms_*` tools with their canonical schemas. Inspect
participants before addressing a peer; use the peer's ID, not its display name.
Send useful context, questions, findings, and answers directly. Reference the
invocation and use `in_reply_to` for a specific message when appropriate.

Reads and acknowledgements are separate; follow the advertised cursor/ack fields.
Reading or delivering a message does not prove it was acted on. Use bounded waits
or native delivery rather than repeated model-driven inbox checks. A clarification
message does not rewrite the accepted input or automatically change invocation
status; submit a distinct call if the contract itself must change.

## Handle asynchronous work

- Retry an uncertain submission with the same key and unchanged arguments. A new
  key can start duplicate work. Conflicting key reuse is rejected.
- `capacity_exhausted` means the call was not accepted. Continue useful work or
  retry later; avoid a tight retry loop. A parent occupying the last slot cannot
  queue a child and wait indefinitely.
- `launch_uncertain` reserves capacity and requires reconciliation. Inspect the
  recorded error and supervisor state; do not automatically submit another call.
- Request cancellation with `invocations_cancel`, then inspect status. `cancelling`
  is a request, not proof of termination. Connected implementations acknowledge
  through completion; lease expiry can leave external process state unknown.
- Resolve required child work before finishing your own assignment. Parent
  completion does not automatically cancel children.

Separate model contexts do not isolate filesystem edits. Use explicit invocation
working directories or prepared worktrees when parallel edits would conflict.
Do not change shared repository state merely because another invocation is active.

## When implementing an assigned capability

If `FABRIC_CONTEXT_PATH` is supplied, read that private JSON context and the input
at `FACTORY_INPUTS_PATH`. Use its connection and assigned thread. A native harness
can connect through `fabric mcp ENDPOINT --context CONTEXT_PATH`; use
`context.fabric_endpoint`. Execution paths and environment references belong to the
Controls execution host; do not assume shared filesystems. Keep credential contents out of prompts, output, and logs.

Send completion through `comms_send_message`, addressed to `result_recipient`,
with a new `operation_id` and this payload:

```json
{
  "kind": "fabric.result",
  "version": 1,
  "parts": [{
    "kind": "data",
    "data": {"invocation_id": "ASSIGNED_ID", "output": {}}
  }]
}
```

Replace `output` with a value matching the assigned output schema. For a failure,
use `error` instead of `output`, with a factual explanation. Wait for the comms
receipt. Printed JSON, process exit, and a workflow `step_result` do not complete
Fabric work. Reserved workflow control payloads are unavailable to Fabric peers.

If delegation is enabled, invoke specialists using this same identity. Fabric
infers managed workers' parent; connected implementations supply `parent_id`.
Exchange follow-up messages directly. A permission denial is a boundary: report
it or continue within scope; do not attach as an operator to bypass it.
