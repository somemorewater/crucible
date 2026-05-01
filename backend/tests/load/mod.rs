//! Load and stress tests for the backend API.
//!
//! These tests exercise the API under concurrent load to verify that the
//! server remains stable and responsive. They are gated behind the
//! `load_tests` feature flag so they don't run in normal CI:
//!
//! ```bash
//! cargo test -p backend --test load_tests -- --nocapture
//! ```

pub mod status_load;
pub mod profile_load;
