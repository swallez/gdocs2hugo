use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use crate::hugo_site::FrontMatter;
use scraper::Selector;
use scraper::ElementRef;
use html5ever::namespace_url;
use anyhow::Result;
use anyhow::bail;
use itertools::Itertools;
use rayon::prelude::*;
use crate::gdoc_to_html::ImageReference;
use crate::SiteData;

// Builds a `html5ever::QualName` with no prefix nor namespace
macro_rules! qname {
    ($name:tt) => {
        html5ever::QualName::new(None, html5ever::ns!(), html5ever::local_name!($name))
    };
}

pub fn remove_head(dom: &mut scraper::Html) {
    let selector = scraper::Selector::parse("head").unwrap();
    let ids = dom.select(&selector).map(|elt| elt.id()).collect::<Vec<_>>();
    for id in ids {
        dom.tree.get_mut(id).unwrap().detach();
    }
}

pub fn rewrite_links(dom: &mut scraper::Html, site_data: &SiteData) -> Result<()> {
    let selector = scraper::Selector::parse("a").unwrap();
    let ids = dom.select(&selector).map(|elt| elt.id()).collect::<Vec<_>>();

    for id in ids {
        let mut node = dom.tree.get_mut(id).unwrap();
        if let scraper::Node::Element(elt) = node.value() {
            if let Some(href) = elt.attrs.get_mut(&qname!("href")) {
                if let Some(new_href) = site_data.rewrite_href(href)? {
                    *href = new_href.into();
                }

                if href.starts_with("https://") || href.starts_with("http://") {
                    use std::hash::Hash;
                    let url = reqwest::Url::parse(href)?;
                    match url.host().unwrap() {
                        url::Host::Domain(s) => {
                            let mut hasher = DefaultHasher::new();
                            s.hash(&mut hasher);
                            let h = hasher.finish();
                            let target = format!("{:X}", h);
                            elt.attrs.insert(qname!("target"), target.into());
                        },
                        _ => (),
                    }
                }
            }
        }
    }

    Ok(())
}

/// Extract title, banner and summary:
/// - the content before <h1> becomes the front matter's description and is removed
/// - the first image found in the description becomes the front matter's banner
/// - the <h1> tag is removes, as it's inserted by the page template from the front matter's title
///
/// NOTE: image URLs must have been resolved so that the banner URL is correct.
pub fn extract_title_and_summary(doc: &mut scraper::Html, fm: &mut FrontMatter) -> Result<()> {
    let h1_selector = Selector::parse("h1").unwrap();
    let img_selector = Selector::parse("img").unwrap();
    let mut ids = Vec::new();

    if let Some(h1) = doc.select(&h1_selector).next() {
        let txt = h1.text().join(" ");
        ids.push(h1.id());
        fm.title = txt;

        // Summary is all the text preceding <h1>
        let mut summary = String::new();
        for sibling in h1.prev_siblings().collect::<Vec<_>>().into_iter().rev() { // prev_siblings iterates in reverse order
            if let Some(elt) = ElementRef::wrap(sibling) {
                summary += &elt.text().join(" ");
                summary += " ";

                // Img above <h1> becomes the article banner
                if let Some(img) = elt.select(&img_selector).next() {
                    let src = qname!("src");
                    if let Some(url) = img.value().attrs.get(&src) {
                        if url.starts_with("http") {
                            bail!("Banner image url hasn't been resolved: {}", url);
                        }
                        fm.banner = Some(url.to_string());
                    }
                }
            }
            ids.push(sibling.id());
        }
        if !summary.is_empty() {
            fm.summary = Some(summary);
        }
    }

    for id in ids {
        doc.tree.get_mut(id).unwrap().detach();
    }

    Ok(())
}


/// `<img>` - import pictures
/// The `resolver` takes an image reference (id & src) and returns the new value for the `src` attribute.
///
pub fn import_img_elts(doc: &mut scraper::Html, resolver: impl Fn(&ImageReference) -> Result<String> + Send + Sync) -> Result<()> {
    let selector = scraper::Selector::parse("img").unwrap();

    // Collect all <img> ids and src attributes
    let ids_and_src = doc
        .select(&selector)
        .map(|elt| {
            let src = elt.value().attrs.get(&qname!("src")).unwrap().to_string();
            let img_id = elt.value().attrs.get(&qname!("id")).unwrap().to_string();
            (elt.id(), img_id, src)
        })
        .collect::<Vec<_>>();

    // Parallel import all images and rewrite src
    let ids_and_new_src = ids_and_src
        .into_par_iter()
        .map(|(node_id, img_id, src)| {
            let img_ref = ImageReference {
                id: &img_id,
                src: &src
            };
            let new_src = resolver(&img_ref)?;

            Ok((node_id, new_src))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    for (id, src) in ids_and_new_src {
        let mut node = doc.tree.get_mut(id).unwrap();
        if let scraper::Node::Element(elt) = node.value() {
            elt.attrs.insert(qname!("src"), src.into());
        }
    }

    Ok(())
}
