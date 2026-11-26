//! Kea's engine: fetch a deployed contract, draft claims and the checks that decide them, prove
//! them, replay what breaks, review the words against the checks, and keep the best attempt.
//!
//! The command line and the API both drive it.

pub mod ask;
pub mod prove;
pub mod review;
