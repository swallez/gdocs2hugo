use std::collections::BTreeMap;
use crate::DateTimeWithDefault;

#[derive(Debug, Serialize, Default)]
pub struct FrontMatter {
    pub markup: &'static str,
    pub author: Option<String>,
    pub title: String,
    pub date: Option<DateTimeWithDefault>,
    pub lastmod: Option<DateTimeWithDefault>,
    pub banner: Option<String>,
    pub slug: String,
    pub url: Option<String>,
    pub categories: Vec<String>,
    // "weight" should be "categories_weight" but it doesn't seem to work as advertised in Hugo's docs.
    pub weight: Option<i16>,
    pub summary: Option<String>,
    pub description: Option<String>, // same as summary, Hugo uses summary for page lists, and description for SEO
    pub inline_style: Option<String>,
    // not used in the publication process, but useful to distinguish generated pages
    pub gdoc_url: Option<String>,
    #[serde(flatten)]
    pub other: BTreeMap<String, String>
}
