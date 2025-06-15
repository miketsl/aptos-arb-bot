//! Main runtime for the arbitrage bot.

fn main() {
    core::init();
    detector::init();
    executor::init();
    analytics::init();
    println!("Hello, arb-bot!");
}
