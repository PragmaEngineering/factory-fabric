# Capability package contract

Each invocation launches one configured command through Controls. `path` resolves
from fabric.yaml; `entrypoint` stays within that package. Arguments are literal
except `{prompt}`. Working directory defaults to the configuration directory and
can be selected explicitly for each managed invocation. `environment` maps target
variable names to execution-host environment variable names. Provider credentials remain in
the environment; `overrides` is package-interpreted configuration, never a place
for literal secrets. Host processes retain the host user's privileges.

## MCP assignment mode

Declare `protocol: mcp`. The process receives only its scoped connection bootstrap,
`AGENT_MCP_URL` and `AGENT_MCP_TOKEN`. These are supplied by the host, not manually
maintained per-invocation settings. `assignment_get` supplies input, configuration,
instructions, identity/thread, working/output directories, and the pinned output
schema. `assignment_status` reports running/waiting; `assignment_complete` accepts
one output or error and returns a durable receipt. The same contract is used by
Factory and Fabric. Agents do not choose a workflow-specific completion payload.

For native Pi packages, declare `runtime: {name: pi, version: '>=1.0.0 <2.0.0'}`. Controls
uses its existing adapter to check the executable, create the native private
profile, and apply YAML provider/model overrides. The shared Pi bridge loads the
assignment through MCP and configures communication tools. No generated input,
prompt or context files are required. Native profile files are harness artifacts,
not a second assignment contract. `runtime.executable` can explicitly select the
installed native executable. Tools and package source must exist on the host.

`allow_delegation: true` permits child calls. The runtime configures the Fabric
MCP adapter with `--assigned`, using the same scoped identity and API assignment
rather than context files or an operator credential. Install the Fabric binary on
the execution host when enabling that adapter. Parent/root attribution and capacity
rules are unchanged.

## Legacy compatibility

`protocol: legacy` retains the old invocation files and `fabric.result` completion
contract below. Omitted protocol remains legacy for existing manifests and stored
invocations. New packages should declare MCP explicitly.

The private invocation directory contains input.json, config.json, prompt.txt,
context.json, and the embedded Fabric skill. Standard paths are passed through
`FACTORY_INPUTS_PATH`, `FACTORY_PACKAGE_CONFIG`, `FACTORY_PROMPT_PATH`, and
`FACTORY_INVOCATION_DIR`. `FABRIC_CONTEXT_PATH` locates the context;
`FABRIC_SKILL_PATH` locates the provisioned skill. The context contains the
invocation ID, pinned capability, assigned thread, result recipient, Fabric endpoint,
and Controls connection (URL, token, participant ID). Do not print the connection.

A native runner can configure a stdio MCP entry with the Fabric binary and args
`["mcp", context.fabric_endpoint, "--context", FABRIC_CONTEXT_PATH]`. Append the
provided prompt and load the provisioned skill using that runner's supported
mechanisms. The package owns this native configuration; the supervisor only
launches the prepared command. Registry Go packages now declare MCP mode and do not use this compatibility path.

Return through Controls send_message with payload kind `fabric.result`, version 1,
and a data part `{invocation_id, output}` or `{invocation_id, error}`. Address the
context's result recipient. Only the assigned implementation's result is accepted.
Inputs are validated before dispatch; outputs before success. The result and
process lifecycles remain separate. The current comms envelope limit is 64 KiB;
use references for large artifacts instead of embedding their contents.

Connected implementations register their own capability contracts with a renewable
lease, consume the returned mailbox, and receive `fabric.invoke` messages. Those
carry the input, pinned contract, invocation thread, ID, and result recipient.
`fabric.cancel` requests cancellation. Reply with completion/error on the invocation
thread to acknowledge the end of work; receipt of cancellation alone is not proof
of completion. Renew leases before expiry. Withdrawal/expiry blocks new work and
fails unfinished calls with external process state explicitly unknown.

Question and answer messages are ordinary communication, not workflow decisions.
They do not mutate accepted input. Complete or explicitly cancel required delegated
calls before completing your own work. Parent completion does not cancel children.

## Execution-host paths

With `controls_endpoint`, package and working-directory paths must be absolute
paths on the Controls host. Fabric does not read or canonicalize them locally.
MCP-mode inputs, configuration, and instructions are fetched through the
authenticated assignment API. Legacy mode stages its context and assignment files
through the host workspace API. Package source and project checkouts must already be installed
on that host. Environment references are resolved by Controls at launch, so host
credentials need not pass through Fabric. See [network deployment](network.md).

Pi compatibility belongs to Controls: stable Pi 1.x (`>=1.0.0 <2.0.0`) is accepted, intersected with the package constraint at runtime preparation. Fabric forwards `runtime.version` unchanged; offline configuration validation does not probe the execution host or establish version compatibility. An omitted version uses the adapter policy. Other majors and prereleases are rejected by Controls. Exact session versions remain recorded, and Factory retries cannot silently change them.
