//! Cover image sourcing for games.
//!
//! Tiered pipeline: BoardGameGeek lookup → first-page rulebook thumbnail →
//! manual override. Runs once per game after first successful ingest.

pub mod auto;
pub mod bgg;
