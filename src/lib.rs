#[macro_use]
extern crate serde_derive;

use std::collections::HashMap;

use anyhow::anyhow;
use lazy_static::lazy_static;
use gdocs::DateTimeWithDefault;

pub mod config;
mod css_rules;
pub mod gdocs;
mod html;
mod images;
pub mod publish;
mod hugo;

use regex::Regex;

#[derive(Default)]
pub struct SiteData {
    url_to_slug: HashMap<String, String>,
}

lazy_static! {
    static ref DOC_USER_RE: Regex = Regex::new("/document/u/[0-9]/").unwrap();
}

impl SiteData {
    pub fn translate_url(&self, url: &str) -> anyhow::Result<&str> {
        // Link urls may contain '/document/u/{id}/' that should just be '/document/'
        let url = DOC_USER_RE.replace(url, "/document/");

        self.url_to_slug.get(url.as_ref())
            .map(|slug| slug.as_str())
            .ok_or_else(|| anyhow!("Google doc not found in site pages {}", url))
    }
}
