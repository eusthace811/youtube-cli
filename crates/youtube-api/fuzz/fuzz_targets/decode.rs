//! Any bytes the API (or a hostile server) returns must decode to a value or
//! an error, never a panic, and every decoded value must encode.
#![no_main]

use libfuzzer_sys::fuzz_target;
use youtube_api::{decode, error::ApiError, generated, json, yaml};

fuzz_target!(|data: &[u8]| {
    for ty in [generated::VIDEO_LIST_RESPONSE, generated::PLAYLIST, generated::CHANNEL_LIST_RESPONSE] {
        if let Ok(v) = decode::decode(ty, data) {
            let _ = json::value(&v, false);
            let _ = json::value(&v, true);
            let _ = yaml::value(&v);
        }
    }
    let _ = ApiError::from_body(400, data).to_string();
});
