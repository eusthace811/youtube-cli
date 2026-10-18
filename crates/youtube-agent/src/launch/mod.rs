//! The web launchers: ADK's REST API, its dev UI, A2A and the Pub/Sub and
//! Eventarc triggers, behind a per-launch bearer token (divergence E11).

pub mod a2a;
pub mod api;
pub mod flags;
pub mod http;
mod origin;
pub mod session;
pub mod triggers;
pub mod web;
pub mod webui;
pub mod wire;
