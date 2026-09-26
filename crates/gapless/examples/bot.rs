//! The smallest useful integration: every Pump.fun transaction, exactly once, through outages.
//! Replace the `println!` with your bot's logic.
//!
//! ```bash
//! SOLAMI_API_KEY=... cargo run -p gapless --example bot
//! ```

use futures::StreamExt;
use gapless::{Event, Gapless};

#[tokio::main]
async fn main() -> Result<(), gapless::Error> {
    let key = std::env::var("SOLAMI_API_KEY").expect("set SOLAMI_API_KEY");
    let (mut events, _control) = Gapless::builder(key)
        .program("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
        .build()?
        .start();

    while let Some(event) = events.next().await {
        match event {
            Event::Transaction(tx) => println!("{} {}", tx.slot, tx.signature),
            Event::Recovered(i) => eprintln!(
                "back: {} replayed, {} dropped as duplicates",
                i.replayed, i.duplicates
            ),
            _ => {}
        }
    }
    Ok(())
}
