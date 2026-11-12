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
  "args": ["mcp", "/path/to/project/.fabric/fabric.sock", "--identity", "development"]
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

The default socket is `.fabric/fabric.sock` in the current directory. Supply an
explicit socket before the method when calling from elsewhere.
For the peer collaboration fixture, start `bin/fabric up fabric.demo.yaml` instead,
then invoke package `reviewer`, capability `review`, with input `{}`.

## Existing infrastructure

If you already operate PostgreSQL and Controls, retain the separate-service mode:

```sh
FACTORY_DATABASE_URL=postgres://... factory-runtime --root /private/controls
fabric serve /path/to/fabric.yaml /private/fabric-state
```

Set `controls_socket` in that configuration to `/private/controls/factory.sock`.
In this mode Ctrl-C on Fabric leaves Controls and its supervised agents running.
See [recovery and ownership](recovery.md) for failure states and restart limits,
and [verification](verification.md) for tested behavior.
