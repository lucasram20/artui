//! Session persistence — SQLite-backed session store with WAL mode.
//!
//! Stores sessions, messages, and memory entries. ULID-based IDs for
//! lexicographic sorting. Write-through on every message append.

pub mod store;

pub use store::SessionStore;
