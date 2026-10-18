//! Prints the API endpoint this build resolves, for the parity-feature lane.

fn main() {
    println!("{}", youtube_api::client::endpoint());
}
