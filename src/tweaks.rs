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
use tendril::StrTendril;
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
            fm.description = Some(summary.clone());
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
    let selector = Selector::parse("img").unwrap();

    // Collect all <img> ids and src attributes
    let ids_and_src = doc
        .select(&selector)
        .map(|elt| {
            let src = elt.value().attrs.get(&qname!("src")).unwrap().to_string();
            if src.starts_with("/") {
                // local path to a static asset added with the {{ html }} shortcode.
                // Do not download and resize
                None
            } else {
                let img_id = elt.value().attrs.get(&qname!("id")).unwrap().to_string();
                Some((elt.id(), img_id, src))
            }
        })
        .flatten()
        .collect::<Vec<_>>();

    // Parallel import all images and rewrite src
    let rt = tokio::runtime::Handle::try_current();
    let ids_and_new_src = ids_and_src
        .into_par_iter()
        .map(|(node_id, img_id, src)| {
            let _guard = rt.as_ref().map(|rt| rt.enter());
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

pub fn move_bootstrap_btn_classes(doc: &mut scraper::Html) -> Result<()> {

    // FIXME: scraper's DOM (ego-tree) is a major PITA to mutate several node values
    // (e.g. move attributes around) without modifying the tree structure. Need to explore alternatives.

    use scraper::Node::Element;

    let selector = Selector::parse(".btn > p > a").unwrap();

    let id_tuples = doc.select(&selector)
        .map(|elt| {
            (
                elt.parent().unwrap().parent().unwrap().id(),
                elt.parent().unwrap().id(),
                elt.id())
        })
        .collect::<Vec<_>>();

    for (div_id, p_id, a_id) in id_tuples {

        // Extract and remove "btn*" classes from the div node.
        let mut btn_classes = StrTendril::new();

        {
            let mut div_node = doc.tree.get_mut(div_id).unwrap();
            if let Element(div_elt) = div_node.value() {
                let new_div_class = div_elt.attrs.get_mut(&qname!("class")).unwrap()
                    .split_whitespace()
                    .map(|class| {
                        if class.starts_with("btn") {
                            btn_classes.push_slice(" ");
                            btn_classes.push_slice(class);
                            None
                        } else {
                            Some(class)
                        }
                    })
                    .flatten()
                    .join(" ");

                div_elt.attrs.insert(qname!("class"), new_div_class.into());
            }
        }

        // Add the btn classes to the anchor node.
        {
            let mut a_node = doc.tree.get_mut(a_id).unwrap();
            if let Element(a_elt) = a_node.value() {
                a_elt.attrs.insert(qname!("class"), btn_classes);
            }
        }

        // Move p's style attribute to its parent div
        let mut p_style = None::<StrTendril>;

        {
            let p_node = doc.tree.get(p_id).unwrap();
            let p_elt = p_node.value().as_element().unwrap();
            if let Some(style) = p_elt.attrs.get(&qname!("style")) {
                p_style = Some(style.clone());
            }
        }

        if let Some(style) = p_style {
            let mut div_node = doc.tree.get_mut(div_id).unwrap();
            if let Element(div_elt) = div_node.value() {
                div_elt.attrs.insert(qname!("style"), style);
            }
        }

        // Restructure the tree: remove the p elt and reparent its children.
        doc.tree.get_mut(div_id).unwrap().reparent_from_id_append(p_id);
        doc.tree.get_mut(p_id).unwrap().detach();
    }

    Ok(())
}
