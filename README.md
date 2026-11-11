# Agent Fabric

Rust capability discovery and asynchronous delegation built on Controls comms and
session supervision. Factory workflows remain optional and separate.

```sh
cargo test
cargo run -- validate fabric.example.yaml
cargo run -- catalog fabric.example.yaml
```

See [issues.md](issues.md) for implementation status.

## Configuration contract

`fabric.yaml` version 1 reuses package `name`, `path`, `entrypoint`, `args`, and
`overrides` conventions. Paths resolve from the configuration file; entrypoints
must stay inside their package. Packages can supply their own native profiles,
tools, and skills. This is not full factory.yaml compatibility: workflow routes,
hooks, variables, native runtime requirements, and declarative loops are rejected.
Native adapter preparation will need an explicit supported boundary; raw session
launch alone does not prepare Pi profiles.

Capabilities use JSON Schema 2020-12, local references only. Each accepted call
pins its version, schemas, and package settings; changes apply to future calls.
`environment` maps target variable names to names in the host environment; secret
values belong in the environment, never YAML. `overrides` is package-interpreted
JSON. Invocation and process session states have separate owners: Fabric owns
invocations, Controls owns sessions. Neither is a workflow step.
