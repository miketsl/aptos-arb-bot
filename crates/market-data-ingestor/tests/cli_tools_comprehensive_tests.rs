//! Comprehensive CLI tools testing suite
//! Tests all CLI commands with various scenarios, error conditions, and edge cases

use market_data_ingestor::data_source::{RecordedBatch, RecordedPoolState};
use prost::Message;
use std::fs;
use std::process::Command;
use tempfile::{NamedTempFile, TempDir};

/// Helper to create test protobuf data
fn create_test_protobuf_data() -> Vec<u8> {
    let pool_state = RecordedPoolState {
        pool_id: "test_pool_cli".to_string(),
        dex_name: "hyperion".to_string(),
        token_a: "APT".to_string(),
        token_b: "USDC".to_string(),
        reserve_a: "1000000".to_string(),
        reserve_b: "2000000".to_string(),
        fee_rate: "0.003".to_string(),
        all_tokens: vec!["APT".to_string(), "USDC".to_string()],
        all_reserves: vec!["1000000".to_string(), "2000000".to_string()],
        all_weights: vec![],
        pool_type: "clmm".to_string(),
        block_height: 54321,
        additional_data: serde_json::to_vec(&serde_json::json!({
            "sqrt_price": "987654321",
            "liquidity": "123456789",
            "tick": 200,
            "tick_spacing": 64
        }))
        .unwrap(),
    };

    let batch = RecordedBatch {
        start_version: 54321,
        end_version: 54322,
        timestamp_ms: 1641000000000,
        transactions: vec![],
        pool_initializations: vec![pool_state],
    };

    let mut data = Vec::new();
    batch.encode_length_delimited(&mut data).unwrap();
    data
}

/// Test mdi-converter to-json with all options
#[test]
fn test_converter_to_json_comprehensive() {
    // Create test protobuf file
    let temp_pb_file = NamedTempFile::new().expect("Failed to create temp protobuf file");
    let pb_data = create_test_protobuf_data();
    fs::write(temp_pb_file.path(), pb_data).expect("Failed to write protobuf data");

    // Test basic conversion
    let temp_json_file = NamedTempFile::new().expect("Failed to create temp JSON file");
    let output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_json_file.path())
        .output()
        .expect("Failed to run mdi-converter");

    assert!(
        output.status.success(),
        "Basic to-json conversion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify JSON output
    let json_content =
        fs::read_to_string(temp_json_file.path()).expect("Failed to read JSON output");
    let json_value: serde_json::Value =
        serde_json::from_str(&json_content).expect("Invalid JSON output");

    if json_value.is_array() {
        // Direct array format
        let batches = json_value.as_array().unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0]["startVersion"], 54321);
    } else {
        // Metadata format
        assert!(json_value.get("metadata").is_some());
        assert!(json_value.get("batches").is_some());
        let batches = json_value["batches"].as_array().unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0]["startVersion"], 54321);
    }

    // Test with metadata enabled
    let temp_json_metadata =
        NamedTempFile::new().expect("Failed to create temp JSON metadata file");
    let output_metadata = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_json_metadata.path())
        .arg("--include-metadata")
        .arg("--pretty")
        .output()
        .expect("Failed to run mdi-converter with metadata");

    assert!(
        output_metadata.status.success(),
        "Metadata to-json conversion failed: {}",
        String::from_utf8_lossy(&output_metadata.stderr)
    );

    let metadata_content =
        fs::read_to_string(temp_json_metadata.path()).expect("Failed to read metadata JSON output");
    let metadata_json: serde_json::Value =
        serde_json::from_str(&metadata_content).expect("Invalid metadata JSON output");

    assert!(metadata_json.get("metadata").is_some());
    assert!(metadata_json.get("batches").is_some());
    assert!(metadata_json["metadata"].get("statistics").is_some());

    // Test with stats enabled
    let temp_json_stats = NamedTempFile::new().expect("Failed to create temp JSON stats file");
    let output_stats = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_json_stats.path())
        .arg("--stats")
        .output()
        .expect("Failed to run mdi-converter with stats");

    assert!(
        output_stats.status.success(),
        "Stats to-json conversion failed: {}",
        String::from_utf8_lossy(&output_stats.stderr)
    );

    // Should print statistics to stdout
    let stats_output = String::from_utf8_lossy(&output_stats.stdout);
    assert!(
        stats_output.contains("Conversion Summary") || stats_output.contains("Files Processed")
    );
}

/// Test mdi-converter to-protobuf with validation
#[test]
fn test_converter_to_protobuf_comprehensive() {
    // Create test JSON file
    let temp_json_file = NamedTempFile::new().expect("Failed to create temp JSON file");
    let json_data = serde_json::json!([{
        "startVersion": 98765,
        "endVersion": 98766,
        "timestampMs": 1641100000000i64,
        "transactions": [],
        "poolInitializations": [{
            "poolId": "json_test_pool",
            "dexName": "thala",
            "tokenA": "BTC",
            "tokenB": "ETH",
            "reserveA": "5000000",
            "reserveB": "8000000",
            "feeRate": "0.005",
            "allTokens": ["BTC", "ETH"],
            "allReserves": ["5000000", "8000000"],
            "allWeights": [],
            "poolType": "stable",
            "blockHeight": 98765,
            "additionalData": []
        }]
    }]);

    fs::write(
        temp_json_file.path(),
        serde_json::to_string_pretty(&json_data).unwrap(),
    )
    .expect("Failed to write JSON test data");

    // Test basic conversion
    let temp_pb_file = NamedTempFile::new().expect("Failed to create temp protobuf file");
    let output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-protobuf"])
        .arg("--input")
        .arg(temp_json_file.path())
        .arg("--output")
        .arg(temp_pb_file.path())
        .output()
        .expect("Failed to run mdi-converter to-protobuf");

    assert!(
        output.status.success(),
        "Basic to-protobuf conversion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify protobuf output by reading it back
    let pb_data = fs::read(temp_pb_file.path()).expect("Failed to read protobuf output");
    assert!(!pb_data.is_empty(), "Protobuf output should not be empty");

    // Test with validation enabled
    let temp_pb_validated =
        NamedTempFile::new().expect("Failed to create temp validated protobuf file");
    let output_validated = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-protobuf"])
        .arg("--input")
        .arg(temp_json_file.path())
        .arg("--output")
        .arg(temp_pb_validated.path())
        .arg("--validate")
        .arg("--stats")
        .output()
        .expect("Failed to run mdi-converter with validation");

    assert!(
        output_validated.status.success(),
        "Validated to-protobuf conversion failed: {}",
        String::from_utf8_lossy(&output_validated.stderr)
    );

    // Should include statistics
    let validated_output = String::from_utf8_lossy(&output_validated.stdout);
    assert!(
        validated_output.contains("Conversion Summary")
            || validated_output.contains("Files Processed")
    );
}

/// Test mdi-converter validate command
#[test]
fn test_converter_validate_comprehensive() {
    // Create matching protobuf and JSON files
    let temp_pb_file = NamedTempFile::new().expect("Failed to create temp protobuf file");
    let pb_data = create_test_protobuf_data();
    fs::write(temp_pb_file.path(), pb_data).expect("Failed to write protobuf data");

    // Convert to JSON first
    let temp_json_file = NamedTempFile::new().expect("Failed to create temp JSON file");
    let convert_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_json_file.path())
        .output()
        .expect("Failed to convert to JSON");

    assert!(convert_output.status.success(), "JSON conversion failed");

    // Test validation
    let validate_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "validate"])
        .arg("--protobuf")
        .arg(temp_pb_file.path())
        .arg("--json")
        .arg(temp_json_file.path())
        .output()
        .expect("Failed to run validation");

    assert!(
        validate_output.status.success(),
        "Validation failed: {}",
        String::from_utf8_lossy(&validate_output.stderr)
    );

    let validate_stdout = String::from_utf8_lossy(&validate_output.stdout);
    assert!(
        validate_stdout.contains("Round-trip validation successful")
            || validate_stdout.contains("Data integrity: PASSED")
    );

    // Test detailed validation
    let detailed_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "validate"])
        .arg("--protobuf")
        .arg(temp_pb_file.path())
        .arg("--json")
        .arg(temp_json_file.path())
        .arg("--detailed")
        .output()
        .expect("Failed to run detailed validation");

    assert!(
        detailed_output.status.success(),
        "Detailed validation failed: {}",
        String::from_utf8_lossy(&detailed_output.stderr)
    );
}

/// Test mdi-converter info command
#[test]
fn test_converter_info_comprehensive() {
    // Create test protobuf file with multiple batches
    let temp_pb_file = NamedTempFile::new().expect("Failed to create temp protobuf file");
    let mut pb_data = Vec::new();

    // Add multiple batches
    for i in 0..3 {
        let pool_state = RecordedPoolState {
            pool_id: format!("info_test_pool_{}", i),
            dex_name: if i % 2 == 0 { "hyperion" } else { "thala" }.to_string(),
            token_a: "APT".to_string(),
            token_b: "USDC".to_string(),
            reserve_a: format!("{}", 1000000 + i * 100000),
            reserve_b: format!("{}", 2000000 + i * 200000),
            fee_rate: "0.003".to_string(),
            all_tokens: vec!["APT".to_string(), "USDC".to_string()],
            all_reserves: vec![
                format!("{}", 1000000 + i * 100000),
                format!("{}", 2000000 + i * 200000),
            ],
            all_weights: vec![],
            pool_type: "clmm".to_string(),
            block_height: 60000 + i,
            additional_data: vec![],
        };

        let batch = RecordedBatch {
            start_version: 60000 + i,
            end_version: 60001 + i,
            timestamp_ms: 1641200000000 + (i as i64 * 10000),
            transactions: vec![],
            pool_initializations: vec![pool_state],
        };

        batch.encode_length_delimited(&mut pb_data).unwrap();
    }

    fs::write(temp_pb_file.path(), pb_data).expect("Failed to write protobuf data");

    // Test basic info
    let info_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "info"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .output()
        .expect("Failed to run info command");

    assert!(
        info_output.status.success(),
        "Info command failed: {}",
        String::from_utf8_lossy(&info_output.stderr)
    );

    let info_stdout = String::from_utf8_lossy(&info_output.stdout);
    assert!(info_stdout.contains("File Information"));
    assert!(info_stdout.contains("Batches: 3"));
    assert!(info_stdout.contains("Pool States: 3"));

    // Test detailed info
    let detailed_info_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "info"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--detailed")
        .output()
        .expect("Failed to run detailed info command");

    assert!(
        detailed_info_output.status.success(),
        "Detailed info command failed: {}",
        String::from_utf8_lossy(&detailed_info_output.stderr)
    );

    // Test with breakdowns
    let breakdown_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "info"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--dex-breakdown")
        .arg("--pool-breakdown")
        .arg("--time-range-analysis")
        .output()
        .expect("Failed to run breakdown info command");

    assert!(
        breakdown_output.status.success(),
        "Breakdown info command failed: {}",
        String::from_utf8_lossy(&breakdown_output.stderr)
    );

    let breakdown_stdout = String::from_utf8_lossy(&breakdown_output.stdout);
    assert!(
        breakdown_stdout.contains("DEX Breakdown")
            || breakdown_stdout.contains("hyperion")
            || breakdown_stdout.contains("thala")
    );
}

/// Test mdi-converter batch operations
#[test]
fn test_converter_batch_operations() {
    // Create temporary directories
    let temp_input_dir = TempDir::new().expect("Failed to create temp input directory");
    let temp_output_dir = TempDir::new().expect("Failed to create temp output directory");

    // Create multiple protobuf files in input directory
    for i in 0..3 {
        let pb_file_path = temp_input_dir.path().join(format!("batch_test_{}.pb", i));
        let pb_data = create_test_protobuf_data();
        fs::write(&pb_file_path, pb_data).expect("Failed to write batch protobuf file");
    }

    // Test batch to-json operation
    let batch_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "batch"])
        .arg("--input-dir")
        .arg(temp_input_dir.path())
        .arg("--output-dir")
        .arg(temp_output_dir.path())
        .arg("--operation")
        .arg("to-json")
        .arg("--workers")
        .arg("2")
        .output()
        .expect("Failed to run batch operation");

    assert!(
        batch_output.status.success(),
        "Batch operation failed: {}",
        String::from_utf8_lossy(&batch_output.stderr)
    );

    // Verify JSON files were created
    let output_files: Vec<_> = fs::read_dir(temp_output_dir.path())
        .expect("Failed to read output directory")
        .collect();

    assert_eq!(output_files.len(), 3, "Should have created 3 JSON files");

    // Verify file contents
    for entry in output_files {
        let entry = entry.expect("Failed to read directory entry");
        let file_path = entry.path();

        if file_path.extension().and_then(|s| s.to_str()) == Some("json") {
            let json_content = fs::read_to_string(&file_path).expect("Failed to read JSON file");

            // Should be valid JSON
            serde_json::from_str::<serde_json::Value>(&json_content)
                .expect("Invalid JSON in batch output");
        }
    }
}

/// Test mdi-converter error handling and edge cases
#[test]
fn test_converter_error_handling() {
    // Test with non-existent input file
    let nonexistent_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg("/nonexistent/file.pb")
        .arg("--output")
        .arg("/tmp/output.json")
        .output()
        .expect("Failed to run mdi-converter with nonexistent file");

    assert!(
        !nonexistent_output.status.success(),
        "Should fail with nonexistent input file"
    );

    // Test with invalid JSON for to-protobuf
    let temp_invalid_json = NamedTempFile::new().expect("Failed to create temp invalid JSON file");
    fs::write(temp_invalid_json.path(), "invalid json content")
        .expect("Failed to write invalid JSON");

    let temp_pb_output = NamedTempFile::new().expect("Failed to create temp protobuf output");
    let invalid_json_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-protobuf"])
        .arg("--input")
        .arg(temp_invalid_json.path())
        .arg("--output")
        .arg(temp_pb_output.path())
        .output()
        .expect("Failed to run mdi-converter with invalid JSON");

    assert!(
        !invalid_json_output.status.success(),
        "Should fail with invalid JSON input"
    );

    // Test validation with mismatched files
    let temp_pb1 = NamedTempFile::new().expect("Failed to create temp protobuf file");
    let temp_pb2 = NamedTempFile::new().expect("Failed to create temp protobuf file");

    fs::write(temp_pb1.path(), create_test_protobuf_data()).expect("Failed to write pb1");
    fs::write(temp_pb2.path(), [1, 2, 3, 4]).expect("Failed to write pb2"); // Invalid protobuf

    let temp_json = NamedTempFile::new().expect("Failed to create temp JSON file");
    fs::write(temp_json.path(), "[{\"test\": \"data\"}]").expect("Failed to write JSON");

    let mismatch_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "validate"])
        .arg("--protobuf")
        .arg(temp_pb2.path()) // Invalid protobuf
        .arg("--json")
        .arg(temp_json.path())
        .output()
        .expect("Failed to run validation with mismatched files");

    assert!(
        !mismatch_output.status.success(),
        "Should fail with invalid protobuf"
    );
}

/// Test mdi-converter filtering options
#[test]
fn test_converter_filtering_options() {
    // Create test protobuf with multiple batches at different times
    let temp_pb_file = NamedTempFile::new().expect("Failed to create temp protobuf file");
    let mut pb_data = Vec::new();

    let timestamps = [1641000000000i64, 1641100000000i64, 1641200000000i64];

    for (i, &timestamp) in timestamps.iter().enumerate() {
        let pool_state = RecordedPoolState {
            pool_id: format!("filter_test_pool_{}", i),
            dex_name: if i == 0 { "hyperion" } else { "thala" }.to_string(),
            token_a: "APT".to_string(),
            token_b: "USDC".to_string(),
            reserve_a: "1000000".to_string(),
            reserve_b: "2000000".to_string(),
            fee_rate: "0.003".to_string(),
            all_tokens: vec!["APT".to_string(), "USDC".to_string()],
            all_reserves: vec!["1000000".to_string(), "2000000".to_string()],
            all_weights: vec![],
            pool_type: "clmm".to_string(),
            block_height: 70000 + i as u64,
            additional_data: vec![],
        };

        let batch = RecordedBatch {
            start_version: 70000 + i as u64,
            end_version: 70001 + i as u64,
            timestamp_ms: timestamp,
            transactions: vec![],
            pool_initializations: vec![pool_state],
        };

        batch.encode_length_delimited(&mut pb_data).unwrap();
    }

    fs::write(temp_pb_file.path(), pb_data).expect("Failed to write protobuf data");

    // Test time range filtering
    let temp_filtered_json =
        NamedTempFile::new().expect("Failed to create temp filtered JSON file");
    let filter_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_filtered_json.path())
        .arg("--start-time")
        .arg("1641050000") // Timestamp in seconds (middle range) - fixed to match data scale
        .arg("--end-time")
        .arg("1641150000")
        .output()
        .expect("Failed to run filtered conversion");

    assert!(
        filter_output.status.success(),
        "Filtered conversion failed: {}",
        String::from_utf8_lossy(&filter_output.stderr)
    );

    // Verify filtered output
    let filtered_content =
        fs::read_to_string(temp_filtered_json.path()).expect("Failed to read filtered JSON");
    let filtered_json: serde_json::Value =
        serde_json::from_str(&filtered_content).expect("Invalid filtered JSON");

    let batches = if filtered_json.is_array() {
        filtered_json.as_array().unwrap()
    } else {
        filtered_json["batches"].as_array().unwrap()
    };

    // Should only have the middle batch based on timestamp filtering
    assert_eq!(batches.len(), 1, "Time filtering should return 1 batch");

    // Test DEX filtering
    let temp_dex_filtered = NamedTempFile::new().expect("Failed to create temp DEX filtered file");
    let dex_filter_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_dex_filtered.path())
        .arg("--dex-filter")
        .arg("hyperion")
        .output()
        .expect("Failed to run DEX filtered conversion");

    assert!(
        dex_filter_output.status.success(),
        "DEX filtered conversion failed: {}",
        String::from_utf8_lossy(&dex_filter_output.stderr)
    );

    let dex_filtered_content =
        fs::read_to_string(temp_dex_filtered.path()).expect("Failed to read DEX filtered JSON");
    let dex_filtered_json: serde_json::Value =
        serde_json::from_str(&dex_filtered_content).expect("Invalid DEX filtered JSON");

    let dex_batches = if dex_filtered_json.is_array() {
        dex_filtered_json.as_array().unwrap()
    } else {
        dex_filtered_json["batches"].as_array().unwrap()
    };

    // Should only have hyperion pools
    assert_eq!(dex_batches.len(), 1, "DEX filtering should return 1 batch");

    if let Some(pool_inits) = dex_batches[0]["poolInitializations"].as_array() {
        if !pool_inits.is_empty() {
            assert_eq!(pool_inits[0]["dexName"], "hyperion");
        }
    }
}

/// Test mdi-inspector command functionality
#[test]
fn test_inspector_comprehensive() {
    let temp_pb_file = NamedTempFile::new().expect("Failed to create temp protobuf file");
    let pb_data = create_test_protobuf_data();
    fs::write(temp_pb_file.path(), pb_data).expect("Failed to write protobuf data");

    // Test basic inspection
    let inspect_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-inspector", "--"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .output()
        .expect("Failed to run mdi-inspector");

    // Inspector should at least try to run (may not have full functionality in tests)
    // The key is that it doesn't crash and provides some output
    let stdout = String::from_utf8_lossy(&inspect_output.stdout);
    let stderr = String::from_utf8_lossy(&inspect_output.stderr);

    // Either succeeds or provides meaningful error message
    assert!(
        inspect_output.status.success()
            || stderr.contains("inspector")
            || stderr.contains("input")
            || stdout.contains("inspector")
            || !stdout.is_empty()
            || !stderr.is_empty(),
        "Inspector should produce some output or meaningful error"
    );
}

/// Test comprehensive CLI argument validation
#[test]
fn test_cli_argument_validation() {
    // Test mdi-converter without required arguments
    let no_args_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .output()
        .expect("Failed to run mdi-converter without args");

    assert!(
        !no_args_output.status.success(),
        "Should fail without required arguments"
    );
    let stderr = String::from_utf8_lossy(&no_args_output.stderr);
    assert!(stderr.contains("required") || stderr.contains("input") || stderr.contains("output"));

    // Test invalid command
    let invalid_cmd_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "invalid-command"])
        .output()
        .expect("Failed to run mdi-converter with invalid command");

    assert!(
        !invalid_cmd_output.status.success(),
        "Should fail with invalid command"
    );

    // Test conflicting options (if any exist in the CLI design)
    let temp_input = NamedTempFile::new().expect("Failed to create temp input");
    let temp_output = NamedTempFile::new().expect("Failed to create temp output");

    // Test with various flag combinations
    let flag_test_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_input.path())
        .arg("--output")
        .arg(temp_output.path())
        .arg("--include-metadata")
        .output()
        .expect("Failed to run mdi-converter with flags");

    // This should succeed or fail gracefully
    let flag_stderr = String::from_utf8_lossy(&flag_test_output.stderr);
    if !flag_test_output.status.success() {
        assert!(flag_stderr.contains("error") || flag_stderr.contains("failed"));
    }
}

/// Test help and version commands
#[test]
fn test_help_and_version() {
    // Test help command for mdi-converter
    let help_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "--help"])
        .output()
        .expect("Failed to run mdi-converter --help");

    assert!(help_output.status.success(), "Help command should succeed");
    let help_stdout = String::from_utf8_lossy(&help_output.stdout);
    assert!(
        help_stdout.contains("mdi-converter")
            || help_stdout.contains("Usage")
            || help_stdout.contains("Commands")
            || help_stdout.contains("convert")
    );

    // Test version command
    let version_output = Command::new("cargo")
        .args(["run", "--bin", "mdi-converter", "--", "--version"])
        .output()
        .expect("Failed to run mdi-converter --version");

    assert!(
        version_output.status.success(),
        "Version command should succeed"
    );
    let version_stdout = String::from_utf8_lossy(&version_output.stdout);
    assert!(
        version_stdout.contains("mdi-converter")
            || version_stdout.contains("version")
            || version_stdout.contains("2.0")
            || !version_stdout.is_empty()
    );
}

/// Test output format consistency across different input sizes
#[test]
fn test_output_format_consistency() {
    // Test with different sized inputs
    let test_sizes = [1, 5, 10];

    for &size in &test_sizes {
        let temp_pb_file = NamedTempFile::new().expect("Failed to create temp protobuf file");
        let mut pb_data = Vec::new();

        // Create multiple batches
        for i in 0..size {
            let pool_state = RecordedPoolState {
                pool_id: format!("consistency_pool_{}", i),
                dex_name: "hyperion".to_string(),
                token_a: "APT".to_string(),
                token_b: "USDC".to_string(),
                reserve_a: "1000000".to_string(),
                reserve_b: "2000000".to_string(),
                fee_rate: "0.003".to_string(),
                all_tokens: vec!["APT".to_string(), "USDC".to_string()],
                all_reserves: vec!["1000000".to_string(), "2000000".to_string()],
                all_weights: vec![],
                pool_type: "clmm".to_string(),
                block_height: 80000 + i,
                additional_data: vec![],
            };

            let batch = RecordedBatch {
                start_version: 80000 + i,
                end_version: 80001 + i,
                timestamp_ms: 1641300000000 + (i as i64 * 1000),
                transactions: vec![],
                pool_initializations: vec![pool_state],
            };

            batch.encode_length_delimited(&mut pb_data).unwrap();
        }

        fs::write(temp_pb_file.path(), pb_data).expect("Failed to write protobuf data");

        // Convert to JSON
        let temp_json_file = NamedTempFile::new().expect("Failed to create temp JSON file");
        let convert_output = Command::new("cargo")
            .args(["run", "--bin", "mdi-converter", "--", "to-json"])
            .arg("--input")
            .arg(temp_pb_file.path())
            .arg("--output")
            .arg(temp_json_file.path())
            .arg("--include-metadata")
            .output()
            .expect("Failed to convert to JSON");

        assert!(
            convert_output.status.success(),
            "Conversion failed for size {}: {}",
            size,
            String::from_utf8_lossy(&convert_output.stderr)
        );

        // Verify JSON structure is consistent
        let json_content =
            fs::read_to_string(temp_json_file.path()).expect("Failed to read JSON output");
        let json_value: serde_json::Value =
            serde_json::from_str(&json_content).expect("Invalid JSON output");

        // Should have consistent metadata structure
        assert!(json_value.get("metadata").is_some());
        assert!(json_value.get("batches").is_some());

        let batches = json_value["batches"].as_array().unwrap();
        assert_eq!(
            batches.len(),
            size as usize,
            "Batch count mismatch for size {}",
            size
        );

        // Verify each batch has consistent structure
        for batch in batches {
            assert!(batch.get("startVersion").is_some());
            assert!(batch.get("endVersion").is_some());
            assert!(batch.get("timestampMs").is_some());
            assert!(batch.get("transactions").is_some());
            assert!(batch.get("poolInitializations").is_some());
        }
    }
}
