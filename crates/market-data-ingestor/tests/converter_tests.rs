use prost::Message;
use serde_json;
use std::fs;
use tempfile::NamedTempFile;

use market_data_ingestor::data_source::{RecordedBatch, RecordedPoolState};

/// Test helper to create a sample RecordedBatch with pool state
fn create_test_batch_with_pools() -> RecordedBatch {
    let pool_state = RecordedPoolState {
        pool_id: "test_pool_123".to_string(),
        dex_name: "hyperion".to_string(),
        token_a: "APT".to_string(),
        token_b: "USDC".to_string(),
        reserve_a: "1000000".to_string(),
        reserve_b: "500000".to_string(),
        fee_rate: "0.003".to_string(),
        all_tokens: vec!["APT".to_string(), "USDC".to_string()],
        all_reserves: vec!["1000000".to_string(), "500000".to_string()],
        all_weights: vec![],
        pool_type: "clmm".to_string(),
        block_height: 12345,
        additional_data: serde_json::to_vec(&serde_json::json!({
            "sqrt_price": "1234567890",
            "liquidity": "9876543210",
            "tick": -100,
            "tick_spacing": 100
        }))
        .unwrap(),
    };

    RecordedBatch {
        start_version: 12345,
        end_version: 12346,
        timestamp_ms: 1640995200000,
        transactions: vec![], // Empty for simplicity in tests
        pool_initializations: vec![pool_state],
    }
}

/// Test helper to create a protobuf file with test data
fn create_test_protobuf_file(batches: Vec<RecordedBatch>) -> NamedTempFile {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let mut data = Vec::new();

    for batch in batches {
        let mut buf = bytes::BytesMut::new();
        batch
            .encode_length_delimited(&mut buf)
            .expect("Failed to encode batch");
        data.extend_from_slice(&buf);
    }

    fs::write(temp_file.path(), data).expect("Failed to write test data");
    temp_file
}

#[test]
fn test_protobuf_to_json_conversion() {
    // Create test data
    let test_batch = create_test_batch_with_pools();
    let temp_pb_file = create_test_protobuf_file(vec![test_batch.clone()]);
    let temp_json_file = NamedTempFile::new().expect("Failed to create temp JSON file");

    // Run conversion using the mdi-converter binary
    let output = std::process::Command::new("cargo")
        .args(&["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_json_file.path())
        .arg("--pretty")
        .arg("--include-metadata")
        .output()
        .expect("Failed to run mdi-converter");

    assert!(
        output.status.success(),
        "Conversion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify JSON output exists and is valid
    let json_content =
        fs::read_to_string(temp_json_file.path()).expect("Failed to read JSON output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_content).expect("Invalid JSON output");

    // Verify structure
    assert!(json_value.get("metadata").is_some(), "Missing metadata");
    assert!(json_value.get("batches").is_some(), "Missing batches");

    let batches = json_value["batches"]
        .as_array()
        .expect("Batches should be array");
    assert_eq!(batches.len(), 1, "Should have exactly one batch");

    let batch = &batches[0];
    assert_eq!(batch["start_version"], 12345);
    assert_eq!(batch["end_version"], 12346);
    assert_eq!(batch["timestamp_ms"], 1640995200000i64);

    let pool_inits = batch["pool_initializations"]
        .as_array()
        .expect("Pool initializations should be array");
    assert_eq!(
        pool_inits.len(),
        1,
        "Should have exactly one pool initialization"
    );

    let pool = &pool_inits[0];
    assert_eq!(pool["pool_id"], "test_pool_123");
    assert_eq!(pool["dex_name"], "hyperion");
    assert_eq!(pool["token_a"], "APT");
    assert_eq!(pool["token_b"], "USDC");
    assert_eq!(pool["pool_type"], "clmm");
}

#[test]
fn test_json_to_protobuf_conversion() {
    // Create test data and convert to JSON first
    let test_batch = create_test_batch_with_pools();
    let temp_pb_file = create_test_protobuf_file(vec![test_batch.clone()]);
    let temp_json_file = NamedTempFile::new().expect("Failed to create temp JSON file");
    let temp_reconverted_pb_file =
        NamedTempFile::new().expect("Failed to create temp reconverted PB file");

    // First convert to JSON
    let output = std::process::Command::new("cargo")
        .args(&["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_json_file.path())
        .output()
        .expect("Failed to run mdi-converter to-json");

    assert!(output.status.success(), "JSON conversion failed");

    // Then convert back to protobuf
    let output = std::process::Command::new("cargo")
        .args(&["run", "--bin", "mdi-converter", "--", "to-protobuf"])
        .arg("--input")
        .arg(temp_json_file.path())
        .arg("--output")
        .arg(temp_reconverted_pb_file.path())
        .arg("--validate")
        .output()
        .expect("Failed to run mdi-converter to-protobuf");

    assert!(
        output.status.success(),
        "Protobuf conversion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the reconverted protobuf can be read
    let reconverted_data =
        fs::read(temp_reconverted_pb_file.path()).expect("Failed to read reconverted protobuf");

    let mut buf = bytes::BytesMut::from(&reconverted_data[..]);
    let reconverted_batch = RecordedBatch::decode_length_delimited(&mut buf)
        .expect("Failed to decode reconverted batch");

    // Verify data integrity
    assert_eq!(reconverted_batch.start_version, test_batch.start_version);
    assert_eq!(reconverted_batch.end_version, test_batch.end_version);
    assert_eq!(reconverted_batch.timestamp_ms, test_batch.timestamp_ms);
    assert_eq!(
        reconverted_batch.pool_initializations.len(),
        test_batch.pool_initializations.len()
    );

    let reconverted_pool = &reconverted_batch.pool_initializations[0];
    let original_pool = &test_batch.pool_initializations[0];

    assert_eq!(reconverted_pool.pool_id, original_pool.pool_id);
    assert_eq!(reconverted_pool.dex_name, original_pool.dex_name);
    assert_eq!(reconverted_pool.token_a, original_pool.token_a);
    assert_eq!(reconverted_pool.token_b, original_pool.token_b);
    assert_eq!(reconverted_pool.pool_type, original_pool.pool_type);
}

#[test]
fn test_round_trip_validation() {
    // Create test data
    let test_batch = create_test_batch_with_pools();
    let temp_pb_file = create_test_protobuf_file(vec![test_batch]);
    let temp_json_file = NamedTempFile::new().expect("Failed to create temp JSON file");

    // Convert to JSON
    let output = std::process::Command::new("cargo")
        .args(&["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_json_file.path())
        .output()
        .expect("Failed to run mdi-converter to-json");

    assert!(output.status.success(), "JSON conversion failed");

    // Run round-trip validation
    let output = std::process::Command::new("cargo")
        .args(&["run", "--bin", "mdi-converter", "--", "validate"])
        .arg("--protobuf")
        .arg(temp_pb_file.path())
        .arg("--json")
        .arg(temp_json_file.path())
        .output()
        .expect("Failed to run mdi-converter validate");

    assert!(
        output.status.success(),
        "Round-trip validation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Round-trip validation successful"),
        "Validation should succeed"
    );
    assert!(
        stdout.contains("Data integrity: PASSED"),
        "Data integrity should pass"
    );
}

#[test]
fn test_multi_token_pool_conversion() {
    // Create a multi-token pool for testing
    let multi_token_pool = RecordedPoolState {
        pool_id: "multi_token_pool_456".to_string(),
        dex_name: "thala".to_string(),
        token_a: "APT".to_string(),
        token_b: "USDC".to_string(),
        reserve_a: "1000000".to_string(),
        reserve_b: "500000".to_string(),
        fee_rate: "0.003".to_string(),
        all_tokens: vec![
            "APT".to_string(),
            "USDC".to_string(),
            "USDT".to_string(),
            "BTC".to_string(),
        ],
        all_reserves: vec![
            "1000000".to_string(),
            "500000".to_string(),
            "750000".to_string(),
            "100".to_string(),
        ],
        all_weights: vec![25, 25, 25, 25], // Weighted pool
        pool_type: "weighted".to_string(),
        block_height: 54321,
        additional_data: serde_json::to_vec(&serde_json::json!({
            "pool_type": "weighted",
            "swap_fee": "0.003",
            "weights": [25, 25, 25, 25]
        }))
        .unwrap(),
    };

    let test_batch = RecordedBatch {
        start_version: 54321,
        end_version: 54322,
        timestamp_ms: 1640995300000,
        transactions: vec![],
        pool_initializations: vec![multi_token_pool],
    };

    let temp_pb_file = create_test_protobuf_file(vec![test_batch]);
    let temp_json_file = NamedTempFile::new().expect("Failed to create temp JSON file");

    // Convert to JSON
    let output = std::process::Command::new("cargo")
        .args(&["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_json_file.path())
        .output()
        .expect("Failed to run mdi-converter");

    assert!(output.status.success(), "Multi-token conversion failed");

    // Verify JSON contains multi-token data
    let json_content =
        fs::read_to_string(temp_json_file.path()).expect("Failed to read JSON output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_content).expect("Invalid JSON output");

    let pool = &json_value["batches"][0]["pool_initializations"][0];

    // Verify multi-token fields
    let all_tokens = pool["all_tokens"]
        .as_array()
        .expect("all_tokens should be array");
    assert_eq!(all_tokens.len(), 4, "Should have 4 tokens");
    assert_eq!(all_tokens[0], "APT");
    assert_eq!(all_tokens[3], "BTC");

    let all_reserves = pool["all_reserves"]
        .as_array()
        .expect("all_reserves should be array");
    assert_eq!(all_reserves.len(), 4, "Should have 4 reserves");

    let all_weights = pool["all_weights"]
        .as_array()
        .expect("all_weights should be array");
    assert_eq!(all_weights.len(), 4, "Should have 4 weights");
    assert_eq!(all_weights[0], 25);

    assert_eq!(pool["pool_type"], "weighted");
}

#[test]
fn test_empty_batch_conversion() {
    // Test conversion of empty batch (no transactions, no pools)
    let empty_batch = RecordedBatch {
        start_version: 99999,
        end_version: 99999,
        timestamp_ms: 1640995400000,
        transactions: vec![],
        pool_initializations: vec![],
    };

    let temp_pb_file = create_test_protobuf_file(vec![empty_batch]);
    let temp_json_file = NamedTempFile::new().expect("Failed to create temp JSON file");

    // Convert to JSON
    let output = std::process::Command::new("cargo")
        .args(&["run", "--bin", "mdi-converter", "--", "to-json"])
        .arg("--input")
        .arg(temp_pb_file.path())
        .arg("--output")
        .arg(temp_json_file.path())
        .output()
        .expect("Failed to run mdi-converter");

    assert!(output.status.success(), "Empty batch conversion failed");

    // Verify JSON structure
    let json_content =
        fs::read_to_string(temp_json_file.path()).expect("Failed to read JSON output");

    let json_value: serde_json::Value =
        serde_json::from_str(&json_content).expect("Invalid JSON output");

    let batch = &json_value["batches"][0];
    assert_eq!(batch["start_version"], 99999);
    assert_eq!(batch["transactions"].as_array().unwrap().len(), 0);
    assert_eq!(batch["pool_initializations"].as_array().unwrap().len(), 0);

    // Verify metadata
    let metadata = &json_value["metadata"];
    assert_eq!(metadata["batch_count"], 1);
    assert_eq!(metadata["total_transactions"], 0);
    assert_eq!(metadata["total_pools_discovered"], 0);
}
