# Deploy Platform Node Agent Guide

The Deploy Platform Node Agent (`platform-agent`) runs on each worker node to manage container releases, report resource capacity, execute health checks, and forward runtime metrics/logs to the control plane.

## Overview

- **Authentication**: All RPC commands sent from the scheduler to the node agent are packaged in an `AgentEnvelope` signed with HMAC-SHA256 using the node's shared secret key.
- **Deduplication**: Every RPC envelope contains a unique `operation_id` to prevent replay attacks and duplicate release allocations.
- **Health Gating**: The agent exposes `/health` and `/agent/rpc` endpoints.
- **Graceful Draining**: Upon receiving SIGTERM/SIGINT or an operator drain command, the agent stops accepting new releases and gracefully waits for existing traffic to complete drainage.

## Configuration & Environment Variables

| Variable | Flag | Description | Default |
| --- | --- | --- | --- |
| `NODE_ID` | `--node-id` | Unique UUID assigned to the node | Required |
| `NODE_SECRET_KEY` | `--secret-key` | Hex-encoded HMAC-SHA256 secret key | Required |
| `LISTEN_ADDR` | `--listen-addr` | IP and port for the agent HTTP server | `0.0.0.0:9090` |

## Installation & Running

```bash
# Build the node agent binary
cargo build --release --bin platform-agent

# Run the agent
./target/release/platform-agent \
  --node-id "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d" \
  --secret-key "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789" \
  --listen-addr "0.0.0.0:9090"
```

## RPC Protocol Reference

All requests to `POST /agent/rpc` require a signed `AgentEnvelope`:

```json
{
  "version": "v1",
  "operation_id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
  "node_id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
  "timestamp": "2026-09-09T10:00:00Z",
  "payload": "{\"action\":\"CreateRelease\",\"params\":{...}}",
  "signature": "<hex-hmac-sha256>"
}
```

Supported actions:
- `CreateRelease`: Prepares release container and assigns isolation parameters.
- `StartRelease`: Starts container process.
- `StopRelease`: Terminates container with graceful timeout.
- `DrainRelease`: Initiates connection draining.
- `ReleaseLogs`: Queries logs for a specific release.
- `ReleaseStats`: Reports CPU and memory consumption.
- `Health`: Probes node agent readiness.
