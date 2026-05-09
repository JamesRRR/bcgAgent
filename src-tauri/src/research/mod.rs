//! Import-time knowledge research: pull as much external context as we can
//! (BGG description, forum threads, gallery captions) and persist to
//! `game_external_refs` so RAG and the walkthrough coach have it.

pub mod bgg_extra;
pub mod pipeline;
