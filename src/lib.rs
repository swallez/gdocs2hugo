#[macro_use]
extern crate serde_derive;

use std::collections::HashMap;

use anyhow::{anyhow, bail};
use lazy_static::lazy_static;
use gdocs_site::DateTimeWithDefault;

pub mod config;
pub mod gdocs_site;
mod images;
mod hugo_site;
pub mod gdoc_to_html;
pub mod from_web_pub;
pub mod experiments;
pub mod publish;
pub mod html;
mod tweaks;

use regex::Regex;
use crate::gdocs_site::DocData;

#[derive(Default)]
pub struct SiteData {
    url_to_slug: HashMap<String, String>,
    id_to_slug: HashMap<String, String>,
}

lazy_static! {
    static ref DOC_USER_RE: Regex = Regex::new("/document/u/[0-9]/").unwrap();
}

impl SiteData {

    pub fn new(docs: &Vec<DocData>) -> anyhow::Result<Self> {
        let mut url_to_slug = HashMap::new();
        let mut id_to_slug = HashMap::new();
        for doc in docs {
            if let Some(url) = doc.gdoc_pub_url.as_ref() {
                url_to_slug.insert(url.clone(), doc.slug.clone());
            }
            if let Some(url) = doc.gdoc_url.as_ref() {
                let id = gdocs_site::get_doc_id(url)
                    .ok_or_else(|| anyhow!("Cannot extract doc id from {}", url))?;
                id_to_slug.insert(id.to_string(), doc.slug.clone());
            }
        }

        Ok(SiteData {
            url_to_slug,
            id_to_slug,
        })
    }

    ///
    /// Rewrite a href URL to translate references to GDocs to internal site URLs.
    /// The URL fragment, if any, is kept.
    ///
    pub fn rewrite_href(&self, href: &str) -> anyhow::Result<Option<String>> {
        if let Some((url, frag)) = href.split_once('#') {
            if let Some(mut new_url) = self.rewrite_url(url)? {
                new_url.push('#');
                new_url.push_str(frag);
                Ok(Some(new_url))
            } else {
                Ok(None)
            }
        } else {
            self.rewrite_url(href)
        }
    }

    fn rewrite_url(&self, url: &str) -> anyhow::Result<Option<String>> {
        // It may happen that some links go through a redirect warning page.
        if url.starts_with("https://www.google.com/url?") {
            let url = reqwest::Url::parse(url).unwrap();
            if let Some((_, v)) = url.query_pairs().find(|(k, _)| k == "q") {
                let url_param = v.trim();
                return Ok(self.rewrite_url(url_param)?.or_else(|| Some(url_param.to_string())));
            }
        }

        // Legacy link
        if let Some(slug) = self.url_to_slug.get(url) {
            return Ok(Some(format!("{}/", slug)));
        }

        // Reference to GDocs (internal site links)
        if url.starts_with("https://docs.google.com/document/") {
            return if let Some(id) = gdocs_site::get_doc_id(url) {
                if let Some(slug) = self.id_to_slug.get(id) {
                    // Add a trailing slash
                    Ok(Some(format!("{}/", slug)))
                } else {
                    //Ok(None)
                    bail!("Found a link to a GDoc that is not in the page list: id={}", id)
                }
            } else {
                // Legacy lookup, for "export to web" URLs
                // Link urls may contain '/document/u/{id}/' that should just be '/document/'
                let url = DOC_USER_RE.replace(url, "/document/");

                if let Some(slug) = self.url_to_slug.get(url.as_ref()) {
                    Ok(Some(format!("{}/", slug)))
                } else {
                    //Ok(None)
                    bail!("Google doc url not found in site pages {}", url)
                }
            }
        }

        // No rewrite
        Ok(None)
    }
}
