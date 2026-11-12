# Running Fabric locally

Build once from the Fabric checkout (requires Rust and Go; Controls is the sibling checkout):

```sh
./scripts/build.sh
```

Start everything in one terminal:

```sh
bin/fabric up fabric.example.yaml
```

This starts a private PostgreSQL database, Controls' communication/session server,
and Fabric. No workflow engine is started. The first run may download PostgreSQL.
Wait for “Fabric listening”. Ctrl-C stops the stack and its active agents.
Run the same command again to reuse stored conversations and invocation history.
All project state and logs live in `.fabric/` beside the configuration; keep it.
Local services use authenticated HTTP endpoints, recorded in `.fabric/endpoint.json`.
Controls logs are `.fabric/controls.log`; database logs are
`.fabric/controls/postgres.log`.

For your own project, copy the example to `fabric.yaml`, define your packages,
and run `fabric up`. Omit `controls_socket`: local startup supplies the default.
The two binaries in `bin/` must stay together; add that directory to PATH if desired.

## Connect your coding agent

Use a stdio MCP server with absolute paths (replace `/path/to/project`):

```json
{
  "command": "/path/to/fabric/bin/fabric",
  "args": ["mcp", "/path/to/project/.fabric/endpoint.json", "--identity", "development"]
}
```

Load `skills/fabric/SKILL.md` through your harness's skill mechanism. Ask the agent
to discover available capabilities and delegate work. The MCP connection may close
without stopping Fabric. Give independent operators distinct identity names;
reusing a name recovers that operator's earlier work.

## Try it without a coding agent

In another terminal, from the directory containing the configuration:

```sh
bin/fabric call catalog '{}'
bin/fabric call invoke '{"request_key":"example-1","package":"utility","capability":"echo","input":{"task":"inspect"}}'
bin/fabric call wait '{"id":"RETURNED_INVOCATION_ID","seconds":20}'
```

The default endpoint is read from `.fabric/endpoint.json` in the current directory.
Supply an HTTPS URL or an endpoint file before the method when calling elsewhere.
For the peer collaboration fixture, start `bin/fabric up fabric.collaboration.yaml` instead,
then invoke package `reviewer`, capability `review`, with input `{}`.

## Go agent catalog

`fabric.demo.yaml` mirrors all five packages in Registry's Go factory: readiness,
architect, architecture_reviewer, coder_golang, and code_reviewer_golang. It uses
relative registry paths and preserves the factory's provider overrides. The default
working directory is the sibling Controls Go checkout; change it for your project.
Inspect it with `bin/fabric catalog fabric.demo.yaml`.

These are native registry launchers, not the deterministic fixtures. They still
need Fabric MCP/completion integration before successful structured invocation.
The readiness launcher currently requires Factory workflow runtime variables;
Pi agents retain native profiles, and Fabric does not apply Factory's Pi preparer
or translate its provider overrides. No workflow, approval gates, or worktree hooks
are imported. Use `fabric.collaboration.yaml` for the runnable collaboration demo.

## Separate machines

Controls, Fabric, and the coding harness can run on separate machines. Connect
using HTTPS endpoints and bearer credentials; see [network deployment](network.md).
Package paths and environment references belong to the Controls execution host.
Invocation inputs, context, prompts, and skills are transferred through its API.
Packages themselves must be installed on that host.

`fabric up` with `controls_endpoint` connects to that host and starts only Fabric.
Explicit `serve` runs Fabric against existing infrastructure. Legacy Unix socket
transport remains available for existing installations and compatibility tests.

See [recovery and ownership](recovery.md) for failure states and restart limits,
and [verification](verification.md) for tested behavior.
