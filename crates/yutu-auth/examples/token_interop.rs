//! Go interop check for token files.
//!
//! `token_interop write <dir> <file>` saves the reference token through the
//! store; `token_interop load <path>` prints a token file the way the Go
//! oracle's `load` mode prints it (`%q %q %q %s %d %v`).

use chrono::{Local, TimeZone};
use std::sync::Arc;
use yutu_auth::log::quote;
use yutu_auth::store::TokenStore;
use yutu_auth::token::GoTime;
use yutu_auth::{RootDir, Token};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args[1].as_str() {
        "write" => {
            let expiry = Local
                .with_ymd_and_hms(2026, 10, 18, 17, 5, 6)
                .unwrap()
                .fixed_offset()
                + chrono::TimeDelta::nanoseconds(123_456_789);
            let token = Token {
                access_token: "ya29.a0Af<&>\u{2028}\"x\"".to_string(),
                token_type: "Bearer".to_string(),
                refresh_token: "1//0gRefresh".to_string(),
                expiry: GoTime::new(expiry),
                expires_in: 3599,
            };
            let root = Arc::new(RootDir::open(&args[2]).unwrap());
            TokenStore::new(root, args[3].clone()).save(&token).unwrap();
        }
        "load" => {
            let t = Token::from_go_json(&std::fs::read(&args[2]).unwrap());
            match t {
                Ok(t) => println!(
                    "{} {} {} {} {} {} err=<nil>",
                    quote(&t.access_token),
                    quote(&t.token_type),
                    quote(&t.refresh_token),
                    t.expiry.to_rfc3339_nano(),
                    t.expires_in,
                    t.valid()
                ),
                Err(e) => println!("err={e}"),
            }
        }
        other => panic!("unknown mode {other}"),
    }
}
