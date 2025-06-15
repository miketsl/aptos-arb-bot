//! Main runtime for the arbitrage bot.

fn main() {
    common::init();
    core::init();
    dex_adapter_trait::init();
    detector::init();
    executor::init();
    analytics::init();
    println!("Hello, arb-bot!");
}