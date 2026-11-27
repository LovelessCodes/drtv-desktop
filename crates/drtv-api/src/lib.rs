//! DR TV, as a desktop client needs it: the wire types, the catalogue calls, the sign-in flow and
//! the mapping from a DR item to the card the UI draws.
//!
//! The service this talks to is the one DR's TV apps use, not a public API. It is unofficial,
//! undocumented and may change without notice; everything here is measured behaviour.
//!
//! Nothing in this crate knows about windows, widgets or players, which is what lets the app swap
//! its UI without touching the service model.

pub mod api;
pub mod auth;
pub mod card;
pub mod session;

/// Resolve a service base URL from an override, falling back to the measured default. The override
/// exists so a host build can be pointed at a captured fixture server (`tests/mock_dr.py`).
pub(crate) fn base(default: &str, var: &str) -> String {
    match std::env::var(var) {
        Ok(value) if !value.trim().is_empty() => value.trim_end_matches('/').to_string(),
        _ => default.to_string(),
    }
}

/// A one-line trace of something the service said. The client's own event log was a file on the TV;
/// here the subscriber decides where it goes.
pub(crate) fn log(message: &str) {
    tracing::debug!(target: "drtv_api", "{message}");
}
