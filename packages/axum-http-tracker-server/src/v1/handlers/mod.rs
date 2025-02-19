//! Axum [`handlers`](axum#handlers) for the HTTP server.
//!
//! Refer to the generic [HTTP server documentation](crate::servers::http) for
//! more information about the HTTP tracker.
pub mod announce;
pub mod common;
pub mod health_check;
pub mod scrape;
