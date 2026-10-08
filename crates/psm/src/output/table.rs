//! The one table type every command renders as text, JSON or CSV.

use serde_json::{Map, Value, json};

use super::units::{human, human_delta};

#[derive(Debug, Clone)]
pub enum Cell {
    Text(String),
    Int(Option<i64>),
    IntDelta(i64),
    /// Missing is printed empty (no second CPU reading), not as n/a.
    Pct(Option<f64>),
    Bytes(Option<i64>),
    Delta(Option<i64>),
    /// Not applicable, e.g. BEFORE of a new process.
    Dash,
}

impl Cell {
    pub fn text(s: impl Into<String>) -> Cell {
        Cell::Text(s.into())
    }

    fn render(&self) -> String {
        match self {
            Cell::Text(s) => s.clone(),
            Cell::Int(Some(n)) => n.to_string(),
            Cell::IntDelta(n) if *n > 0 => format!("+{n}"),
            Cell::IntDelta(n) => n.to_string(),
            Cell::Pct(Some(x)) => format!("{x:.1}"),
            Cell::Pct(None) => String::new(),
            Cell::Bytes(Some(b)) => human(*b),
            Cell::Delta(Some(b)) => human_delta(*b),
            Cell::Int(None) | Cell::Bytes(None) | Cell::Delta(None) => "n/a".into(),
            Cell::Dash => "-".into(),
        }
    }

    /// Raw value: bytes stay bytes, so scripts never parse "1.91 GiB".
    fn json(&self) -> Value {
        match self {
            Cell::Text(s) => json!(s),
            Cell::Int(v) | Cell::Bytes(v) | Cell::Delta(v) => json!(v),
            Cell::IntDelta(n) => json!(n),
            Cell::Pct(v) => json!(v.map(|x| (x * 10.0).round() / 10.0)),
            Cell::Dash => Value::Null,
        }
    }
}

pub struct Table {
    /// `(title, json key)`
    pub cols: Vec<(&'static str, &'static str)>,
    pub rows: Vec<Vec<Cell>>,
}

impl Table {
    pub fn new(cols: &[(&'static str, &'static str)]) -> Table {
        Table {
            cols: cols.to_vec(),
            rows: Vec::new(),
        }
    }

    pub fn truncate(&mut self, top: Option<usize>) {
        if let Some(n) = top {
            self.rows.truncate(n);
        }
    }

    /// Text columns are left-aligned, numeric ones right-aligned.
    pub fn render(&self) -> String {
        if self.rows.is_empty() {
            return "(none)".into();
        }
        let cells: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|r| r.iter().map(Cell::render).collect())
            .collect();
        let titles: Vec<String> = self.cols.iter().map(|c| c.0.to_string()).collect();
        // Per column: (left-aligned?, width).
        let layout: Vec<(bool, usize)> = (0..titles.len())
            .map(|i| {
                let left = self.rows.iter().all(|r| matches!(r[i], Cell::Text(_)));
                let width = cells
                    .iter()
                    .map(|r| &r[i])
                    .chain([&titles[i]])
                    .map(|c| c.chars().count());
                (left, width.max().unwrap_or(0))
            })
            .collect();
        let mut lines = Vec::with_capacity(cells.len() + 1);
        for row in std::iter::once(&titles).chain(cells.iter()) {
            let mut line = String::new();
            for (cell, (left, width)) in row.iter().zip(&layout) {
                if !line.is_empty() {
                    line.push_str("  ");
                }
                if *left {
                    line.push_str(&format!("{cell:<width$}"));
                } else {
                    line.push_str(&format!("{cell:>width$}"));
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    pub fn json(&self) -> Value {
        self.rows
            .iter()
            .map(|r| {
                let obj: Map<String, Value> = self
                    .cols
                    .iter()
                    .zip(r)
                    .map(|(c, cell)| (c.1.to_string(), cell.json()))
                    .collect();
                Value::Object(obj)
            })
            .collect()
    }

    pub fn csv(&self) -> String {
        let header = self
            .cols
            .iter()
            .map(|c| c.1.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let rows = self.rows.iter().map(|r| {
            r.iter()
                .map(|cell| match cell.json() {
                    Value::Null => String::new(),
                    Value::String(s) => csv_field(&s),
                    v => v.to_string(),
                })
                .collect::<Vec<_>>()
                .join(",")
        });
        std::iter::once(header)
            .chain(rows)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_quotes_only_when_needed() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b \"c\""), "\"a,b \"\"c\"\"\"");
    }
}
