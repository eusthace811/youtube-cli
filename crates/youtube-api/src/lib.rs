#![forbid(unsafe_code)]

pub mod client;
pub mod decode;
pub mod error;
#[rustfmt::skip]
pub mod generated;
pub mod json;
mod media;
pub mod schema;
pub mod sniff;
pub mod value;
pub mod yaml;

pub use client::{Call, Media, Service, TokenSource};
pub use error::{ApiError, Error};
pub use value::{Struct, Value};
