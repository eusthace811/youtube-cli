//! Decode a 500-video VideoListResponse and re-encode its items as JSON, the
//! work behind `youtube video list -o json`. Prints the median of 20 runs.
//!
//!     BIG_FIXTURE=/tmp/big/fx/GET_videos.json cargo bench -p youtube-api --bench decode

use std::time::Instant;
use youtube_api::generated::VIDEO_LIST_RESPONSE;
use youtube_api::{decode, json};

fn main() {
    let path = std::env::var("BIG_FIXTURE").expect("BIG_FIXTURE");
    let raw = std::fs::read(path).unwrap();
    let mut times = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let v = decode::decode(VIDEO_LIST_RESPONSE, &raw).unwrap();
        let out = json::list(
            v.as_struct().unwrap().get("items").unwrap().as_list(),
            false,
        );
        std::hint::black_box(out);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "decode+encode 500 videos: median {:.2} ms (min {:.2}, max {:.2})",
        times[10], times[0], times[19]
    );
}
