# Production Dashboard Usage Guide

## Overview

The Market Data Ingestor now includes production-ready monitoring infrastructure with real-time metrics collection and an HTTP dashboard.

## Architecture

**Two-Component System:**

1. **MDI Process** (`market-data-ingestor`) - Runs data ingestion with live metrics collection
2. **Dashboard** (`mdi-dashboard`) - Queries and displays metrics from running MDI instance

## Quick Start

### 1. Start MDI with Metrics Enabled

```bash
# The MDI process will start an HTTP server on port 8080 (configurable)
cargo run --bin market-data-ingestor -- --config-path config/default.yml
```

The MDI process now includes:
- **Live metrics collection** during transaction processing  
- **HTTP server** exposing metrics at `http://localhost:8080`
- **Prometheus endpoint** at `http://localhost:8080/api/metrics/prometheus`

### 2. Start Dashboard

```bash
# Dashboard runs on port 8081 and queries MDI at port 8080
cargo run --bin mdi-dashboard -- --mdi-endpoint http://localhost:8080
```

### 3. Access Dashboard

Open `http://localhost:8081` to view the real-time production dashboard showing:

- **Live transaction processing** metrics
- **Connection status** and quality
- **Performance metrics** (latency, throughput)
- **System health** (CPU, memory usage)
- **Error tracking** and rates

## Configuration

### Enable Metrics in MDI

```yaml
# config/default.yml
market_data_config:
  performance:
    metrics_enabled: true    # Enable HTTP metrics server
    metrics_port: 8080      # Port for metrics/dashboard API
```

### Dashboard Options

```bash
cargo run --bin mdi-dashboard -- \
  --bind-address 127.0.0.1:8081 \
  --mdi-endpoint http://localhost:8080 \
  --refresh-interval-seconds 1
```

## API Endpoints

### MDI Metrics Server (port 8080)
- `GET /` - Dashboard HTML page
- `GET /api/health` - Health check
- `GET /api/metrics` - JSON metrics for dashboard
- `GET /api/metrics/prometheus` - Prometheus format metrics

### Dashboard Server (port 8081)  
- `GET /` - Dashboard HTML page (queries live MDI)
- `GET /api/health` - Dashboard health + MDI connectivity
- `GET /api/metrics` - Cached metrics from MDI

## Production Deployment

### 1. Multi-Instance Monitoring

Deploy multiple dashboard instances to monitor different MDI processes:

```bash
# Monitor production MDI
cargo run --bin mdi-dashboard -- \
  --bind-address 0.0.0.0:8081 \
  --mdi-endpoint http://prod-mdi:8080

# Monitor staging MDI  
cargo run --bin mdi-dashboard -- \
  --bind-address 0.0.0.0:8082 \
  --mdi-endpoint http://staging-mdi:8080
```

### 2. External Monitoring

Scrape Prometheus metrics for external monitoring systems:

```bash
curl http://localhost:8080/api/metrics/prometheus
```

### 3. Health Checks

Use health endpoints for load balancer/orchestrator health checks:

```bash
curl http://localhost:8080/api/health   # MDI health
curl http://localhost:8081/api/health   # Dashboard + MDI connectivity
```

## Metrics Collected

### Data Flow
- Transactions processed  
- Processing latency
- Throughput (transactions/sec, MB/sec)
- Last activity timestamp

### Pool Discovery
- Pools discovered/accepted/rejected
- Pool fetch success rates
- Cache hit rates

### System Health  
- Memory and CPU usage
- Connection uptime
- Active connections

### Error Tracking
- Connection, parsing, write errors
- Error rates and trends
- Timeout tracking

## Development vs Production

**Development:** Simple single-instance setup
```bash
# Terminal 1: Start MDI with metrics
cargo run --bin market-data-ingestor -- --config-path config/default.yml

# Terminal 2: Start dashboard  
cargo run --bin mdi-dashboard
```

**Production:** Distributed monitoring
- MDI processes run with metrics enabled
- Dashboard instances deployed separately  
- External monitoring scrapes Prometheus endpoints
- Load balancers use health check endpoints

## Key Benefits

✅ **Real metrics** from actual MDI processing (no simulation)  
✅ **Production-ready** HTTP servers with proper error handling  
✅ **Prometheus integration** for external monitoring systems  
✅ **Health checks** for orchestration and load balancing  
✅ **Live connection status** tracking  
✅ **Performance monitoring** with latency and throughput metrics