//! Tables of one snapshot: `list` and `info` (live), `show` (stored or live).

use anyhow::{Result, bail};
use serde_json::{Map, Value};

use super::{Ctx, note_restricted};
use crate::analysis::reports::system_header;
use crate::analysis::view::{self, View, lifetime_cpu, view_table};
use crate::cli::{FilterArgs, ViewArgs};
use crate::output::{out, print_json};

pub fn list(ctx: &Ctx, view: &ViewArgs, interval: &Option<String>) -> Result<()> {
    // After an option clap no longer looks for subcommands, so `psm procs
    // --top 3 list` would search for the word "list". Say so instead.
    if let Some(sub) = view.words.iter().find(|w| *w == "list" || *w == "show") {
        bail!(
            "`{sub}` is a subcommand: put it right after `procs` (`psm procs {sub} ...`); to search for the word use --name {sub}"
        );
    }
    let deep = view.deep.unwrap_or(ctx.cfg.collection.deep);
    let (snapshot, cpu, restricted) = ctx.live(deep, interval)?;
    ctx.emit(
        &view_table(&snapshot, &cpu, &ctx.view(view, deep)?)?,
        view.csv,
    );
    note_restricted(restricted, &snapshot);
    Ok(())
}

pub fn info(
    ctx: &Ctx,
    n: usize,
    by: &[String],
    group: &Option<String>,
    interval: &Option<String>,
    deep: Option<bool>,
    filter: &FilterArgs,
) -> Result<()> {
    let deep = deep.unwrap_or(ctx.cfg.collection.deep);
    let (snapshot, cpu, _) = ctx.live(deep, interval)?;
    let group = ctx.group(group);
    let (header, system) = system_header(&snapshot, &ctx.filter(filter)?);
    let mut text = vec![header];
    let mut doc = Map::from_iter([("system".to_string(), system)]);
    for metric in by {
        // Every table is exactly `psm procs --sort <metric> --top <n>` over the same pass.
        let view = View {
            group: group.clone(),
            sort: metric.clone(),
            top: Some(n),
            deep,
            filter: ctx.filter(filter)?,
        };
        let table = view_table(&snapshot, &cpu, &view)?;
        let title = if metric == "mem" {
            "MEMORY".to_string()
        } else {
            metric.to_uppercase()
        };
        text.push(format!("TOP {n} BY {title}\n{}", table.render()));
        doc.insert(metric.clone(), table.json());
    }
    if ctx.json {
        print_json(&Value::Object(doc))
    } else {
        out(text.join("\n\n"))
    }
    Ok(())
}

pub fn show(ctx: &Ctx, snapshot: &str, view: &ViewArgs) -> Result<()> {
    let db = ctx.db()?;
    let session = db.session(None)?;
    let s = match snapshot {
        "now" => super::capture::live_snapshot(ctx, None)?,
        stored => db.load(db.resolve(&session, stored)?)?,
    };
    let deep = view.deep.unwrap_or(s.deep);
    // A stored snapshot holds one reading, so %CPU is the lifetime average.
    let table = view::view_table(&s, &lifetime_cpu(&s), &ctx.view(view, deep)?)?;
    ctx.emit(&table, view.csv);
    Ok(())
}
