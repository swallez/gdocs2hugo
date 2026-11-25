//! Representation of the Google Docs that constitute the website.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use rayon::prelude::*;
use std::fs;
use serde::{Deserialize, Deserializer, Serialize};
use chrono::{NaiveDateTime, Utc};
use itertools::Itertools;
use lazy_regex::regex;

pub fn download_toc(toc_url: &str, download_dir: &Path) -> anyhow::Result<Vec<DocData>> {
    fs::create_dir_all(download_dir).with_context(|| format!("Cannot create directory {:?}", download_dir))?;

    println!("Downloading ToC from {}", toc_url);
    let csvtext = reqwest::blocking::get(toc_url)?
        .text()
        .context("Failed to download ToC spreadsheet")?;

    let toc_path = download_dir.join("pages.csv");
    fs::write(&toc_path, &csvtext).with_context(|| format!("Failed to write ToC spreadsheet {:?}", toc_path))?;

    let docs = DocData::read_csv(csvtext.as_bytes())?;

    Ok(docs)
}

pub fn download_html_docs(docs: &Vec<DocData>, download_dir: &Path, all: bool) -> anyhow::Result<()> {
    let rt = tokio::runtime::Handle::try_current();
    docs.par_iter()
        .map(|doc| {
            let _guard = rt.as_ref().map(|rt| rt.enter());
            download_html_doc(doc, download_dir, all)
        })
        .collect::<anyhow::Result<Vec<()>>>()?; // Report any error downloading docs
    Ok(())
}

fn download_html_doc(doc: &DocData, download_dir: &Path, all: bool) -> anyhow::Result<()> {
    if !doc.publish && !all {
        //println!("Skipping '{}' (not published)", doc.slug);
        return Ok(());
    }

    println!("Downloading doc for '{}'", doc.slug);

    let content = reqwest::blocking::get(format!("{}?embedded=true", doc.gdoc_pub_url.as_ref().unwrap()))
        .with_context(|| format!("Failed to download {}", doc.slug))?
        .text()?;

    let doc_path = download_dir.join(&doc.download_path);
    fs::write(doc_path, content)?;

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct DocData {
    pub title: String,
    /// Target path. Has a leading '/' but no trailing '/'
    pub slug: String,
    pub author: Option<String>,
    pub category: Option<String>,
    pub weight: Option<i16>,
    #[serde(deserialize_with = "deser_uppercase_bool")]
    pub publish: bool,
    #[serde(deserialize_with = "deser_csv_date_option", default)]
    pub publish_date: Option<DateTimeWithDefault>,
    #[serde(deserialize_with = "deser_csv_date_option", default)]
    pub update_date: Option<DateTimeWithDefault>,
    /// "Publish to web" URL, used to get the HTML rendering of the doc.
    pub gdoc_pub_url: Option<String>,
    /// URL of the doc, used to translate links.
    pub gdoc_url: Option<String>,
    /// Relative path of the downloaded html
    #[serde(skip, default)]
    pub download_path: PathBuf,
    /// Arbitrary data that will be forwarded to the frontmatter
    // #[serde(rename="type")]
    // pub page_type: Option<String>,
    // pub layout: Option<String>,
    /// Values starting with '{' or '[' are parsed as YAML, others are strings.
    #[serde(flatten, deserialize_with = "deser_other")]
    pub other: BTreeMap<String, serde_yaml::Value>
}

impl DocData {
    pub fn read_csv(reader: impl std::io::Read) -> csv::Result<Vec<DocData>> {
        let mut rdr = csv::ReaderBuilder::new().from_reader(reader);

        rdr.deserialize()
            // First line after the header is the human-readable column names: skip it
            .skip(1)
            .map_ok(|mut doc: DocData| {

                // Cleanup gdoc URLs that may contain a fragment
                if let Some(gdoc_url) = doc.gdoc_url.as_mut() {
                    if let Some(frag_pos) = gdoc_url.find('#') {
                        gdoc_url.truncate(frag_pos);
                    }
                }

                // Normalize slugs (actually URL paths) so they have a leading '/' and no trailing '/'
                if doc.slug.ends_with('/') {
                    doc.slug.truncate(doc.slug.len() - 1)
                }
                if !doc.slug.starts_with('/') {
                    doc.slug.insert(0, '/');
                }

                // Compute the download path from the slug/url
                let flat_slug = if doc.slug.len() == 1 {
                    "_index".to_string()
                } else {
                    doc.slug[1..].replace('/', "_")
                };
                doc.download_path = format!("{}.html", flat_slug).into();

                // Remove empty "other" entries
                doc.other.retain(|_, v| !v.is_null());

                // Done
                doc
            })
            .collect()
    }
}

/// A raw CSV cell. Inside a `#[serde(flatten)]` field, the csv crate guesses the type of values
/// instead of always giving strings, so numbers and booleans have to be accepted and turned back
/// into text.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawCell {
    Str(String),
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
}

impl RawCell {
    fn into_string(self) -> String {
        match self {
            RawCell::Str(s) => s,
            RawCell::Bool(b) => b.to_string(),
            RawCell::Int(i) => i.to_string(),
            RawCell::UInt(i) => i.to_string(),
            RawCell::Float(f) => f.to_string(),
        }
    }
}

/// Deserializes the "other" columns. Values starting with '{' or '[' are parsed as YAML,
/// any other value is kept as a string.
fn deser_other<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, serde_yaml::Value>, D::Error> {
    let raw = BTreeMap::<String, RawCell>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(k, v)| {
            let v = v.into_string();
            let value: serde_yaml::Value = match v {
                v if v.starts_with('{') || v.starts_with('[') => {
                    serde_yaml::from_str(&v)
                        .map_err(|e| serde::de::Error::custom(format!("Invalid YAML in column '{}': {}", k, e)))?
                },
                v if v == "TRUE" => serde_yaml::Value::Bool(true),
                v if v == "FALSE" => serde_yaml::Value::Bool(false),
                v if v.is_empty() => serde_yaml::Value::Null,
                v => serde_yaml::Value::String(v)
            };
            Ok((k, value))
        })
        .collect()
}

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
        DateTimeWithDefault(chrono::DateTime::<Utc>::MIN_UTC)
    }
}

fn deser_csv_date_option<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DateTimeWithDefault>, D::Error> {
    static FORMAT: &str = "%d/%m/%Y %H:%M:%S";

    let s = String::deserialize(deserializer)?;
    if s.is_empty() {
        Ok(None)
    } else {
        let t = NaiveDateTime::parse_from_str(&s, FORMAT)
            .with_context(|| format!("Failed to parse date '{}' with format {}", s, FORMAT))
            .map_err(serde::de::Error::custom)?
            .and_local_timezone(Utc)
            .unwrap();
        Ok(Some(DateTimeWithDefault(t)))
    }
}

/// Extracts the document id, if any, from a GDocs URL
pub fn get_doc_id(url: &str) -> Option<&str> {
    regex!("^https://docs.google.com/(document|spreadsheets)(/u/[0-9]+)?/d/([^/]+)/")
        .captures(url)
        .and_then(|captures| captures.get(3))
        .map(|m| m.as_str())
}

#[cfg(test)]
mod test {

    use serde_yaml::Value;

    /// Reads a single-row CSV (after the human-readable header line) and returns its "other" map.
    fn read_other(columns: &[(&str, &str)]) -> anyhow::Result<std::collections::BTreeMap<String, Value>> {
        fn quote(s: &str) -> String {
            format!("\"{}\"", s.replace('"', "\"\""))
        }
        let header = format!("title,slug,publish,{}", columns.iter().map(|c| c.0).collect::<Vec<_>>().join(","));
        let row = format!("A,/a,TRUE,{}", columns.iter().map(|c| quote(c.1)).collect::<Vec<_>>().join(","));
        let csv = format!("{}\nT,S,P\n{}\n", header, row);
        let mut docs = super::DocData::read_csv(csv.as_bytes())?;
        Ok(docs.remove(0).other)
    }

    fn yaml(s: &str) -> Value {
        serde_yaml::from_str(s).unwrap()
    }

    #[test]
    fn test_other_plain_string() {
        let other = read_other(&[("layout", "wide"), ("other", "some text"), ("num", "42"), ("neg", "-7"), ("float", "1.5"), ("lower", "true")]).unwrap();
        assert_eq!(other["layout"], Value::String("wide".into()));
        assert_eq!(other["other"], Value::String("some text".into()));
        // Numbers and booleans guessed by the csv crate are kept as text
        assert_eq!(other["num"], Value::String("42".into()));
        assert_eq!(other["neg"], Value::String("-7".into()));
        assert_eq!(other["float"], Value::String("1.5".into()));
        assert_eq!(other["lower"], Value::String("true".into()));
    }

    #[test]
    fn test_other_yaml_list_and_map() {
        let other = read_other(&[("tags", "[x, y]"), ("layout", "{a: 1, b: [c, d]}")]).unwrap();
        assert_eq!(other["tags"], yaml("[x, y]"));
        assert_eq!(other["layout"], yaml("{a: 1, b: [c, d]}"));
        assert!(other["tags"].is_sequence());
        assert!(other["layout"].is_mapping());
    }

    #[test]
    fn test_other_uppercase_bool() {
        let other = read_other(&[("yes", "TRUE"), ("no", "FALSE"), ("mixed", "True")]).unwrap();
        assert_eq!(other["yes"], Value::Bool(true));
        assert_eq!(other["no"], Value::Bool(false));
        // Only the exact uppercase spelling used by Google Sheets is a boolean
        assert_eq!(other["mixed"], Value::String("True".into()));
    }

    #[test]
    fn test_other_empty_removed() {
        let other = read_other(&[("empty", ""), ("kept", "x")]).unwrap();
        assert!(!other.contains_key("empty"));
        assert_eq!(other["kept"], Value::String("x".into()));
    }

    #[test]
    fn test_other_invalid_yaml() {
        let err = read_other(&[("tags", "[unclosed")]).unwrap_err();
        assert!(err.to_string().contains("tags"), "error should name the column: {}", err);
    }

    #[test]
    fn test_other_leading_space_is_string() {
        // The check is on the first character, no trimming
        let other = read_other(&[("v", " [x]")]).unwrap();
        assert_eq!(other["v"], Value::String(" [x]".into()));
    }

    #[test]
    fn test_doc_id_extraction() {

        assert_eq!(
            Some("rUSGSdveGGDaGxuPmDyXus"),
            super::get_doc_id("https://docs.google.com/document/d/rUSGSdveGGDaGxuPmDyXus/edit")
        );

        assert_eq!(
            Some("rUSGSdveGGDaGxuPmDyXus"),
            super::get_doc_id("https://docs.google.com/document/u/0/d/rUSGSdveGGDaGxuPmDyXus/edit")
        );
    }
}
