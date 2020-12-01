use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::Result;
use argh::FromArgs;
use chrono::Duration;
use itertools::Itertools;

mod event;
use event::Event;

mod rules;
use rules::{MatchRule, MatchTree, RemoveMatchRule, RemoveTree};

#[derive(FromArgs)]
/// Filter events from an ICAL file
struct Opts {
    #[argh(option, short = 'i')]
    /// input file, defaults to stdin
    input: Option<PathBuf>,

    #[argh(option, short = 'o')]
    /// output file, defaults to stdout
    output: Option<PathBuf>,

    #[argh(option, short = 'I')]
    /// include events matching this rule
    include: Vec<MatchRule>,

    #[argh(option, short = 'R')]
    /// remove events from a group in those dates
    remove: Vec<RemoveMatchRule>,

    #[argh(switch, short = 's')]
    /// show duration statistics on this calendar
    show_stats: bool,
}

fn main_io(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    match_tree: MatchTree,
    remove_tree: RemoveTree,
    show_stats: bool,
) -> Result<()> {
    let reader = ical::IcalParser::new(input);
    let mut stats: HashMap<isize, chrono::Duration> = HashMap::new();

    for calendar in reader {
        let mut calendar = calendar?;

        // Filter events
        calendar.events.retain(|event| {
            let event: Event = event.into();

            match match_tree.is_match(&event) {
                Some(id) => {
                    let result = remove_tree.is_match(id, &event) != Some(true)
                        && remove_tree.is_match(-1, &event) != Some(true);

                    if result {
                        match event.duration() {
                            Ok(d) => {
                                let d = stats.get(&id).cloned().unwrap_or_else(Duration::zero) + d;
                                stats.insert(id, d);
                            }
                            Err(e) => {
                                if show_stats {
                                    eprintln!("error computing duration for {:?}: {}", event, e);
                                }
                            }
                        }
                    }

                    result
                }
                None => false,
            }
        });

        // Format calendar to output
        write!(output, "{}", calendar)?;
    }

    if show_stats {
        // Print group stats
        for (k, v) in stats.iter().sorted_by_key(|(k, _v)| *k) {
            eprintln!("duration for group {}: {} hours", k, v.num_hours());
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let opts: Opts = argh::from_env();
    let match_tree = opts.include.into();
    let remove_tree = opts.remove.into();

    match (opts.input.as_ref(), opts.output.as_ref()) {
        (Some(input_path), Some(output_path)) => main_io(
            &mut BufReader::new(File::open(input_path)?),
            &mut File::create(output_path)?,
            match_tree,
            remove_tree,
            opts.show_stats,
        ),
        (None, Some(output_path)) => main_io(
            &mut std::io::stdin().lock(),
            &mut File::create(output_path)?,
            match_tree,
            remove_tree,
            opts.show_stats,
        ),
        (Some(input_path), None) => main_io(
            &mut BufReader::new(File::open(input_path)?),
            &mut std::io::stdout(),
            match_tree,
            remove_tree,
            opts.show_stats,
        ),
        (None, None) => main_io(
            &mut std::io::stdin().lock(),
            &mut std::io::stdout(),
            match_tree,
            remove_tree,
            opts.show_stats,
        ),
    }
}
