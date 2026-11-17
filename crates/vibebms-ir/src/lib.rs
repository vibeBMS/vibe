//! Internet Ranking (IR): which plays may go where (`eligibility`, `rule`),
//! one adapter per service behind the `Provider` trait, and the lamp
//! model.
//!
//! The crate is HTTP-free and clock-free: it builds [`HttpRequest`]s and
//! reads [`HttpResponse`]s; the application sends them with its own HTTP
//! stack and passes the time in.

pub mod eligibility;
pub mod http;
pub mod lamps;
pub mod limiter;
pub mod outbox;
pub mod provider;
pub mod providers;
pub mod rule;

pub use http::{HttpRequest, HttpResponse, Secret};
pub use provider::{Op, Outcome, Provider, ProviderId};
