//! Prints the API endpoint this build resolves, for the parity-feature lane.

fn main() {
    println!("{}", yutu_api::client::endpoint());
}
