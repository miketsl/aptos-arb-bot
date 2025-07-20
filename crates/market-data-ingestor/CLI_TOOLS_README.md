# Market Data Ingestor CLI Tools

This document describes the CLI tools for the Market Data Ingestor (MDI) component, implementing Task #5 Phase 4 requirements.

## Overview

The CLI tools provide professional-grade recording, conversion, and analysis capabilities with the following key features:

- **Professional Recording**: File rotation, pool state embedding, real-time monitoring
- **Advanced Conversion**: Batch processing, filtering, validation, repair modes
- **Unified Interface**: Single tool for all operations with comprehensive subcommands
- **Professional Monitoring**: Statistics, progress reporting, health checks

## Tools

### 1. mdi-recorder

Professional recording tool with pool state management and file rotation.

#### Features
- YAML configuration file support
- Automatic file rotation by size
- Real-time pool state detection and embedding
- Concurrent worker pools for pool state fetching
- Professional monitoring and progress reporting
- Graceful shutdown handling
- Comprehensive error handling and recovery

#### Usage

```bash
# Basic recording with default settings
mdi-recorder --config-path config/default.yml

# With custom recording configuration
mdi-recorder --config-path config/default.yml --recording-config recording_config.yml

# With CLI overrides
mdi-recorder --config-path config/default.yml --max-batches 1000 --verbose

# Generate sample configuration
mdi-recorder --generate-config

# Disable specific features
mdi-recorder --config-path config/default.yml --no-pool-detection --no-rotation
```

#### Configuration

The tool supports a comprehensive YAML configuration file:

```yaml
recording:
  output_file: "recording_{timestamp}.pb"
  file_rotation:
    enabled: true
    max_size_mb: 1000
    max_files: 10
    compress_rotated: false
  max_batches: 0
  max_duration_seconds: 0

pool_detection:
  enabled: true
  filters:
    max_tracked_pools: 5000
    dex_whitelist: ["hyperion", "thala"]
    token_blacklist: ["SCAM_TOKEN"]
  workers:
    pool_size: 20
    timeout_seconds: 30
    retry_attempts: 3
    rate_limit_per_minute: 100

monitoring:
  stats_interval_seconds: 10
  progress_report_interval: 100
  health_check_interval_seconds: 60
  verbose_logging: false
```

### 2. mdi-converter

Advanced bidirectional conversion tool with batch processing and filtering.

#### Features
- Batch processing of multiple files
- Time-based and content-based filtering
- Data validation and repair modes
- Compression support
- Detailed statistics and reporting
- Parallel processing capabilities

#### Usage

```bash
# Convert single protobuf to JSON
mdi-converter to-json --input recording.pb --output recording.json --pretty

# Convert with filtering
mdi-converter to-json --input recording.pb --output filtered.json \
  --start-time 1640995200 --end-time 1641081600 --dex-filter "hyperion,thala"

# Batch convert directory
mdi-converter to-json --input recordings/ --output json_output/ --batch --stats

# Convert JSON back to protobuf with validation
mdi-converter to-protobuf --input recording.json --output recording.pb --validate

# Repair mode for corrupted JSON
mdi-converter to-protobuf --input corrupted.json --output repaired.pb --repair

# Validate round-trip conversion
mdi-converter validate --protobuf recording.pb --json recording.json --detailed

# Batch operations with parallel workers
mdi-converter batch --input-dir recordings/ --output-dir converted/ \
  --operation to-json --workers 8 --continue-on-error
```

#### Filtering Options

- **Time Range**: `--start-time` and `--end-time` (Unix timestamps)
- **DEX Filter**: `--dex-filter "hyperion,thala"` (comma-separated)
- **Pool Filter**: `--pool-filter "pool1,pool2"` (comma-separated)
- **Compression**: `--compress` (for output files)

### 3. mdi-tools

Unified interface combining all operations with additional analysis tools.

#### Features
- All recording and conversion operations
- File information and statistics
- Data merging and extraction
- Performance benchmarking
- Comprehensive validation

#### Usage

```bash
# Recording (delegates to mdi-recorder)
mdi-tools record --config config/default.yml --output recording.pb --verbose

# Conversion (delegates to mdi-converter)
mdi-tools convert --input recording.pb --output recording.json --format json --pretty

# File information and analysis
mdi-tools info --input recording.pb --detailed --pools --dex-stats

# Validate file integrity
mdi-tools validate --input recording.pb --detailed

# Merge multiple recordings
mdi-tools merge --inputs file1.pb,file2.pb,file3.pb --output merged.pb --sort --deduplicate

# Extract specific data
mdi-tools extract --input recording.pb --output filtered.pb \
  --start-time 1640995200 --dex-filter "hyperion" --pool-filter "specific_pool_id"

# Performance benchmarking
mdi-tools benchmark --input recording.pb --iterations 10 --operation parse --detailed
```

#### Info Command Output

```
File Information:
File Size: 245.67 MB
Batches: 1,234
Transactions: 45,678
Pool States: 89
Time Range: 2024-01-15 10:30:00 to 2024-01-15 11:45:00 (4500 seconds)
Version Range: 1000000 to 1045677 (45678 versions)

DEX Breakdown:
  hyperion: 45 pools
  thala: 32 pools
  tapp: 12 pools

Top Pools:
  0x123...abc: 156 occurrences
  0x456...def: 134 occurrences
  0x789...ghi: 98 occurrences
```

## Configuration Management

### Recording Configuration

Generate a sample configuration file:

```bash
mdi-recorder --generate-config
```

This creates `recording_config_sample.yml` with all available options and documentation.

### Environment-Specific Configurations

Create different configurations for different environments:

```bash
# Development (fast, minimal filtering)
recording_config_dev.yml

# Production (comprehensive, with rotation)
recording_config_prod.yml

# Testing (limited duration, specific pools)
recording_config_test.yml
```

## Monitoring and Observability

### Real-time Statistics

The recorder provides real-time statistics:

```
Progress: 1,234 batches, 45,678 txns, 245.7 MB, 2.34 batches/s
```

### Health Checks

Automatic health monitoring detects:
- Stalled processing (no new batches)
- High error rates (>10% threshold)
- Connection issues
- Pool state fetch failures

### Final Summary

```
Recording Summary:
Duration: 4500.0s
Batches: 1,234 (0.27/s)
Transactions: 45,678 (10.15/s)
Data Written: 245.67 MB (0.05 MB/s)
Files Created: 3
Pools Discovered: 89 (accepted: 76, rejected: 13)
Pool States Fetched: 76 (failures: 2)
Avg Batch Time: 125.45ms (min: 45ms, max: 890ms)
Errors: conn=0, parse=1, write=0, timeouts=2
```

## Error Handling and Recovery

### Graceful Degradation

- Connection failures trigger exponential backoff reconnection
- Pool state fetch failures are logged but don't stop recording
- File rotation errors are logged with fallback to single file
- Parse errors skip individual batches but continue processing

### Data Integrity

- All conversions include integrity validation
- Round-trip conversion testing ensures data preservation
- Checksums and metadata validation detect corruption
- Repair modes attempt to fix common JSON formatting issues

## Performance Optimization

### File Rotation

- Size-based rotation prevents huge files
- Configurable retention policy
- Optional compression for archived files
- Atomic file operations prevent corruption

### Concurrent Processing

- Worker pools for pool state fetching
- Parallel batch processing for conversions
- Rate limiting to prevent API overload
- Configurable timeouts and retry logic

### Memory Management

- Streaming processing for large files
- Bounded buffers prevent memory exhaustion
- Efficient protobuf encoding/decoding
- Garbage collection friendly data structures

## Best Practices

### Recording

1. **Use file rotation** for long-running recordings
2. **Configure appropriate pool filters** to manage resource usage
3. **Monitor health checks** for early problem detection
4. **Use verbose logging** during development and debugging

### Conversion

1. **Validate conversions** with round-trip testing
2. **Use batch mode** for processing multiple files
3. **Apply filters** to reduce output size when possible
4. **Enable compression** for archival storage

### Analysis

1. **Use info command** to understand file contents before processing
2. **Benchmark performance** on representative data
3. **Extract subsets** for focused analysis
4. **Merge files** chronologically for time-series analysis

## Troubleshooting

### Common Issues

1. **High memory usage**: Enable file rotation, reduce batch sizes
2. **Slow pool state fetching**: Increase worker pool size, check network
3. **Parse errors**: Use repair mode, validate input data
4. **File corruption**: Check disk space, validate checksums

### Debug Mode

Enable verbose logging for detailed troubleshooting:

```bash
mdi-recorder-enhanced --config-path config/default.yml --verbose
```

### Log Analysis

Monitor log files for patterns:
- Connection retry attempts
- Pool state fetch timeouts
- File rotation events
- Error recovery actions

## Integration

### CI/CD Pipeline

```bash
# Validate recorded data in CI
mdi-tools-enhanced validate --input recording.pb --detailed

# Convert for analysis tools
mdi-tools-enhanced convert --input recording.pb --output recording.json --format json

# Extract test data
mdi-tools-enhanced extract --input recording.pb --output test_data.pb --start-time $START --end-time $END
```

### Monitoring Integration

Export metrics to monitoring systems:
- Batch processing rates
- Error rates and types
- File sizes and rotation events
- Pool discovery statistics

## Migration from Legacy Tools

### From mdi-recorder

```bash
# Old
mdi-recorder --config-path config.yml --output recording.pb

# New (equivalent)
mdi-recorder-enhanced --config-path config.yml --output recording.pb --no-rotation --no-pool-detection

# New (enhanced)
mdi-recorder-enhanced --config-path config.yml --recording-config recording_config.yml
```

### From mdi-converter

```bash
# Old
mdi-converter to-json --input recording.pb --output recording.json

# New (equivalent)
mdi-converter-enhanced to-json --input recording.pb --output recording.json

# New (enhanced)
mdi-converter-enhanced to-json --input recording.pb --output recording.json --stats --metadata
```

## Future Enhancements

Planned improvements for future versions:

1. **Real-time streaming**: WebSocket interface for live data
2. **Cloud storage**: Direct integration with S3/GCS
3. **Advanced analytics**: Built-in statistical analysis
4. **Distributed processing**: Multi-node parallel processing
5. **Machine learning**: Anomaly detection and pattern recognition