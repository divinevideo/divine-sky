//! Divine ATBridge — bridge service between Nostr and AT Protocol.
//!
//! Wires together the full pipeline:
//!   nostr_consumer → signature verify → translator → blob upload → publisher
//!
//! The pipeline is trait-based: `AccountStore`, `RecordStore`, `BlobFetcher`,
//! `BlobUploader`, and `PdsPublisher` are all traits so the orchestration
//! logic can be tested with mocks.  Concrete implementations backed by a
//! database and HTTP clients will be provided when deploying.

pub mod config;
pub mod deletion;
pub mod nostr_consumer;
pub mod pipeline;
pub mod provisioner;
pub mod publisher;
pub mod signature;
pub mod text_builder;
pub mod translator;

fn main() {
    println!("divine-atbridge");
    println!();
    println!("Pipeline wiring:");
    println!("  1. NostrConsumer subscribes to relay (funnelcake) for kinds 34235, 34236, 5");
    println!("  2. On each EVENT, pipeline.process_event(event) is called:");
    println!("     a. Verify Nostr signature (signature.rs)");
    println!("     b. Check account linkage + opt-in (AccountStore trait)");
    println!("     c. Check idempotency (RecordStore trait)");
    println!("     d. For video events: fetch blob → upload to PDS → translate → publish");
    println!("     e. For deletion events: look up mapping → delete from PDS → mark deleted");
    println!("  3. Results are logged via tracing");
    println!();
    println!("Run `cargo test -p divine-atbridge` to verify the pipeline logic.");
}
