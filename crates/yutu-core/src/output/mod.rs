//! The output switch every op shares, as pkg/common PrintList and
//! PrintResult do it in Go.

pub mod table;
mod widths;

use std::io::Write;
pub use table::Cell;
use yutu_api::{Value, json, yaml};

/// Where an op writes. `interactive` mirrors Go's `utils.IsInteractive`:
/// a terminal on stdout with `CI` unset. MCP output is never interactive.
pub struct Out<'a> {
    pub w: &'a mut (dyn Write + Send),
    pub interactive: bool,
}

impl Out<'_> {
    pub fn write_str(&mut self, s: &str) -> Result<(), String> {
        self.w.write_all(s.as_bytes()).map_err(|e| e.to_string())
    }
}

/// `PrintList`: json, yaml or table; any other value prints nothing.
pub fn print_list(
    out: &mut Out<'_>,
    output: &str,
    items: &[Value],
    header: &[&str],
    row: impl Fn(&Value) -> Vec<Cell>,
) -> Result<(), String> {
    match output {
        "json" => out.write_str(&format!("{}\n", json::list(items, out.interactive))),
        "yaml" => out.write_str(&format!("{}\n", yaml::list(items))),
        "table" => {
            let rows: Vec<Vec<Cell>> = items.iter().map(&row).collect();
            out.write_str(&table::render(header, &rows))
        }
        _ => Ok(()),
    }
}

/// `PrintResult`: json, yaml, silent, or the human line otherwise.
pub fn print_result(
    out: &mut Out<'_>,
    output: &str,
    value: &Value,
    human: &str,
) -> Result<(), String> {
    match output {
        "json" => out.write_str(&format!("{}\n", json::value(value, out.interactive))),
        "yaml" => out.write_str(&format!("{}\n", yaml::value(value))),
        "silent" => Ok(()),
        _ => out.write_str(human),
    }
}
