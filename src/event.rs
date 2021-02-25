use std::fmt;

use anyhow::{anyhow, Result};
use chrono::prelude::*;

const DATE_TIME_FORMAT: &str = "%Y%m%dT%H%M%SZ";

/// Represents an Ical event with parsed information
#[derive(Debug)]
pub struct Event<'e> {
    ical: &'e ical::parser::ical::component::IcalEvent,
    dtstart: Result<NaiveDateTime>,
    dtend: Result<NaiveDateTime>,
}

impl Event<'_> {
    pub fn dtstart(&self) -> std::result::Result<&NaiveDateTime, &anyhow::Error> {
        self.dtstart.as_ref()
    }

    pub fn dtend(&self) -> std::result::Result<&NaiveDateTime, &anyhow::Error> {
        self.dtend.as_ref()
    }

    pub fn duration(&self) -> Result<chrono::Duration> {
        Ok(*self
            .dtend
            .as_ref()
            .map_err(|_| anyhow!("missing dtend for duration"))?
            - *self
                .dtstart
                .as_ref()
                .map_err(|_| anyhow!("missing dtstart for duration"))?)
    }

    pub fn prop(&self, name: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|p| p.name == name)
            .and_then(|prop| prop.value.as_deref())
    }

    pub fn description(&self) -> Option<&str> {
        self.prop("DESCRIPTION")
    }

    pub fn summary(&self) -> Option<&str> {
        self.prop("SUMMARY")
    }
}

impl std::ops::Deref for Event<'_> {
    type Target = ical::parser::ical::component::IcalEvent;

    fn deref(&self) -> &Self::Target {
        self.ical
    }
}

impl fmt::Display for Event<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ical)
    }
}

impl<'e> From<&'e ical::parser::ical::component::IcalEvent> for Event<'e> {
    fn from(ical: &'e ical::parser::ical::component::IcalEvent) -> Self {
        Self {
            ical,
            dtstart: ical
                .properties
                .iter()
                .find(|p| p.name == "DTSTART")
                .ok_or_else(|| anyhow!("could not find DTSTART property"))
                .and_then(|p| {
                    p.value
                        .as_deref()
                        .ok_or_else(|| anyhow!("DTSTART property is empty"))
                })
                .and_then(|s| Ok(NaiveDateTime::parse_from_str(s, DATE_TIME_FORMAT)?)),
            dtend: ical
                .properties
                .iter()
                .find(|p| p.name == "DTEND")
                .ok_or_else(|| anyhow!("could not find DTEND property"))
                .and_then(|p| {
                    p.value
                        .as_deref()
                        .ok_or_else(|| anyhow!("DTEND property is empty"))
                })
                .and_then(|s| Ok(NaiveDateTime::parse_from_str(s, DATE_TIME_FORMAT)?)),
        }
    }
}
