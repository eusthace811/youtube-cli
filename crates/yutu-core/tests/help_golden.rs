#![forbid(unsafe_code)]

use yutu_core::cli::{App, CmdError, Invocation, execute_with};

struct NoApp;

impl App for NoApp {
    fn pre_run(&self, inv: &Invocation) -> Result<(), CmdError> {
        panic!("--help reached pre_run for {}", inv.command.path)
    }

    fn run(&self, inv: &Invocation) -> Result<(), CmdError> {
        panic!("--help reached run for {}", inv.command.path)
    }
}

#[test]
fn every_help_text_matches_the_go_oracle() {
    let path = format!(
        "{}/../../parity/golden/help.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(golden.len(), 117);
    for (command, want) in &golden {
        let mut args: Vec<String> = command.split(' ').skip(1).map(String::from).collect();
        args.push("--help".to_string());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = execute_with(args, &NoApp, &mut out, &mut err);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            want["stdout"].as_str().unwrap(),
            "stdout of {command} --help"
        );
        assert_eq!(
            String::from_utf8(err).unwrap(),
            want["stderr"].as_str().unwrap(),
            "stderr of {command} --help"
        );
        assert_eq!(
            i64::from(code),
            want["exit"].as_i64().unwrap(),
            "exit of {command} --help"
        );
    }
}
