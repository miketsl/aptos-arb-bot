use common::types::{Quantity, TickInfo};
use rust_decimal::Decimal;
use rust_decimal::MathematicalOps;
use std::collections::HashMap;
use std::sync::Arc;

fn sqrt_price_to_price(sqrt_price: u128) -> Decimal {
    let price_q64 = Decimal::from(sqrt_price) / Decimal::from(2u128.pow(64));
    price_q64 * price_q64
}

pub fn quote_clmm(
    amount_in: Decimal,
    sqrt_price: u128,
    liquidity: u128,
    tick: i32,
    _tick_map: Arc<HashMap<i32, TickInfo>>,
) -> Option<Quantity> {
    if liquidity == 0 {
        return None;
    }

    let mut remaining_in = amount_in;
    let mut total_out = Decimal::ZERO;
    let _current_tick = tick;
    let current_sqrt_price = sqrt_price;

    // Simplified iteration logic. A full implementation would need to handle tick boundaries.
    while remaining_in > Decimal::ZERO {
        // This is a simplified model. A real implementation would need to calculate next_tick and sqrt_price_next_tick
        // For now, we assume all liquidity is in the current tick for simplicity.
        let price = sqrt_price_to_price(current_sqrt_price);
        if price.is_zero() {
            return None; // Avoid division by zero
        }

        let amount_out = remaining_in * price;
        total_out += amount_out;
        remaining_in = Decimal::ZERO; // Exit loop for this simplified version
    }

    if total_out <= Decimal::ZERO {
        None
    } else {
        Some(Quantity(total_out))
    }
}

pub fn quote_stableswap(
    amount_in: Decimal,
    reserves: &[Quantity],
    amplification: u128,
) -> Option<Quantity> {
    if reserves.len() != 2 || amplification == 0 {
        return None;
    }
    // Simplified placeholder logic
    let r_in = reserves[0].0;
    let r_out = reserves[1].0;
    if r_in.is_zero() || r_out.is_zero() {
        return None;
    }

    // Simplified formula, does not use amplification factor correctly
    let amount_out = (r_out * amount_in) / (r_in + amount_in);

    if amount_out <= Decimal::ZERO {
        None
    } else {
        Some(Quantity(amount_out))
    }
}

pub fn quote_weighted(
    amount_in: Decimal,
    reserves: &[Quantity],
    weights: &[u32],
) -> Option<Quantity> {
    if reserves.len() != 2 || weights.len() != 2 {
        return None;
    }

    let r_in = reserves[0].0;
    let r_out = reserves[1].0;
    let w_in = Decimal::from(weights[0]) / Decimal::from(1_000_000); // Assuming weights are scaled
    let w_out = Decimal::from(weights[1]) / Decimal::from(1_000_000);

    if r_in.is_zero() || r_out.is_zero() || w_in.is_zero() || w_out.is_zero() {
        return None;
    }

    // Balancer formula: amountOut = balanceOut * (1 - (balanceIn / (balanceIn + amountIn))^(weightIn / weightOut))
    let ratio = r_in / (r_in + amount_in);
    let power = w_in / w_out;
    let factor = Decimal::ONE - ratio.powd(power);
    let amount_out = r_out * factor;

    if amount_out <= Decimal::ZERO {
        None
    } else {
        Some(Quantity(amount_out))
    }
}
