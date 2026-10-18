//! A pflag FlagSet for one command: its local flags, the persistent flags
//! of its ancestors, and cobra's `-h, --help` once the command executes.

use std::borrow::Cow;
use std::fmt;

use super::goconv;
use super::tree;
use crate::registry::{Command, Flag, FlagKind};

pub(crate) struct Spec {
    pub name: &'static str,
    pub short: &'static str,
    pub kind: FlagKind,
    pub default: &'static str,
    pub no_opt_default: &'static str,
    pub usage: Cow<'static, str>,
    pub hidden: bool,
    pub persistent: bool,
    pub required: bool,
    /// Declared by an ancestor and inherited through `persistent`.
    pub inherited: bool,
}

impl Spec {
    fn from_registry(f: &'static Flag, inherited: bool) -> Self {
        Spec {
            name: f.name,
            short: f.short,
            kind: f.kind,
            default: f.default,
            no_opt_default: f.no_opt_default,
            usage: Cow::Borrowed(f.usage),
            hidden: f.hidden,
            persistent: f.persistent,
            required: f.required,
            inherited,
        }
    }

    fn help(cmd: &Command) -> Self {
        Spec {
            name: "help",
            short: "h",
            kind: FlagKind::Bool,
            default: "false",
            no_opt_default: "true",
            usage: Cow::Owned(format!("help for {}", tree::name(cmd))),
            hidden: false,
            persistent: false,
            required: false,
            inherited: false,
        }
    }

    /// The name pflag shows in InvalidValueError.
    fn display(&self) -> String {
        if self.short.is_empty() {
            format!("--{}", self.name)
        } else {
            format!("-{}, --{}", self.short, self.name)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Bool(bool),
    Str(String),
    Strings(Vec<String>),
    Int(i64),
    Uint(u64),
}

impl Value {
    fn default_of(kind: FlagKind, default: &str) -> Value {
        match kind {
            FlagKind::Bool => Value::Bool(default == "true"),
            FlagKind::String => Value::Str(default.to_string()),
            FlagKind::StringSlice => {
                let inner = default
                    .strip_prefix('[')
                    .and_then(|d| d.strip_suffix(']'))
                    .unwrap_or(default);
                let items = if inner.is_empty() {
                    Vec::new()
                } else {
                    goconv::read_csv_record(inner).unwrap_or_default()
                };
                Value::Strings(items)
            }
            FlagKind::Int | FlagKind::Int64 => Value::Int(goconv::parse_int(default).unwrap_or(0)),
            FlagKind::Uint64 => Value::Uint(goconv::parse_uint(default).unwrap_or(0)),
        }
    }
}

pub(crate) struct Entry {
    pub spec: Spec,
    value: Value,
    pub changed: bool,
}

/// Parsed values of the invoked command's local and inherited flags.
pub struct Flags {
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FlagError {
    BadSyntax(String),
    Unknown(String),
    UnknownShort {
        c: u8,
        shorthands: String,
    },
    NeedsArg(String),
    NeedsArgShort {
        c: u8,
        shorthands: String,
    },
    Invalid {
        flag: String,
        value: String,
        cause: String,
    },
}

/// pflag widens a shorthand byte to a rune before quoting it.
fn short_rune(c: u8) -> String {
    goconv::quote_rune(char::from(c))
}

impl fmt::Display for FlagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlagError::BadSyntax(s) => write!(f, "bad flag syntax: {s}"),
            FlagError::Unknown(name) => write!(f, "unknown flag: --{name}"),
            FlagError::UnknownShort { c, shorthands } => {
                write!(
                    f,
                    "unknown shorthand flag: {} in -{shorthands}",
                    short_rune(*c)
                )
            }
            FlagError::NeedsArg(name) => write!(f, "flag needs an argument: --{name}"),
            FlagError::NeedsArgShort { c, shorthands } => {
                write!(
                    f,
                    "flag needs an argument: {} in -{shorthands}",
                    short_rune(*c)
                )
            }
            FlagError::Invalid { flag, value, cause } => {
                write!(
                    f,
                    "invalid argument {} for {} flag: {cause}",
                    goconv::quote(value),
                    goconv::quote(flag)
                )
            }
        }
    }
}

impl Flags {
    /// The flag set of `cmd`; `help` adds cobra's default help flag, which
    /// cobra only creates when the command executes or its help is shown.
    pub(crate) fn for_command(cmd: &'static Command, help: bool) -> Flags {
        let mut specs: Vec<Spec> = cmd
            .flags
            .iter()
            .map(|f| Spec::from_registry(f, false))
            .collect();
        let mut ancestor = tree::parent(cmd);
        while let Some(p) = ancestor {
            for f in p.flags.iter().filter(|f| f.persistent) {
                if !specs.iter().any(|s| s.name == f.name) {
                    specs.push(Spec::from_registry(f, true));
                }
            }
            ancestor = tree::parent(p);
        }
        if help && !specs.iter().any(|s| s.name == "help") {
            specs.push(Spec::help(cmd));
        }
        specs.sort_by(|a, b| a.name.cmp(b.name));
        let entries = specs
            .into_iter()
            .map(|spec| Entry {
                value: Value::default_of(spec.kind, spec.default),
                spec,
                changed: false,
            })
            .collect();
        Flags { entries }
    }

    pub(crate) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.spec.name == name)
    }

    pub(crate) fn lookup_short(&self, c: u8) -> Option<&Entry> {
        self.entries.iter().find(|e| e.spec.short.as_bytes() == [c])
    }

    // Like pflag's GetString and friends, a flag the command does not define,
    // or one of another type, reads as the zero value; Go callers ignore the
    // returned error.
    pub fn string(&self, name: &str) -> &str {
        match self.lookup(name).map(|e| &e.value) {
            Some(Value::Str(s)) => s,
            _ => "",
        }
    }

    pub fn bool(&self, name: &str) -> bool {
        matches!(self.lookup(name).map(|e| &e.value), Some(Value::Bool(true)))
    }

    pub fn strings(&self, name: &str) -> &[String] {
        match self.lookup(name).map(|e| &e.value) {
            Some(Value::Strings(v)) => v,
            _ => &[],
        }
    }

    pub fn i64(&self, name: &str) -> i64 {
        match self.lookup(name).map(|e| &e.value) {
            Some(Value::Int(n)) => *n,
            _ => 0,
        }
    }

    pub fn u64(&self, name: &str) -> u64 {
        match self.lookup(name).map(|e| &e.value) {
            Some(Value::Uint(n)) => *n,
            _ => 0,
        }
    }

    pub fn changed(&self, name: &str) -> bool {
        self.lookup(name).is_some_and(|e| e.changed)
    }

    /// Required flags left unset, in cobra's sorted order.
    pub(crate) fn missing_required(&self) -> Vec<&'static str> {
        self.entries
            .iter()
            .filter(|e| e.spec.required && !e.changed)
            .map(|e| e.spec.name)
            .collect()
    }

    fn set(&mut self, idx: usize, value: &str) -> Result<(), FlagError> {
        let entry = &mut self.entries[idx];
        let invalid = |cause: String| FlagError::Invalid {
            flag: entry.spec.display(),
            value: value.to_string(),
            cause,
        };
        let parsed = match entry.spec.kind {
            FlagKind::Bool => Value::Bool(goconv::parse_bool(value).map_err(invalid)?),
            FlagKind::String => Value::Str(value.to_string()),
            FlagKind::StringSlice => {
                let items = if value.is_empty() {
                    Vec::new()
                } else {
                    goconv::read_csv_record(value).map_err(invalid)?
                };
                match (&mut entry.value, entry.changed) {
                    (Value::Strings(existing), true) => {
                        existing.extend(items);
                        entry.value.clone()
                    }
                    _ => Value::Strings(items),
                }
            }
            FlagKind::Int | FlagKind::Int64 => {
                Value::Int(goconv::parse_int(value).map_err(invalid)?)
            }
            FlagKind::Uint64 => Value::Uint(goconv::parse_uint(value).map_err(invalid)?),
        };
        entry.value = parsed;
        entry.changed = true;
        Ok(())
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.spec.name == name)
    }

    fn index_short(&self, c: u8) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.spec.short.as_bytes() == [c])
    }

    /// pflag's FlagSet.Parse with interspersed arguments; returns the
    /// positional arguments.
    pub(crate) fn parse(&mut self, args: &[String]) -> Result<Vec<String>, FlagError> {
        let mut positional = Vec::with_capacity(args.len());
        let mut rest = args;
        while let Some((s, tail)) = rest.split_first() {
            rest = tail;
            if s.len() < 2 || !s.starts_with('-') {
                positional.push(s.clone());
                continue;
            }
            if s == "--" {
                positional.extend(rest.iter().cloned());
                break;
            }
            rest = if s.starts_with("--") {
                self.parse_long(s, rest)?
            } else {
                self.parse_short(s, rest)?
            };
        }
        Ok(positional)
    }

    fn parse_long<'a>(&mut self, s: &str, rest: &'a [String]) -> Result<&'a [String], FlagError> {
        let body = &s[2..];
        if body.is_empty() || body.starts_with(['-', '=']) {
            return Err(FlagError::BadSyntax(s.to_string()));
        }
        let (name, inline) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (body, None),
        };
        let idx = self
            .index(name)
            .ok_or_else(|| FlagError::Unknown(name.to_string()))?;
        let no_opt = self.entries[idx].spec.no_opt_default;
        let (value, rest) = match (inline, rest.split_first()) {
            (Some(v), _) => (v, rest),
            (None, _) if !no_opt.is_empty() => (no_opt, rest),
            (None, Some((v, tail))) => (v.as_str(), tail),
            (None, None) => return Err(FlagError::NeedsArg(name.to_string())),
        };
        self.set(idx, value)?;
        Ok(rest)
    }

    fn parse_short<'a>(
        &mut self,
        s: &str,
        mut rest: &'a [String],
    ) -> Result<&'a [String], FlagError> {
        let mut shorthands = &s[1..];
        while !shorthands.is_empty() {
            // pflag leaves `go test` flags such as -test.v to the go flag package.
            if shorthands.starts_with("test.") {
                break;
            }
            let c = shorthands.as_bytes()[0];
            let Some(idx) = self.index_short(c) else {
                return Err(FlagError::UnknownShort {
                    c,
                    shorthands: shorthands.to_string(),
                });
            };
            let no_opt = self.entries[idx].spec.no_opt_default;
            let after = &shorthands[1..];
            let value = if let Some(v) = after.strip_prefix('=').filter(|_| after.len() > 1) {
                shorthands = "";
                v
            } else if !no_opt.is_empty() {
                shorthands = after;
                no_opt
            } else if !after.is_empty() {
                shorthands = "";
                after
            } else if let Some((v, tail)) = rest.split_first() {
                shorthands = "";
                rest = tail;
                v.as_str()
            } else {
                return Err(FlagError::NeedsArgShort {
                    c,
                    shorthands: shorthands.to_string(),
                });
            };
            self.set(idx, value)?;
        }
        Ok(rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;

    fn flags(path: &str) -> Flags {
        Flags::for_command(registry::find(path).unwrap(), true)
    }

    fn parse(path: &str, args: &[&str]) -> Result<(Flags, Vec<String>), String> {
        let mut f = flags(path);
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        f.parse(&args)
            .map(|pos| (f, pos))
            .map_err(|e| e.to_string())
    }

    #[test]
    fn long_flags_take_a_separate_or_inline_value() {
        let (f, pos) = parse("video list", &["--output", "json", "--hl=fr"]).unwrap();
        assert_eq!(f.string("output"), "json");
        assert_eq!(f.string("hl"), "fr");
        assert!(f.changed("output"));
        assert!(!f.changed("chart"));
        assert!(pos.is_empty());
    }

    #[test]
    fn bool_flags_use_their_no_opt_default() {
        let (f, _) = parse("playlist list", &["--mine"]).unwrap();
        assert!(f.bool("mine"));
        let (f, _) = parse("playlist list", &["--mine=false"]).unwrap();
        assert!(!f.bool("mine"));
        assert!(f.changed("mine"));
        let (f, pos) = parse("playlist list", &["--mine", "false"]).unwrap();
        assert!(f.bool("mine"));
        assert_eq!(pos, ["false"]);
    }

    #[test]
    fn shorthands_take_a_separate_attached_or_inline_value() {
        let (f, _) = parse("video list", &["-i", "a"]).unwrap();
        assert_eq!(f.strings("ids"), ["a"]);
        let (f, _) = parse("video list", &["-ia,b", "-ojson"]).unwrap();
        assert_eq!(f.strings("ids"), ["a", "b"]);
        assert_eq!(f.string("output"), "json");
        let (f, _) = parse("video list", &["-i=a", "-o=json"]).unwrap();
        assert_eq!(f.strings("ids"), ["a"]);
        assert_eq!(f.string("output"), "json");
    }

    #[test]
    fn combined_bool_shorthands_set_each_flag() {
        let (f, _) = parse("playlist list", &["-Mo", "json"]).unwrap();
        assert!(f.bool("mine"));
        assert_eq!(f.string("output"), "json");
        let (f, _) = parse("playlist list", &["-Mojson"]).unwrap();
        assert_eq!(f.string("output"), "json");
    }

    #[test]
    fn double_dash_ends_flags_and_positionals_intersperse() {
        let (f, pos) = parse("video list", &["extra", "-o", "json", "--", "--ids", "x"]).unwrap();
        assert_eq!(f.string("output"), "json");
        assert!(!f.changed("ids"));
        assert_eq!(pos, ["extra", "--ids", "x"]);
    }

    #[test]
    fn string_slices_replace_the_default_then_append() {
        let f = flags("video list");
        assert_eq!(
            f.strings("parts"),
            ["id", "snippet", "status", "statistics"]
        );
        let (f, _) = parse(
            "video list",
            &["--parts", "id", "--parts", "snippet,status"],
        )
        .unwrap();
        assert_eq!(f.strings("parts"), ["id", "snippet", "status"]);
        let (f, _) = parse("video list", &["--ids", r#""a,b",c"#]).unwrap();
        assert_eq!(f.strings("ids"), ["a,b", "c"]);
        let (f, _) = parse("video list", &["--ids", ""]).unwrap();
        assert!(f.strings("ids").is_empty());
        assert!(f.changed("ids"));
    }

    #[test]
    fn numbers_parse_with_go_base_prefixes() {
        let (f, _) = parse("video list", &["-n", "0x10"]).unwrap();
        assert_eq!(f.i64("maxResults"), 16);
        assert_eq!(flags("video list").i64("maxResults"), 5);
        let (f, _) = parse("watermark set", &["--durationMs", "1_000"]).unwrap();
        assert_eq!(f.u64("durationMs"), 1000);
    }

    #[test]
    fn errors_match_pflag() {
        let err = |path: &str, args: &[&str]| parse(path, args).err().unwrap();
        assert_eq!(err("video list", &["--nope"]), "unknown flag: --nope");
        assert_eq!(
            err("video list", &["-z"]),
            "unknown shorthand flag: 'z' in -z"
        );
        assert_eq!(
            err("video list", &["-iz", "-zq"]),
            "unknown shorthand flag: 'z' in -zq"
        );
        assert_eq!(
            err("video list", &["--ids"]),
            "flag needs an argument: --ids"
        );
        assert_eq!(
            err("video list", &["-i"]),
            "flag needs an argument: 'i' in -i"
        );
        assert_eq!(err("video list", &["---x"]), "bad flag syntax: ---x");
        assert_eq!(err("video list", &["--=x"]), "bad flag syntax: --=x");
        assert_eq!(
            err("video list", &["--maxResults", "x"]),
            r#"invalid argument "x" for "-n, --maxResults" flag: strconv.ParseInt: parsing "x": invalid syntax"#
        );
        assert_eq!(
            err("video list", &["-n", "9223372036854775808"]),
            r#"invalid argument "9223372036854775808" for "-n, --maxResults" flag: strconv.ParseInt: parsing "9223372036854775808": value out of range"#
        );
        assert_eq!(
            err("watermark set", &["--durationMs", "-5"]),
            r#"invalid argument "-5" for "-d, --durationMs" flag: strconv.ParseUint: parsing "-5": invalid syntax"#
        );
        assert_eq!(
            err("watermark set", &["-d", "18446744073709551616"]),
            r#"invalid argument "18446744073709551616" for "-d, --durationMs" flag: strconv.ParseUint: parsing "18446744073709551616": value out of range"#
        );
        assert_eq!(
            err("playlist list", &["--mine=maybe"]),
            r#"invalid argument "maybe" for "-M, --mine" flag: strconv.ParseBool: parsing "maybe": invalid syntax"#
        );
        assert_eq!(
            err("video list", &["--ids", "a\"b"]),
            r#"invalid argument "a\"b" for "-i, --ids" flag: parse error on line 1, column 2: bare " in non-quoted-field"#
        );
    }

    #[test]
    fn help_flag_exists_only_when_requested() {
        let (f, _) = parse("video list", &["-h"]).unwrap();
        assert!(f.bool("help"));
        assert_eq!(f.lookup("help").unwrap().spec.usage, "help for list");
        assert!(
            Flags::for_command(registry::find("video list").unwrap(), false)
                .lookup("help")
                .is_none()
        );
    }
}
