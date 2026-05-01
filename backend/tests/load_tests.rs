//! Load and stress test suite entry point.
//!
//! Run with:
//! ```bash
//! cargo test -p backend --test load_tests -- --nocapture
//! ```

mod load {
    pub mod profile_load;
    pub mod status_load;
}
