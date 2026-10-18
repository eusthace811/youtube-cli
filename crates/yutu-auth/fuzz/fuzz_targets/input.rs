#![no_main]

use libfuzzer_sys::fuzz_target;
use yutu_auth::flow::ClientConfig;
use yutu_auth::{Token, gobase64, gojson, gopath, input};

fuzz_target!(|data: &[u8]| {
    let valid_json = gojson::is_json(data);

    if let Ok(decoded) = gobase64::std_decode(data) {
        let reencoded = gobase64::std_encode(&decoded);
        assert_eq!(gobase64::std_decode(reencoded.as_bytes()).unwrap(), decoded);
    }

    if let Ok(token) = Token::from_go_json(data) {
        assert!(valid_json, "a token decoded from invalid JSON");
        let bytes = token.to_go_json();
        assert!(gojson::is_json(&bytes), "token encoded to invalid JSON");
        assert_eq!(Token::from_go_json(&bytes).unwrap(), token);
    }

    let _ = ClientConfig::from_json(data);

    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(secret) = input::decode_inline(text) {
            let decoded = gobase64::std_decode(data);
            assert!(decoded.is_ok() || valid_json);
            assert_eq!(secret.expose(), decoded.unwrap_or_else(|_| data.to_vec()).as_slice());
        }
        let cleaned = gopath::clean(text);
        assert_eq!(gopath::clean(&cleaned), cleaned);
        let _ = gopath::rel("/root/dir", &format!("/{text}"));
        let _ = gopath::ext(text);
    }
});
