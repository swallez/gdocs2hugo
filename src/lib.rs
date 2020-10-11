pub mod config;
mod css_rules;
pub mod download;
mod html;
mod images;
pub mod publish;

#[macro_use]
extern crate serde_derive;
use chrono::Utc;

#[derive(Debug, Deserialize)]
pub struct DocData {
    pub slug: String,
    pub author: Option<String>,
    pub category: Option<String>,
    pub weight: Option<u16>,
    #[serde(deserialize_with = "deser_uppercase_bool")]
    pub publish: bool,
    #[serde(deserialize_with = "deser_csv_date")]
    pub publish_date: DateTimeWithDefault,
    #[serde(deserialize_with = "deser_csv_date_option")]
    pub update_date: Option<DateTimeWithDefault>,
    pub gdoc_pub_url: String,
}

impl DocData {
    pub fn read_csv(reader: impl std::io::Read) -> csv::Result<Vec<DocData>> {
        let mut rdr = csv::ReaderBuilder::new().from_reader(reader);

        // First line after the header is the human-readable column names: skip it
        rdr.deserialize().skip(1).collect()
    }
}

#[derive(Debug, Serialize, Default)]
pub struct FrontMatter {
    pub markup: &'static str,
    pub author: Option<String>,
    pub title: String,
    pub date: DateTimeWithDefault,
    pub lastmod: Option<DateTimeWithDefault>,
    pub banner: Option<String>,
    pub slug: String,
    pub categories: Vec<String>,
    // "weight" should be "categories_weight" but it doesn't seem to work as advertised in Hugo's docs.
    pub weight: Option<u16>,
    pub summary: Option<String>,
    pub inline_style: Option<String>,
    // not used in the publication process, but useful to distinguish generated pages
    pub gdoc_pub_url: String,
}

//pub const GDRIVE_DIR: &str = "data/gdrive";

fn deser_uppercase_bool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "TRUE" => Ok(true),
        "FALSE" => Ok(false),
        _ => Err(serde::de::Error::custom(format!("Expecting TRUE or FALSE, got {}", s))),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DateTimeWithDefault(pub chrono::DateTime<Utc>);
impl Default for DateTimeWithDefault {
    fn default() -> Self {
        DateTimeWithDefault(chrono::MIN_DATETIME)
    }
}

use chrono::TimeZone;
use serde::{self, Deserialize, Deserializer};

pub fn deser_csv_date<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DateTimeWithDefault, D::Error> {
    let s = String::deserialize(deserializer)?;
    Ok(DateTimeWithDefault(
        Utc.datetime_from_str(&s, "%d/%m/%Y %H:%M:%S")
            .map_err(serde::de::Error::custom)?,
    ))
}

pub fn deser_csv_date_option<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DateTimeWithDefault>, D::Error> {
    let s = String::deserialize(deserializer)?;
    if s.is_empty() {
        Ok(None)
    } else {
        let t = Utc
            .datetime_from_str(&s, "%d/%m/%Y %H:%M:%S")
            .map_err(serde::de::Error::custom)?;
        Ok(Some(DateTimeWithDefault(t)))
    }
}
