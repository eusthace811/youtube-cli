//! Sends one videos.list request to a base URL and reports the outcome and
//! elapsed time, for the timeout lane.

use std::time::Instant;
use yutu_api::Service;

fn main() {
    let base = std::env::args().nth(1).expect("base URL");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let svc = Service::with_base(base, None);
    let started = Instant::now();
    let result = rt.block_on(
        svc.call("youtube.videos.list")
            .params("part", &["id".into()])
            .send(),
    );
    match result {
        Ok(_) => println!("ok after {:.1}s", started.elapsed().as_secs_f64()),
        Err(e) => println!("error after {:.1}s: {e}", started.elapsed().as_secs_f64()),
    }
}
