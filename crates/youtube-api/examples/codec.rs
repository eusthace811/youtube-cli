//! Decodes a JSON file as a Go API type and prints one encoding of it.
//!
//!     cargo run -p youtube-api --example codec -- VideoListResponse file.json json|yaml|items-json|items-yaml

use youtube_api::generated::TYPES;
use youtube_api::{decode, json, yaml};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ty = TYPES
        .iter()
        .position(|t| t.name == args[0])
        .expect("unknown type");
    let bytes = std::fs::read(&args[1]).expect("read input");
    let v = decode::decode(ty, &bytes).unwrap_or_else(|e| panic!("{e}"));
    let items = || {
        v.as_struct()
            .and_then(|s| s.get("items"))
            .map(|i| i.as_list().to_vec())
            .unwrap_or_default()
    };
    let out = match args[2].as_str() {
        "json" => json::value(&v, false),
        "yaml" => yaml::value(&v),
        "items-json" => json::list(&items(), false),
        "items-yaml" => yaml::list(&items()),
        other => panic!("unknown format {other}"),
    };
    print!("{out}");
}
