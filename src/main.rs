use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use argh::FromArgs;
use chrono::Duration;
use itertools::Itertools;

mod event;
use event::Event;

mod rules;
use rules::{MatchRule, MatchTree, RemoveMatchRule, RemoveTree};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Format {
    ICS,
    CSV,
}

impl Default for Format {
    fn default() -> Self {
        Self::ICS
    }
}

impl std::str::FromStr for Format {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ics" => Ok(Self::ICS),
            "csv" => Ok(Self::CSV),
            _ => Err(anyhow!("invalid format")),
        }
    }
}

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

    #[argh(option, short = 'f', default = "Default::default()")]
    /// output format
    format: Format,
}

fn main_io(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    match_tree: MatchTree,
    remove_tree: RemoveTree,
    show_stats: bool,
    format: Format,
) -> Result<()> {
    let reader = ical::IcalParser::new(input);
    let mut stats: HashMap<isize, chrono::Duration> = HashMap::new();

    let mut writer = match format {
        Format::ICS => Box::new(
            move |calendar: &ical::parser::ical::component::IcalCalendar| {
                write!(output, "{}", calendar)?;
                Ok(())
            },
        )
            as Box<dyn FnMut(&ical::parser::ical::component::IcalCalendar) -> Result<()>>,
        Format::CSV => {
            let mut csv = csv::Writer::from_writer(output);

            Box::new(
                move |calendar: &ical::parser::ical::component::IcalCalendar| {
                    for event in &calendar.events {
                        let evt = Event::from(event);

                        csv.write_record(&[
                            &evt.dtstart()
                                .map(|dt| dt.to_string())
                                .unwrap_or_else(|_| evt.prop("DTSTART").unwrap().to_string()),
                            &evt.dtend()
                                .map(|dt| dt.to_string())
                                .unwrap_or_else(|_| evt.prop("DTEND").unwrap().to_string()),
                            evt.summary().unwrap(),
                            evt.description().unwrap_or_else(|| ""),
                            &evt.duration()
                                .map(|d| d.num_minutes().to_string())
                                .unwrap_or_else(|_| String::new()),
                        ])?;
                    }
                    Ok(())
                },
            )
                as Box<dyn FnMut(&ical::parser::ical::component::IcalCalendar) -> Result<()>>
        }
    };

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
        writer(&calendar)?;
    }

    if show_stats {
        // Print group stats
        for (k, v) in stats.iter().sorted_by_key(|(k, _v)| *k) {
            eprintln!(
                "duration for group {}: {} hours",
                if *k == isize::MIN {
                    "default".to_owned()
                } else {
                    k.to_string()
                },
                v.num_hours()
            );
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
            opts.format,
        ),
        (None, Some(output_path)) => main_io(
            &mut std::io::stdin().lock(),
            &mut File::create(output_path)?,
            match_tree,
            remove_tree,
            opts.show_stats,
            opts.format,
        ),
        (Some(input_path), None) => main_io(
            &mut BufReader::new(File::open(input_path)?),
            &mut std::io::stdout(),
            match_tree,
            remove_tree,
            opts.show_stats,
            opts.format,
        ),
        (None, None) => main_io(
            &mut std::io::stdin().lock(),
            &mut std::io::stdout(),
            match_tree,
            remove_tree,
            opts.show_stats,
            opts.format,
        ),
    }
}
