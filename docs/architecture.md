# Aptos Arbitrage Bot – Architecture & Technical Specification

## Overview
Ultra-low-latency arbitrage engine that monitors multiple Aptos DEXes, detects profitable price discrepancies, and atomically  
executes trades while guaranteeing capital preservation.

**Guiding principles**

* **Never lose money** – safety checks on every transaction.  
* **≤ 100 ms end-to-end** – latency budget aligned with Aptos block time.  
* **Modular & extensible** – plug-in DEX adapters / routes.  
* **Async analytics** – zero impact on hot path.  
* **Secure key management** – no long-lived hot wallet.  
* **Robust SDLC** – disciplined GitHub workflow + CI pipeline.

## Architecture Diagram
```mermaid
graph TD
    MarketData[Market-Data Ingestion] -- tick --> Detector[Opportunity Detector]
    Detector -- candidate --> RiskManager[Risk Manager]
    RiskManager -- validated --> TradeExecutor[Trade Executor]
    TradeExecutor -- "signed tx" --> AptosChain[(Aptos L1)]
    TradeExecutor -- "exec report" --> Telemetry[Telemetry / Analytics]
    RiskManager -- metrics --> Telemetry
    Detector -- metrics --> Telemetry
    Config[Config Manager] -.-> Detector
    KeyMgmt[Key Mgmt<br/>(HSM / Threshold)] -.-> TradeExecutor
```

## Workspace Layout
```text
aptos-arb-bot/
├─ crates/
│  ├─ common/           ← types, error, logging, metrics
│  ├─ core/             ← orchestration, config, DI
│  ├─ dex-adapter-trait/← unified DEX API
│  ├─ adapters/
│  │   ├─ pontem/
│  │   └─ econia/
│  ├─ detector/         ← price graph, path search, risk filters
│  ├─ executor/         ← tx building, gas estimation, relaying
│  ├─ analytics/        ← async sinks → Postgres / Parquet
│  └─ bench/            ← criterion + profilers
└─ bin/
   ├─ arb-bot.rs        ← main runtime
   └─ cli.rs            ← management CLI
```

## Concurrency & Performance
| Layer | Technique |
|-------|-----------|
| IO | `tokio::net` + `TcpSocket::bind()` for QUIC / gRPC feeds |
| Parsing | Zero-copy deserialisation via `serde_json::from_slice` + `simd-json` feature gates |
| Channels | `tokio::sync::mpsc` (bounded); reuse with **crossbeam** for SPSC hot paths |
| CPU | Price-graph maths with **packed_simd_2**; pre-allocated MEM cache |
| Affinity | Avoid `tokio::task::spawn_blocking`; dedicate runtime for cryptography |

## Safety – No-Loss Guarantees
* **Two-phase quote check**  
  1. Off-chain price snapshot & slippage guard.  
  2. On-chain `view_function` re-validation pre-submit.  
* **On-chain simulation** – use Aptos **simulate RPC**; abort on Δ > ϵ.  
* **Atomic execution** – bundle multi-hop txs; fail-or-all semantics via Aptos batch.  
* **Adaptive gas & fallback** – real-time oracle; revert to conservative ceiling on outliers.

## Operational Security (OpSec)
* Primary keys in hardware HSM or Ledger Nano (USB HID).  
* Threshold signing (e.g. dKG + `aptos-hsmsign`) for unattended mode.  
* Ephemeral session keys (time- & tx-count-bounded) loaded into memory.  
* Relay server signs & relays tx; bot holds only session private-key.  
* Secrets injected via HashiCorp Vault using short-lived tokens.

## Extensibility
### Add new DEX adapter
1. Implement `dex_adapter_trait::DexAdapter`.  
2. Provide unit tests + golden JSON fixtures.  

### Add pair / route
* Update `config/<env>.yml`; **no code change required**.

### Multi-hop routing
* `detector` exposes k-shortest-path; adapters declare route capability matrix.

## Telemetry & Analytics
* Structured logging with `tracing_subscriber::fmt().json()` → in-mem channel.  
* Batch writer flushes to Postgres via **sqlx** (copy-in) or S3 Parquet.  
* Prometheus exporter on `:9000` (latency histograms, balances).  
* Analytics executed on low-priority runtime.

## Testing & Benchmarking
| Level | Tooling |
|-------|---------|
| Unit | `cargo test`, **proptest** for math & parsing |
| Integration | Local Aptos **devnet-docker**, run happy-path |
| Simulation | Fork devnet snapshot, replay historic ticks |
| Fuzz TX | `aptos-fuzzer` mutating trade payloads |
| Bench | **criterion**, flamegraph, `tokio-console` |
| CI | GitHub Actions matrix {stable, nightly} × {macOS, ubuntu-latest} |

## SDLC & GitHub Workflow
* **Branch naming**: `feat/`, `fix/`, `chore/`, `perf/`, `docs/`.  
* **Mandatory checks**: `fmt`, `clippy --deny warnings`, `test`, coverage ≥ 90 %.  
* **PR approvals**: 2 (≥ 1 code owner). PRs remain *Draft* until CI green.  
* **Labels**: latency-critical, security, adapter, infra, good-first-issue.  
* **Milestones** map to Aptos mainnet releases.  
* **Release tagging**: `vX.Y.Z` (semver) + annotated changelog.