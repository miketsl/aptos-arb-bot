//! Gas estimation and cost accounting for arbitrage opportunities.

use crate::exchange_const::Exchange;
use crate::prelude::*;
use common::errors::CommonError;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Configuration for gas estimation.
#[derive(Debug, Clone)]
pub struct GasConfig {
    /// Base gas cost for a transaction
    pub base_gas_cost: u64,
    /// Gas cost per swap operation
    pub gas_per_swap: u64,
    /// Current gas unit price (in APT)
    pub gas_unit_price: Decimal,
    /// Maximum gas limit for transactions
    pub max_gas_limit: u64,
    /// Gas estimation buffer multiplier (e.g., 1.2 = 20% buffer)
    pub estimation_buffer: f64,
    /// Aptos RPC endpoint for simulation
    pub rpc_endpoint: String,
}

impl Default for GasConfig {
    fn default() -> Self {
        Self {
            base_gas_cost: 1000,                  // Base transaction cost
            gas_per_swap: 500,                    // Per-swap gas cost
            gas_unit_price: Decimal::new(100, 8), // 0.000001 APT per gas unit
            max_gas_limit: 2_000_000,             // 2M gas limit
            estimation_buffer: 1.2,               // 20% buffer
            rpc_endpoint: "https://fullnode.devnet.aptoslabs.com/v1".to_string(),
        }
    }
}

/// Gas cost calculator and simulator.
pub struct GasCalculator {
    config: GasConfig,
    client: reqwest::Client,
}

impl GasCalculator {
    /// Creates a new gas calculator with the given configuration.
    pub fn new(config: GasConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Creates a new gas calculator with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(GasConfig::default())
    }

    /// Updates the current gas unit price (typically fetched from on-chain).
    pub fn update_gas_price(&mut self, new_price: Decimal) {
        self.config.gas_unit_price = new_price;
    }

    /// Estimates gas cost for a given arbitrage path.
    pub fn estimate_gas_cost(&self, path: &[(Asset, Exchange)]) -> u64 {
        let swap_count = path.len() as u64;
        let base_estimate = self.config.base_gas_cost + (swap_count * self.config.gas_per_swap);

        // Apply estimation buffer
        let buffered_estimate = (base_estimate as f64 * self.config.estimation_buffer) as u64;

        // Ensure we don't exceed max gas limit
        buffered_estimate.min(self.config.max_gas_limit)
    }

    /// Simulates a transaction to get precise gas usage using Aptos simulate() RPC.
    pub async fn simulate_transaction(
        &self,
        path: &[(Asset, Exchange)],
        amount: &Quantity,
    ) -> Result<u64, CommonError> {
        let transaction_payload = self.build_transaction_payload(path, amount)?;

        let simulation_request = serde_json::json!({
            "sender": "0x1", // Placeholder sender
            "sequence_number": "0",
            "max_gas_amount": self.config.max_gas_limit.to_string(),
            "gas_unit_price": self.config.gas_unit_price.to_string(),
            "payload": transaction_payload
        });

        let response = self
            .client
            .post(format!(
                "{}/transactions/simulate",
                self.config.rpc_endpoint
            ))
            .json(&simulation_request)
            .send()
            .await
            .map_err(|e| CommonError::ExternalServiceError(format!("RPC call failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(CommonError::ExternalServiceError(format!(
                "Simulation failed: {}",
                error_text
            )));
        }

        let simulation_result: SimulationResponse = response.json().await.map_err(|e| {
            CommonError::ParseError(format!("Failed to parse simulation response: {}", e))
        })?;

        if !simulation_result.success {
            return Err(CommonError::ExternalServiceError(format!(
                "Transaction simulation failed: {}",
                simulation_result.vm_status.unwrap_or_default()
            )));
        }

        let gas_used = simulation_result
            .gas_used
            .parse::<u64>()
            .map_err(|_| CommonError::ParseError("Invalid gas_used value".to_string()))?;

        if gas_used > self.config.max_gas_limit {
            return Err(CommonError::InvalidConfiguration(
                "Gas usage exceeds limit".to_string(),
            ));
        }

        Ok(gas_used)
    }

    /// Builds the transaction payload for the arbitrage sequence.
    fn build_transaction_payload(
        &self,
        path: &[(Asset, Exchange)],
        amount: &Quantity,
    ) -> Result<serde_json::Value, CommonError> {
        let mut function_calls = Vec::new();

        for i in 0..path.len() {
            let (current_asset, exchange) = &path[i];
            let next_asset = &path[(i + 1) % path.len()].0;

            // Build swap function call based on exchange
            let swap_function = match exchange.as_str() {
                "PancakeSwap" => {
                    serde_json::json!({
                        "function": "0x1::pancakeswap::swap_exact_input",
                        "type_arguments": [
                            self.asset_to_type_string(current_asset)?,
                            self.asset_to_type_string(next_asset)?
                        ],
                        "arguments": [
                            if i == 0 { amount.0.to_string() } else { "0".to_string() }, // Amount for first swap, 0 for others (use output)
                            "0" // Min amount out (will be calculated)
                        ]
                    })
                }
                _ => {
                    return Err(CommonError::InvalidConfiguration(format!(
                        "Unsupported exchange: {}",
                        exchange
                    )));
                }
            };

            function_calls.push(swap_function);
        }

        Ok(serde_json::json!({
            "type": "entry_function_payload",
            "function": "0x1::batch_swap::execute_arbitrage",
            "type_arguments": [],
            "arguments": [
                serde_json::to_string(&function_calls)
                    .map_err(|e| CommonError::ParseError(format!("Failed to serialize function calls: {}", e)))?
            ]
        }))
    }

    /// Converts an Asset to its Aptos type string representation.
    fn asset_to_type_string(&self, asset: &Asset) -> Result<String, CommonError> {
        match asset.0.as_str() {
            s if s.contains("::") => Ok(s.to_string()), // Already a type string
            "USDC" => Ok("0x1::coin::USDC".to_string()),
            "APT" => Ok("0x1::aptos_coin::AptosCoin".to_string()),
            "ETH" => Ok("0x1::coin::ETH".to_string()),
            _ => Ok(asset.0.clone()),
        }
    }

    /// Fetches current gas price from the network.
    pub async fn fetch_current_gas_price(&mut self) -> Result<Decimal, CommonError> {
        let response = self
            .client
            .get(format!("{}/estimate_gas_price", self.config.rpc_endpoint))
            .send()
            .await
            .map_err(|e| {
                CommonError::ExternalServiceError(format!("Failed to fetch gas price: {}", e))
            })?;

        let gas_estimate: GasPriceEstimate = response.json().await.map_err(|e| {
            CommonError::ParseError(format!("Failed to parse gas price response: {}", e))
        })?;

        let gas_price = gas_estimate
            .gas_estimate
            .parse::<u64>()
            .map_err(|_| CommonError::ParseError("Invalid gas price estimate".to_string()))?;

        let gas_price_decimal = Decimal::from(gas_price);
        self.update_gas_price(gas_price_decimal);

        Ok(gas_price_decimal)
    }

    /// Calculates the gas cost in terms of the starting asset.
    pub fn calculate_gas_cost_in_asset(
        &self,
        gas_used: u64,
        start_asset: &Asset,
        oracle_prices: &HashMap<Asset, Decimal>,
    ) -> Result<Decimal, CommonError> {
        // Gas cost in APT
        let gas_cost_apt = Decimal::from(gas_used) * self.config.gas_unit_price;

        if start_asset.0 == "APT" || start_asset.0.contains("AptosCoin") {
            return Ok(gas_cost_apt);
        }

        // Convert APT price to start asset using oracle
        let apt_asset = Asset("APT".to_string());
        let apt_price_in_start_asset = oracle_prices.get(&apt_asset).ok_or_else(|| {
            CommonError::NotFound(format!("Price for APT in terms of {}", start_asset.0))
        })?;

        Ok(gas_cost_apt * apt_price_in_start_asset)
    }

    /// Evaluates a cycle with gas costs included to calculate net profit.
    pub async fn evaluate_cycle_with_gas(
        &self,
        cycle: &PathQuote<Exchange>,
        oracle_prices: &HashMap<Asset, Decimal>,
    ) -> Result<CycleEval, CommonError> {
        // Use actual simulation for precise gas estimation
        let gas_used = self
            .simulate_transaction(&cycle.path, &cycle.amount_in)
            .await?;

        let start_asset = &cycle
            .path
            .first()
            .ok_or_else(|| CommonError::InvalidConfiguration("Empty cycle path".to_string()))?
            .0;

        let gas_cost_in_start_asset =
            self.calculate_gas_cost_in_asset(gas_used, start_asset, oracle_prices)?;

        let gross_profit = cycle.amount_out.0 - cycle.amount_in.0;
        let net_profit = gross_profit - gas_cost_in_start_asset;

        Ok(CycleEval {
            gross_profit,
            gas_estimate: gas_used,
            gas_unit_price: self.config.gas_unit_price,
            net_profit,
        })
    }

    /// Filters cycles by net profitability after gas costs.
    pub async fn filter_profitable_cycles(
        &self,
        cycles: Vec<PathQuote<Exchange>>,
        oracle_prices: &HashMap<Asset, Decimal>,
        min_net_profit: Decimal,
    ) -> Vec<(PathQuote<Exchange>, CycleEval)> {
        let mut profitable_cycles = Vec::new();

        for cycle in cycles {
            if let Ok(eval) = self.evaluate_cycle_with_gas(&cycle, oracle_prices).await {
                if eval.net_profit >= min_net_profit {
                    profitable_cycles.push((cycle, eval));
                }
            }
        }

        profitable_cycles
    }
}

/// Response structure for Aptos simulation RPC.
#[derive(Debug, serde::Deserialize)]
struct SimulationResponse {
    success: bool,
    gas_used: String,
    vm_status: Option<String>,
}

/// Response structure for gas price estimation.
#[derive(Debug, serde::Deserialize)]
struct GasPriceEstimate {
    gas_estimate: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    fn create_test_calculator() -> GasCalculator {
        GasCalculator::with_defaults()
    }

    fn create_test_path() -> Vec<(Asset, Exchange)> {
        vec![
            (Asset::from_str("USDC").unwrap(), Exchange::PancakeSwap),
            (Asset::from_str("APT").unwrap(), Exchange::PancakeSwap),
            (Asset::from_str("ETH").unwrap(), Exchange::PancakeSwap),
        ]
    }

    fn create_test_oracle() -> HashMap<Asset, Decimal> {
        let mut prices = HashMap::new();
        prices.insert(Asset("APT".to_string()), dec!(8)); // $8
        prices.insert(Asset("USDC".to_string()), dec!(1)); // $1
        prices.insert(Asset("ETH".to_string()), dec!(2000)); // $2000
        prices
    }

    #[test]
    fn test_calculator_creation() {
        let calc = create_test_calculator();
        assert!(calc.config.base_gas_cost > 0);
        assert!(calc.config.gas_per_swap > 0);
        assert!(calc.config.gas_unit_price > Decimal::ZERO);
    }

    #[test]
    fn test_estimate_gas_cost() {
        let calc = create_test_calculator();
        let path = create_test_path();

        let gas_estimate = calc.estimate_gas_cost(&path);

        // Should be base cost + (3 swaps * gas_per_swap) * buffer
        let expected_base = calc.config.base_gas_cost + (3 * calc.config.gas_per_swap);
        let expected_buffered = (expected_base as f64 * calc.config.estimation_buffer) as u64;

        assert_eq!(gas_estimate, expected_buffered);
    }

    #[test]
    fn test_update_gas_price() {
        let mut calc = create_test_calculator();
        let new_price = dec!(0.000002);

        calc.update_gas_price(new_price);

        assert_eq!(calc.config.gas_unit_price, new_price);
    }

    #[test]
    fn test_asset_to_type_string() {
        let calc = create_test_calculator();

        assert_eq!(
            calc.asset_to_type_string(&Asset("USDC".to_string()))
                .unwrap(),
            "0x1::coin::USDC"
        );

        assert_eq!(
            calc.asset_to_type_string(&Asset("APT".to_string()))
                .unwrap(),
            "0x1::aptos_coin::AptosCoin"
        );

        // Already a type string
        assert_eq!(
            calc.asset_to_type_string(&Asset("0x1::custom::Token".to_string()))
                .unwrap(),
            "0x1::custom::Token"
        );
    }

    #[test]
    fn test_calculate_gas_cost_in_apt() {
        let calc = create_test_calculator();
        let oracle = create_test_oracle();
        let apt_asset = Asset("APT".to_string());

        let gas_cost = calc.calculate_gas_cost_in_asset(10000, &apt_asset, &oracle);

        assert!(gas_cost.is_ok());
        let cost = gas_cost.unwrap();
        assert_eq!(cost, Decimal::from(10000) * calc.config.gas_unit_price);
    }

    #[test]
    fn test_calculate_gas_cost_in_other_asset() {
        let calc = create_test_calculator();
        let oracle = create_test_oracle();
        let usdc_asset = Asset("USDC".to_string());

        let gas_cost = calc.calculate_gas_cost_in_asset(10000, &usdc_asset, &oracle);

        // Should convert APT cost to USDC using oracle price
        assert!(gas_cost.is_ok());
        let cost = gas_cost.unwrap();
        assert!(cost > Decimal::ZERO);
    }

    #[test]
    fn test_build_transaction_payload() {
        let calc = create_test_calculator();
        let path = create_test_path();
        let amount = Quantity(dec!(100));

        let payload = calc.build_transaction_payload(&path, &amount);

        assert!(payload.is_ok());
        let payload_json = payload.unwrap();
        assert!(payload_json.get("type").is_some());
        assert!(payload_json.get("function").is_some());
    }
}
