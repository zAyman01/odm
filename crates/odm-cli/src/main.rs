//! Command-line client for the odm daemon.
#![forbid(unsafe_code)]

fn main() {
    println!("{} {}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"));
}
