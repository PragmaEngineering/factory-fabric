# Network deployment

Controls is the execution host. Fabric holds the capability catalog and invocation
history. The MCP adapter runs wherever the coding harness runs. These may be three
separate machines; none requires another machine's filesystem.

```text
Coding harness → stdio MCP adapter → HTTPS Fabric API
                                      ↓ HTTPS Controls host API
                                      ↓
                                 supervised agents
Agents and Fabric ↔ Controls' existing authenticated communication MCP API
```

## Endpoints and credentials

Services bind HTTP origins to loopback. For remote access, put an HTTPS reverse
proxy on each service host, forwarding the complete path and preserving `Host`.
This keeps TLS certificate management in the deployment layer. Externally
advertised URLs must be HTTPS; plain HTTP is accepted only for loopback clients.
Clients verify certificates, disable redirects, and do not send credentials
through environment-configured proxies. For private certificate authorities, set
`FABRIC_CA_CERT` to a PEM CA file on each machine using the Rust client.
Native package clients need their own trust configuration too.

The Controls host credential grants process-launch and identity-provisioning
authority. The Fabric operator credential can attach operator identities. Keep
both in secret files/environment references, not YAML or URLs. Use random secrets
of at least 32 characters. Distribute the Controls credential only to Fabric;
distribute the Fabric credential only to trusted operators. These are service
administrator credentials, not separate user accounts. Workers receive only their
existing Controls participant credential and cannot attach operator identities.
The services reject browser Origin headers. This is a trusted execution service,
not a process sandbox.

## Controls host

Install `factory-runtime` and the capability packages on the execution machine.
Provide the host credential in `/run/secrets/controls-token`, then start:

```sh
factory-runtime --root /var/lib/fabric-controls --local-database \
  --listen 127.0.0.1:7331 --public-url https://controls.example.com \
  --token-file /run/secrets/controls-token
```

Point the HTTPS proxy at `127.0.0.1:7331`. All management routes require the host
credential. `/mcp` uses existing per-participant credentials instead. An external
database can replace `--local-database` using `FACTORY_DATABASE_URL`.

## Fabric host

A configuration for packages installed on Controls might contain:

```yaml
version: 1
name: development
controls_endpoint: https://controls.example.com
controls_token_env: CONTROLS_TOKEN
fabric_endpoint: https://fabric.example.com
packages:
  - name: reviewer
    path: /opt/agents/reviewer
    entrypoint: run.sh
    working_directory: /srv/project
    environment:
      PROVIDER_API_KEY: REVIEWER_PROVIDER_KEY
    capabilities:
      - name: review
        description: Review supplied changes.
        version: "1"
        input_schema: {type: object}
        output_schema: {type: object}
```

`path`, `entrypoint`, and `working_directory` are interpreted on Controls.
`REVIEWER_PROVIDER_KEY` is read from the Controls process environment. Fabric
validates contracts and path structure without checking the remote filesystem.
The package and checkout must already exist there; there is no implicit package
or repository synchronization. Invalid execution paths fail through supervision.

Set `CONTROLS_TOKEN` from the Controls secret in Fabric's service environment.
Provide a different operator secret at `/run/secrets/fabric-token`, then run:

```sh
FABRIC_LISTEN=127.0.0.1:7332 \
FABRIC_API_TOKEN_FILE=/run/secrets/fabric-token \
fabric serve /etc/fabric/fabric.yaml /var/lib/fabric
```

Point Fabric's HTTPS proxy at `127.0.0.1:7332`. The configured `fabric_endpoint`
must be reachable from the execution host, allowing peer delegation and follow-up
calls without the operator coordinating them. MCP-mode agents retrieve assignments through Controls' authenticated MCP API.
The workspace-file API remains for explicit legacy packages; execution still uses
the existing supervisor.
Fabric's SQLite history remains on the Fabric machine; communication/session
records and invocation files remain on Controls.

## Coding harness machine

Set `FABRIC_TOKEN` to the operator credential in the MCP process environment.
Configure the stdio adapter command as:

```json
{
  "command": "/path/to/fabric",
  "args": ["mcp", "https://fabric.example.com", "--identity", "development"]
}
```

The CLI accepts the same URL: `fabric call https://fabric.example.com catalog '{}'`.
No socket or shared state directory is required on this machine.

## Local convenience and compatibility

`fabric up` without `controls_endpoint` starts private local services, generates
credentials, and records endpoint discovery in `.fabric/endpoint.json`. It uses
the same authenticated network APIs. MCP assignments stay in the API; legacy
packages retain host-side file staging. With `controls_endpoint`, it starts
only Fabric and connects to the remote host. Set `FABRIC_LISTEN` to the fixed
loopback origin used by your HTTPS proxy in that deployment.

Existing explicit Unix socket commands remain supported. An existing local
`.fabric` identity is migrated when it belongs to the same local Controls root.
An active invocation pinned before transport migration may retain an old socket
context; finish that work before switching deployments. Changing the configured
Controls endpoint requires a new Fabric state directory unless it is the managed
local identity. Persisted calls are not silently moved to another execution host.

New packages should declare `protocol: mcp`; native Pi packages also declare
`runtime: {name: pi, version: '=1.0.3'}`. Node agents using private CAs need a
configured trust root (for example `NODE_EXTRA_CA_CERTS` in their execution-host
service environment). Standard public CA certificates need no custom trust setup.
