use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::Result;
use argh::FromArgs;
use thiserror::Error;

#[derive(Debug, Default)]
struct MatchTree {
    groups: HashMap<usize, MatchGroup>,
}

impl MatchTree {
    fn is_match(&self, event: &ical::parser::ical::component::IcalEvent) -> bool {
        self.groups.iter().any(|(_id, group)| group.is_match(event))
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
        let mut groups: HashMap<usize, MatchGroup> = HashMap::new();

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
    group_id: usize,
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

impl MatchRule {
    pub fn matches(&self, property: &ical::property::Property) -> bool {
        property.name == self.property
            && property
                .value
                .as_ref()
                .map(|val| self.value.is_match(val))
                .unwrap_or(false)
    }
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
}

fn main_io(input: &mut dyn BufRead, output: &mut dyn Write, match_tree: MatchTree) -> Result<()> {
    let reader = ical::IcalParser::new(input);

    for calendar in reader {
        let mut calendar = calendar?;

        // Filter events
        calendar.events.retain(|event| match_tree.is_match(&event));

        // Format calendar to output
        write!(output, "{}", calendar)?;
    }

    Ok(())
}

fn main() -> Result<()> {
    let opts: Opts = argh::from_env();
    let match_tree = opts.include.into();

    match (opts.input.as_ref(), opts.output.as_ref()) {
        (Some(input_path), Some(output_path)) => main_io(
            &mut BufReader::new(File::open(input_path)?),
            &mut File::create(output_path)?,
            match_tree,
        ),
        (None, Some(output_path)) => main_io(
            &mut std::io::stdin().lock(),
            &mut File::create(output_path)?,
            match_tree,
        ),
        (Some(input_path), None) => main_io(
            &mut BufReader::new(File::open(input_path)?),
            &mut std::io::stdout(),
            match_tree,
        ),
        (None, None) => main_io(
            &mut std::io::stdin().lock(),
            &mut std::io::stdout(),
            match_tree,
        ),
    }
}
