//! Representation of the Google Docs that constitute the website.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use rayon::prelude::*;
use std::fs;
use serde::{Deserialize, Deserializer};
use chrono::{TimeZone, Utc};
use itertools::Itertools;
use lazy_static::lazy_static;
use regex::Regex;

pub fn download_toc(toc_url: &str, download_dir: &Path) -> anyhow::Result<Vec<DocData>> {
    fs::create_dir_all(download_dir).with_context(|| format!("Cannot create directory {:?}", download_dir))?;

    println!("Downloading ToC from {}", toc_url);
    let csvtext = reqwest::blocking::get(toc_url)?
        .text()
        .context("Failed to download ToC spreadsheet")?;

    let toc_path = download_dir.join("pages.csv");
    fs::write(&toc_path, &csvtext).with_context(|| format!("Failed to write ToC spreadsheet {:?}", &toc_path))?;

    let docs = DocData::read_csv(csvtext.as_bytes())?;

    Ok(docs)
}

pub fn download_html_docs(docs: &Vec<DocData>, download_dir: &Path, all: bool) -> anyhow::Result<()> {
    let rt = tokio::runtime::Handle::try_current();
    docs.par_iter()
        .map(|doc| {
            let _guard = rt.as_ref().map(|rt| rt.enter());
            download_html_doc(&doc, download_dir, all)
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

    let content = reqwest::blocking::get(&format!("{}?embedded=true", &doc.gdoc_pub_url.as_ref().unwrap()))
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
    #[serde(flatten)]
    pub other: BTreeMap<String, String>
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
                doc.other.retain(|_, v| v.len() > 0);

                // Done
                doc
            })
            .collect()
    }
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

lazy_static! {
    static ref DOC_ID_RE: Regex =
        Regex::new("^https://docs.google.com/(document|spreadsheets)(/u/[0-9]+)?/d/([^/]+)/").unwrap();
}

/// Extracts the document id, if any, from a GDocs URL
pub fn get_doc_id(url: &str) -> Option<&str> {
    DOC_ID_RE.captures(url)
        .and_then(|captures| captures.get(3))
        .map(|m| m.as_str())
}

#[cfg(test)]
mod test {

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
