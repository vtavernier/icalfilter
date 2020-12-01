use std::collections::HashMap;
use std::iter::FromIterator;

use chrono::prelude::*;
use itertools::Itertools;
use thiserror::Error;

use super::Event;

#[derive(Debug, Default)]
pub struct MatchTree {
    groups: HashMap<isize, MatchGroup>,
}

impl MatchTree {
    pub fn is_match(&self, event: &Event) -> Option<isize> {
        self.groups
            .iter()
            .find(|(_id, group)| group.is_match(event))
            .map(|(id, _group)| *id)
    }
}

#[derive(Debug, Default)]
pub struct MatchGroup {
    properties: HashMap<String, regex::Regex>,
}

impl MatchGroup {
    fn add_rule(&mut self, rule: MatchRule) {
        self.properties.insert(rule.property, rule.value);
    }

    fn is_match(&self, event: &Event) -> bool {
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
pub struct MatchRule {
    group_id: isize,
    property: String,
    value: regex::Regex,
}

#[derive(Debug, Error)]
pub enum MatchRuleParseError {
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
pub struct RemoveMatchRule {
    group_id: isize,
    single_day: bool,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
}

impl RemoveMatchRule {
    pub fn is_match(&self, event: &Event) -> bool {
        let date = event
            .dtstart()
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
pub enum RemoveRuleParseError {
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
pub struct RemoveTree {
    rules: HashMap<isize, Vec<RemoveMatchRule>>,
}

impl RemoveTree {
    pub fn is_match(&self, group: isize, event: &Event) -> Option<bool> {
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
