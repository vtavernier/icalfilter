use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::iter::FromIterator;
use std::path::PathBuf;

use anyhow::Result;
use argh::FromArgs;
use chrono::prelude::*;
use itertools::Itertools;
use thiserror::Error;

#[derive(Debug, Default)]
struct MatchTree {
    groups: HashMap<isize, MatchGroup>,
}

impl MatchTree {
    fn is_match(&self, event: &ical::parser::ical::component::IcalEvent) -> Option<isize> {
        self.groups
            .iter()
            .find(|(_id, group)| group.is_match(event))
            .map(|(id, _group)| *id)
    }
}

#[derive(Debug, Default)]
struct MatchGroup {
    properties: HashMap<String, regex::Regex>,
}

impl MatchGroup {
    fn add_rule(&mut self, rule: MatchRule) {
        self.properties.insert(rule.property, rule.value);
    }

    fn is_match(&self, event: &ical::parser::ical::component::IcalEvent) -> bool {
        // TODO: do this in not O(N^2)
        self.properties.iter().all(|(prop, val)| {
            event
                .properties
                .iter()
                .find(|ep| ep.name == *prop)
                .map(|ep| {
                    ep.value
                        .as_ref()
                        .map(|ev| val.is_match(ev))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        })
    }
}

impl From<Vec<MatchRule>> for MatchTree {
    fn from(rules: Vec<MatchRule>) -> Self {
        let mut groups: HashMap<isize, MatchGroup> = HashMap::new();

        for rule in rules {
            let group_id = rule.group_id;
            if let Some(group) = groups.get_mut(&group_id) {
                group.add_rule(rule);
            } else {
                let mut group = MatchGroup::default();
                group.add_rule(rule);
                groups.insert(group_id, group);
            }
        }

        Self { groups }
    }
}

/// A rule for matching a property event
#[derive(Debug)]
struct MatchRule {
    group_id: isize,
    property: String,
    value: regex::Regex,
}

#[derive(Debug, Error)]
enum MatchRuleParseError {
    #[error("missing equal sign in rule")]
    MissingEqualSign,
    #[error("failed to parse regex: {0}")]
    RegexParseError(#[source] regex::Error),
    #[error("failed to parse group id: {0}")]
    GroupIdParseError(#[source] std::num::ParseIntError),
}

impl std::str::FromStr for MatchRule {
    type Err = MatchRuleParseError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let group_split: Vec<_> = s.splitn(2, ',').collect();
        let split: Vec<_> = group_split.last().unwrap().splitn(2, '=').collect();

        if split.len() != 2 {
            return Err(Self::Err::MissingEqualSign);
        }

        Ok(Self {
            group_id: if group_split.len() == 2 {
                group_split[0]
                    .parse()
                    .map_err(Self::Err::GroupIdParseError)?
            } else {
                0
            },
            property: split[0].to_owned(),
            value: regex::Regex::new(split[1]).map_err(Self::Err::RegexParseError)?,
        })
    }
}

/// A rule for deleting events in a time frame
#[derive(Debug)]
struct RemoveMatchRule {
    group_id: isize,
    single_day: bool,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
}

impl RemoveMatchRule {
    fn is_match(&self, event: &ical::parser::ical::component::IcalEvent) -> bool {
        let date = NaiveDateTime::parse_from_str(
            event
                .properties
                .iter()
                .find(|p| p.name == "DTSTART")
                .expect("could not find DTSTART property")
                .value
                .as_deref()
                .expect("missing value for DTSTART"),
            "%Y%m%dT%H%M%SZ",
        )
        .expect("failed to parse DTSTART as date")
        .date();

        if self.single_day {
            self.start_date.map(|sd| sd == date).unwrap_or(false)
        } else {
            match (self.start_date, self.end_date) {
                (Some(sd), Some(ed)) => sd <= date && date <= ed,
                (None, Some(ed)) => date <= ed,
                (Some(sd), None) => sd <= date,
                (None, None) => true,
            }
        }
    }
}

#[derive(Debug, Error)]
enum RemoveRuleParseError {
    #[error("failed to parse group id: {0}")]
    GroupIdParseError(#[source] std::num::ParseIntError),
    #[error("failed to parse date: {0}")]
    DateParseError(#[source] chrono::format::ParseError),
}

impl std::str::FromStr for RemoveMatchRule {
    type Err = RemoveRuleParseError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let split: Vec<_> = s.splitn(3, ',').collect();
        let group_id = split[0].parse().map_err(Self::Err::GroupIdParseError)?;

        match split.len() {
            1 => Ok(Self {
                group_id,
                single_day: false,
                start_date: None,
                end_date: None,
            }),
            2 => Ok(Self {
                group_id,
                single_day: true,
                start_date: Some(
                    NaiveDate::parse_from_str(split[1], "%Y-%m-%d")
                        .map_err(Self::Err::DateParseError)?,
                ),
                end_date: None,
            }),
            3 => Ok(Self {
                group_id,
                single_day: false,
                start_date: if split[1].is_empty() {
                    None
                } else {
                    Some(
                        NaiveDate::parse_from_str(split[1], "%Y-%m-%d")
                            .map_err(Self::Err::DateParseError)?,
                    )
                },
                end_date: if split[2].is_empty() {
                    None
                } else {
                    Some(
                        NaiveDate::parse_from_str(split[2], "%Y-%m-%d")
                            .map_err(Self::Err::DateParseError)?,
                    )
                },
            }),
            _ => unreachable!("splitn with n = 3"),
        }
    }
}

#[derive(Debug, Default)]
struct RemoveTree {
    rules: HashMap<isize, Vec<RemoveMatchRule>>,
}

impl RemoveTree {
    fn is_match(
        &self,
        group: isize,
        event: &ical::parser::ical::component::IcalEvent,
    ) -> Option<bool> {
        self.rules
            .get(&group)
            .map(|rules| rules.iter().any(|rule| rule.is_match(event)))
    }
}

impl From<Vec<RemoveMatchRule>> for RemoveTree {
    fn from(mut rules: Vec<RemoveMatchRule>) -> Self {
        rules.sort_unstable_by_key(|rule| rule.group_id);

        Self {
            rules: HashMap::from_iter(
                rules
                    .into_iter()
                    .group_by(|rule| rule.group_id)
                    .into_iter()
                    .map(|(group_id, rules)| (group_id, rules.collect::<Vec<_>>())),
            ),
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
}

fn main_io(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    match_tree: MatchTree,
    remove_tree: RemoveTree,
) -> Result<()> {
    let reader = ical::IcalParser::new(input);

    for calendar in reader {
        let mut calendar = calendar?;

        // Filter events
        calendar
            .events
            .retain(|event| match match_tree.is_match(&event) {
                Some(id) => {
                    remove_tree.is_match(id, &event) != Some(true)
                        && remove_tree.is_match(-1, &event) != Some(true)
                }
                None => false,
            });

        // Format calendar to output
        write!(output, "{}", calendar)?;
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
        ),
        (None, Some(output_path)) => main_io(
            &mut std::io::stdin().lock(),
            &mut File::create(output_path)?,
            match_tree,
            remove_tree,
        ),
        (Some(input_path), None) => main_io(
            &mut BufReader::new(File::open(input_path)?),
            &mut std::io::stdout(),
            match_tree,
            remove_tree,
        ),
        (None, None) => main_io(
            &mut std::io::stdin().lock(),
            &mut std::io::stdout(),
            match_tree,
            remove_tree,
        ),
    }
}
