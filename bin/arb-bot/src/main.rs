//! Main runtime for the arbitrage bot.

fn main() {
    core::init();
    dex_adapter_trait::init();
    detector::init();
    executor::init();
    analytics::init();
    println!("Hello, arb-bot!");
}
