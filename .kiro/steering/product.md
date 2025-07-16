# Product Overview

## Aptos Arbitrage Bot

Ultra-low-latency arbitrage engine that monitors multiple Aptos DEXes, detects profitable price discrepancies, and atomically executes trades while guaranteeing capital preservation.

### Core Principles
- **Never lose money** – safety checks on every transaction
- **≤ 100ms end-to-end** – latency budget aligned with Aptos block time  
- **Modular & extensible** – plug-in DEX adapters and routing strategies
- **Async analytics** – zero impact on hot path
- **Secure key management** – no long-lived hot wallet

### Architecture Pipeline
1. **Market Data Ingestor** → Produces market update events from Aptos DEXes
2. **Opportunity Detector** → Finds arbitrage opportunities using graph algorithms
3. **Risk Manager** → Validates opportunities against risk profiles
4. **Trade Executor** → Executes approved trades atomically on-chain

### Current Status
The project is in active development with core detection algorithms implemented but missing production-ready data ingestion, risk management, and execution components. Focus is on building a robust, extensible foundation before optimizing for ultra-low latency.