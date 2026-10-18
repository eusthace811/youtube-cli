//! A stand-in CLI for checking ops against the parity corpus before the
//! cobra-compatible engine lands. Handles only the flag syntax the corpus
//! uses for op cases; gating is by `--yes` only.

use std::collections::HashMap;
use std::io::IsTerminal;
use yutu_api::Service;
use yutu_api::client::BoxFuture;
use yutu_core::ops::{self, Ctx, FlagValues, ServiceProvider};
use yutu_core::output::Out;
use yutu_core::registry::{self, FlagKind};

struct Parsed {
    values: HashMap<String, String>,
    slices: HashMap<String, Vec<String>>,
    changed: Vec<String>,
    cmd: &'static registry::Command,
}

impl FlagValues for Parsed {
    fn string(&self, n: &str) -> String {
        self.values
            .get(n)
            .cloned()
            .unwrap_or_else(|| self.default(n))
    }
    fn bool(&self, n: &str) -> bool {
        self.string(n) == "true"
    }
    fn strings(&self, n: &str) -> Vec<String> {
        if let Some(v) = self.slices.get(n) {
            return v.clone();
        }
        let d = self.default(n);
        let d = d.trim_start_matches('[').trim_end_matches(']');
        if d.is_empty() {
            Vec::new()
        } else {
            d.split(',').map(str::to_string).collect()
        }
    }
    fn i64(&self, n: &str) -> i64 {
        self.string(n).parse().unwrap_or(0)
    }
    fn u64(&self, n: &str) -> u64 {
        self.string(n).parse().unwrap_or(0)
    }
    fn changed(&self, n: &str) -> bool {
        self.changed.iter().any(|c| c == n)
    }
}

impl Parsed {
    fn default(&self, n: &str) -> String {
        self.cmd
            .flags
            .iter()
            .find(|f| f.name == n)
            .map(|f| f.default.to_string())
            .unwrap_or_default()
    }
}

struct Parity;

impl ServiceProvider for Parity {
    fn service(&self) -> BoxFuture<'_, Result<Service, String>> {
        Box::pin(async { Ok(Service::new(None)) })
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = format!("{} {}", args[0], args[1]);
    let cmd = registry::find(&path).expect("command");
    let mut p = Parsed {
        values: HashMap::new(),
        slices: HashMap::new(),
        changed: Vec::new(),
        cmd,
    };
    let mut i = 2;
    while i < args.len() {
        let a = &args[i];
        let (name, inline) = if let Some(rest) = a.strip_prefix("--") {
            match rest.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (rest.to_string(), None),
            }
        } else if let Some(rest) = a.strip_prefix('-') {
            let f = cmd
                .flags
                .iter()
                .find(|f| f.short == rest)
                .expect("short flag");
            (f.name.to_string(), None)
        } else {
            i += 1;
            continue;
        };
        let flag = cmd.flags.iter().find(|f| f.name == name).expect("flag");
        let value = match (inline, flag.kind) {
            (Some(v), _) => v,
            (None, FlagKind::Bool) => "true".into(),
            (None, _) => {
                i += 1;
                args[i].clone()
            }
        };
        if flag.kind == FlagKind::StringSlice {
            let entry = p.slices.entry(name.clone()).or_default();
            entry.extend(
                value
                    .split(',')
                    .map(str::to_string)
                    .filter(|s| !s.is_empty()),
            );
        } else {
            p.values.insert(name.clone(), value);
        }
        p.changed.push(name);
        i += 1;
    }
    let entry = ops::find(&path).expect("op");
    let op = (entry.cli)(&p);
    if !op.confirm().is_empty() && !p.bool("yes") {
        eprintln!("{}", op.confirm());
        eprintln!("Error: operation not confirmed (pass confirm: true or rerun with --yes)");
        eprintln!("Run 'yutu {path} -h' to see usage.");
        std::process::exit(1);
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut stdout = std::io::stdout();
    let interactive = stdout.is_terminal() && std::env::var("CI").unwrap_or_default().is_empty();
    let mut ctx = Ctx::new(
        Out {
            w: &mut stdout,
            interactive,
        },
        &Parity,
    );
    if let Err(e) = rt.block_on(op.run(&mut ctx)) {
        eprintln!("Error: {e}");
        eprintln!("Run 'yutu {path} -h' to see usage.");
        std::process::exit(1);
    }
}
