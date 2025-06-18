# `market-data-ingestor` — Comprehensive Analysis  
_Date: 2025-06-18_

---

## 1  Executive Summary
The `market-data-ingestor` (MDI) crate implements an Aptos on-chain event pipeline that partially matches the architecture documented in `docs/`.  
Core pipeline scaffolding (transaction stream ➜ event filter ➜ CLMM parser ➜ detector push) exists, yet several critical features are stubs or missing:

* **Pool snapshot bootstrap, tick-map maintenance, Edge translation, metrics, back-pressure, error resilience, and production wiring are incomplete.**
* **No direct integration with the `detector` crate** (PriceGraph updates) or with DEX-specific adapter crates envisioned in the design.

---

## 2  Documentation Expectations

| Source Doc | Key MDI Requirements |
| --- | --- |
| [`architecture.md`](docs/architecture.md:18) | First stage in pipeline; supplies fresh pool data to **PriceGraph** via async channel. |
| [`plan_aptos_data_ingestion.md`](docs/plan_aptos_data_ingestion.md:90) | Detailed step plan: snapshot → poll events → upsert `Edge` → metrics; milestones M1–M5. |
| [`price_graph_design.md`](docs/price_graph_design.md:117) | Integration diagram shows `market-data-ingestor → PriceGraphSvc`. |
| [`current_status.md`](docs/current_status.md:34) | Identifies MDI as **placeholder** and lists eight remediation priorities. |

---

## 3  Source-Code Review

| Area | Implementation Snapshot |
| --- | --- |
| Pipeline skeleton | [`processor.rs`](crates/market-data-ingestor/src/processor.rs:29) loops over `TransactionStream`, pushes through three custom steps. |
| Event filtering | [`event_extractor.rs`](crates/market-data-ingestor/src/steps/event_extractor.rs:16-60) matches `type_str` & optional pool whitelist. |
| CLMM parsing & pool state | [`clmm_parser.rs`](crates/market-data-ingestor/src/steps/clmm_parser.rs:15-170) maintains `HashMap<String, PoolState>`; TODOs for tick-map & token-pair inference. |
| Update forwarding | [`detector_push.rs`](crates/market-data-ingestor/src/steps/detector_push.rs:11-30) sends `MarketUpdate` over `mpsc`. |
| Config structs | [`types.rs`](crates/market-data-ingestor/src/types.rs:11-25) and [`config.rs`](crates/market-data-ingestor/src/config.rs:7-20). |
| Executable | [`main.rs`](crates/market-data-ingestor/src/main.rs:7-42) spawns mock detector receiver only. |

---

## 4  Design-vs-Implementation Gap Analysis

| Requirement (docs) | Current Code | Gap / Deviation |
| --- | --- | --- |
| ⬤ **DEX adapter abstraction** | Direct event parsing; no adapter crates | Breaks modularity; cannot reuse trait pattern. |
| ⬤ Initial pool **snapshot** via adapter::`fetch_pools` | Not implemented; relies on first `PoolSnapshot` event | Detector starts with empty graph → missed arbs on launch. |
| ⬤ **Edge / PoolModel** generation & `PriceGraph.upsert_pool` | Emits `MarketUpdate` (custom struct) | Translation layer missing → detector still fabricates pools. |
| ⬤ **Tick map** construction for CLMM | TODO comment | Concentrated-liquidity maths impossible. |
| ⬤ **Metrics** (`ticks_ingested_total`, `edges_active`) | None | Observability missing. |
| ⬤ **Back-pressure / channel capacity** | Fixed `mpsc(100)` without retry | Potential data loss under burst load. |
| ⬤ **Error handling** | Many `anyhow!` but loop breaks on `TransactionStream` error (`break;`) | Lacks retry & exponential back-off. |
| ⬤ **Config hot-reload** | Not planned | Diverges from docs extensibility goals. |
| ⬤ **CI quality gates** | Warnings allowed, sparse tests | Spec demands `clippy --deny warnings` & ≥90 % coverage. |

---

## 5  Integration Assessment

```
             ┌──────────────────────────┐
             │  market-data-ingestor    │
TxStream ⇒   │ EventExtractor │         │
             │ ClmmParser     │         │   (sends MarketUpdate)
             └────────┬───────┘         │
                      ▼                 │
             ┌──────────────────────────┐
             │ detector::PriceGraphSvc  │  ← expects Edge updates
             └──────────────────────────┘
```

* **Current integration broken** – signature mismatch (`MarketUpdate` vs `Edge`).
* **Main binary wiring absent** – `arb-bot` does not construct PriceGraph channel.
* **Shared types** should live in `crates/common`, but CLMM structs currently local.

---

## 6  Go-Forward Plan

```mermaid
flowchart TD
    subgraph Phase-1  (📆 1–2 d)
        A1(Refactor structs → common) --> A2(Implement Edge translator)
        A2 --> A3(Add PriceGraph consumer task)
    end
    subgraph Phase-2  (📆 2–3 d)
        B1(Snapshot bootstrap via DexAdapter::fetch_pools)
        B1 --> B2(Replace fabricated data in detector)
        B2 --> B3(Tick-map build + CL maths)
    end
    subgraph Phase-3  (📆 1 d)
        C1(Metrics & Prometheus)
        C1 --> C2(Channel back-pressure & retry)
    end
    subgraph Phase-4  (📆 1 d)
        D1(Integration tests with devnet fork)
        D1 --> D2(CI: clippy-deny-warnings + coverage)
    end
```

### Immediate Action Items (Phase-1)
1. **Move** `TokenPair`, `PoolState`, `MarketUpdate` to `crates/common`.  
2. **Add** `impl From<&MarketUpdate> for detector::Edge` in new module.  
3. **Spawn** `PriceGraphUpdateService` in `detector` listening to channel.  
4. **Adjust** `MarketDataIngestorProcessor` to send `Vec<Edge>` instead of `MarketUpdate`.  
5. **Patch** `arb-bot` startup: create channel, snapshot pools (`DexAdapter::fetch_pools`), seed graph, then launch MDI and detector concurrently.

### Risk Mitigations
* Use `tokio::sync::mpsc::Sender::try_send` with overflow metrics to surface drops.  
* Implement **exponential back-off** on `TransactionStream` failures.  
* Harden parsing with `serde(deny_unknown_fields)` & per-DEX parsers.

### Testing Matrix
| Level | What to test | Status |
| --- | --- | --- |
| Unit | Event → MarketUpdate → Edge conversion | ❌ |
| Integration | Full pipeline on devnet snapshot | ❌ |
| Bench | Parser throughput under 1 000 TPS | ❌ |

---

## 7  Conclusion
The MDI crate lays a solid foundation but misses critical translation and robustness layers required for production arbitrage. Executing the phased plan above will realign the implementation with architectural intent and unblock end-to-end detection within the ≤ 100 ms latency budget.