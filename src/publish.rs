// Other DOM crates:
// https://crates.io/crates/soup -- no DOM modification
// https://crates.io/crates/kuchiki -- no DOM modification
// https://crates.io/crates/markup5ever_rcdom -- no DOM modification, no select
// https://crates.io/crates/lol_html -- only streaming update (no ancestor/child modifications)
// https://crates.io/crates/html5ever_ext - may help, but outdated

use cssparser::CowRcStr;

use crate::css_rules;
use crate::DocData;
use crate::FrontMatter;

use html5ever::QualName;
use indoc::indoc;
use maplit::*;
use markup5ever::{local_name, namespace_url, ns};
use std::fs;
use std::fs::File;
use std::path::Path;

use rayon::prelude::*;

use crate::images;
use std::collections::HashMap;

use lazy_static::lazy_static;

/// Publish all GDoc html content to the Hugo directory. This include cleaning up the html,
/// importing images, extracting front-matter content, etc.
///
pub fn publish(download_dir: &Path, hugo_dir: &Path, default_author: Option<String>, all: bool) -> anyhow::Result<()> {
    let toc_path = download_dir.join("pages.csv");

    let toc_file = File::open(&toc_path).with_context(|| format!("Failed to open ToC file {:?}", &toc_path))?;

    let records = DocData::read_csv(&toc_file).with_context(|| format!("Failed to parse ToC file {:?}", &toc_path))?;

    let url_to_slug = records
        .iter()
        .map(|r| (r.gdoc_pub_url.clone(), r.slug.clone()))
        .collect::<HashMap<_, _>>();

    records
        .into_par_iter()
        .map(|mut record: DocData| {
            if record.author.is_none() {
                record.author = default_author.clone();
            }
            publish_doc(record, download_dir, hugo_dir, &url_to_slug, all)
        })
        .collect::<anyhow::Result<Vec<()>>>()?;

    Ok(())
}

pub fn publish_doc(
    record: DocData,
    download_dir: &Path,
    hugo_dir: &Path,
    url_to_slug: &HashMap<String, String>,
    all: bool,
) -> anyhow::Result<()> {
    if !record.publish && !all {
        println!("Skipping '{}' (not published)", record.slug);
        return Ok(());
    }

    println!("Processing '{}'", record.slug);
    let mut fm = FrontMatter {
        markup: "html",
        date: record.publish_date,
        lastmod: record.update_date,
        author: record.author,
        slug: record.slug,
        gdoc_pub_url: record.gdoc_pub_url,
        ..FrontMatter::default()
    };

    if let Some(category) = record.category {
        fm.categories.push(category);
        fm.weight = record.weight;
    }

    let html_path = download_dir.join(format!("{}.html", fm.slug));
    let html = fs::read_to_string(&html_path).with_context(|| format!("Cannot read content from {:?}", &html_path))?;

    let cleaned_html = cleanup_html(&mut fm, &url_to_slug, &html, &hugo_dir)?;

    let post_path = hugo_dir.join(format!("content/posts/{}.html", fm.slug));
    fs::write(
        &post_path,
        format!(
            indoc! {r#"
                {}
                ---

                {}
            "#},
            serde_yaml::to_string(&fm)?,
            &cleaned_html
        ),
    )
    .with_context(|| format!("Cannot write to {:?}", &post_path))?;

    Ok(())
}

fn cleanup_html(
    fm: &mut FrontMatter,
    url_to_slug: &HashMap<String, String>,
    html: &str,
    hugo_dir: &Path,
) -> anyhow::Result<String> {
    let mut doc = scraper::Html::parse_document(html);

    // Cleanup inline style
    cleanup_style_elts(&mut doc, fm);

    // Remove all <meta>
    remove_meta_elts(&mut doc);

    // "subtitle" is the only gdoc style beyond h1-h6 but needs some cleanup
    cleanup_subtitle_class(&mut doc);

    // Extract title to front matter from <h1> and remove it
    cleanup_h1_and_get_meta(&mut doc, fm);

    // Import all images and rewrite their 'src'
    import_img_elts(&mut doc, hugo_dir)?;

    // Rewrite links and cleanup <a> structure
    rewrite_and_cleanup_a_elts(&mut doc, fm, &url_to_slug);

    // Done!
    crate::html::stable_html(&doc)
}

fn cleanup_css(css: &str) -> String {
    // Found the style node
    let mut cinput = cssparser::ParserInput::new(css);
    let mut cparser = cssparser::Parser::new(&mut cinput);
    let rule_list = cssparser::RuleListParser::new_for_stylesheet(&mut cparser, css_rules::CSSRuleListParser);

    let mut new_style = String::new();

    for parsed in rule_list {
        if let Ok((selector, declarations)) = parsed {
            let selector: &str = selector.trim();

            if !REMOVED_SELECTORS.contains(selector)
                && !REMOVED_SELECTOR_PREFIXES
                    .iter()
                    .any(|prefix| selector.starts_with(prefix))
            {
                new_style.push_str(selector);
                new_style.push_str(" {");

                let declarations: Vec<(CowRcStr, &str)> = declarations;
                for (name, value) in &declarations {
                    let name = name.as_ref().trim();
                    let value = value.trim();

                    if !REMOVED_PROPS.contains(name) && !remove_property(name, value) {
                        //new_style.push_str("  ");
                        new_style.push_str(name);
                        new_style.push_str(" : ");
                        new_style.push_str(value);
                        new_style.push_str("; ");
                    }
                    //println!("  '{}': '{}';", name, value);
                }

                new_style.push_str("} ");
            } else {
                //println!("Removing selector {}", selector);
            }
            //println!("'{:}'", selector);
        }
    }

    //println!("{}", new_style);
    new_style
}

/// `<style>` -
///
fn cleanup_style_elts(doc: &mut scraper::Html, fm: &mut FrontMatter) {
    let selector = scraper::Selector::parse("style").unwrap();

    // Can't mutate the DOM when we have the result of select in scope as it holds an immutable
    // reference to it.
    let mut id: Option<ego_tree::NodeId> = None;

    if let Some(element) = doc.select(&selector).next() {
        // Found the style node
        let css = cleanup_css(element.text().next().unwrap());
        fm.inline_style = Some(css);
        id = Some(element.id());
    }

    if let Some(real_id) = id {
        let mut text_node = doc.tree.get_mut(real_id).unwrap();
        text_node.detach();
    }
}

fn remove_meta_elts(doc: &mut scraper::Html) {
    let selector = scraper::Selector::parse("meta").unwrap();
    let ids = doc.select(&selector).map(|elt| elt.id()).collect::<Vec<_>>();
    for id in ids {
        doc.tree.get_mut(id).unwrap().detach();
    }
}

/// `.subtitle` - only keep this class (generated class may conflict)
/// and remove child span class (we should remove that element, actually)
///
fn cleanup_subtitle_class(doc: &mut scraper::Html) {
    let selector = scraper::Selector::parse(".subtitle").unwrap();
    let ids = doc.select(&selector).map(|elt| elt.id()).collect::<Vec<_>>();
    let class_name = QualName::new(None, ns!(), local_name!("class"));
    for id in ids {
        let mut node = doc.tree.get_mut(id).unwrap();
        if let scraper::Node::Element(elt) = node.value() {
            if let Some(v) = elt.attrs.get_mut(&class_name) {
                *v = "subtitle".into();
            }
        }

        let mut child = node.first_child().unwrap();
        if let scraper::Node::Element(elt) = child.value() {
            elt.attrs.remove(&class_name);
        }
    }
}

/// `<h1>` - extract title, summary and remove tag
///
fn cleanup_h1_and_get_meta(doc: &mut scraper::Html, fm: &mut FrontMatter) {
    let selector = scraper::Selector::parse("h1").unwrap();
    let mut ids = Vec::new();

    if let Some(h1) = doc.select(&selector).next() {
        let txt = h1.text().collect::<Vec<_>>().join(" ");
        ids.push(h1.id());
        fm.title = txt;

        // Summary is all the text preceding <h1>
        let mut summary = String::new();
        for sibling in h1.prev_siblings() {
            if let Some(elt) = scraper::ElementRef::wrap(sibling) {
                summary.push_str(&elt.text().collect::<Vec<_>>().join(" "));
                summary.push_str(" ");
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
}

/// `<a>` - reformat links (internal and external) and remove enclosing span
///
fn rewrite_and_cleanup_a_elts(doc: &mut scraper::Html, fm: &mut FrontMatter, url_to_slug: &HashMap<String, String>) {
    let selector = scraper::Selector::parse("a").unwrap();
    let ids = doc.select(&selector).map(|elt| elt.id()).collect::<Vec<_>>();

    for id in ids {
        let mut node = doc.tree.get_mut(id).unwrap();

        // Remove wrapper <span> around <a>
        remove_parent_span(&mut node);

        if let scraper::Node::Element(elt) = node.value() {
            // scraper's DOM isn't really meant for mutability: '.classes' is set only in the
            // constructor from attrs. So to modify classes we actually have to tweak attrs.
            //elt.classes.clear();
            let qualname = QualName::new(None, ns!(), local_name!("class"));
            elt.attrs.remove(&qualname);

            // Remove redirections through google.com
            let qualname = QualName::new(None, ns!(), local_name!("href"));

            if let Some(href) = elt.attrs.get_mut(&qualname) {
                if href.starts_with("https://www.google.com/url?") {
                    let url = reqwest::Url::parse(href).unwrap();
                    if let Some((_, v)) = url.query_pairs().find(|(k, _)| k == "q") {
                        *href = v.trim().into();
                    }
                }

                if href.starts_with("https://docs.google.com/document/") {
                    if let Some(slug) = url_to_slug.get(href.as_ref()) {
                        *href = format!("/{}/", slug).into();
                    } else {
                        eprintln!("Warning: link to a gdoc in {} - {}", fm.slug, href);
                    }
                }
            }
        }
    }
}

/// `<img>` - import pictures
///
fn import_img_elts(doc: &mut scraper::Html, hugo_dir: &Path) -> anyhow::Result<()> {
    let selector = scraper::Selector::parse("img").unwrap();
    let src_name = QualName::new(None, ns!(), local_name!("src"));

    // Collect all <img> ids and src attributes
    let ids_and_src = doc
        .select(&selector)
        .flat_map(|elt| {
            if let Some(src) = elt.value().attrs.get(&src_name) {
                Some((elt.id(), src.to_string()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    // Parallel import all images and rewrite src
    let ids_and_new_src = ids_and_src
        .into_par_iter()
        .map(|(id, src)| {
            let new_src = images::import_image(src.deref(), &hugo_dir)?;
            Ok((id, new_src))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    for (id, src) in ids_and_new_src {
        let mut node = doc.tree.get_mut(id).unwrap();
        if let scraper::Node::Element(elt) = node.value() {
            elt.attrs.insert(src_name.clone(), src.into());
        }
    }

    Ok(())
}

fn remove_property(name: &str, value: &str) -> bool {
    name == "text-align" && value == "justify"
}

use anyhow::Context;
use std::collections::HashSet;
use std::ops::Deref;
lazy_static! {
    static ref REMOVED_SELECTORS: HashSet<&'static str> = hashset!{
        "ol",
        "table td,table th",
        ".title", // added in theme
        ".subtitle", // added in theme
        "h1", "h2", "h3", "h4", "h5", "h6",
        "li",
        "p",
    };

    static ref REMOVED_SELECTOR_PREFIXES: Vec<&'static str> = vec!{
        "ul.",
        ".lst-kix_",
    };

    static ref REMOVED_PROPS: HashSet<&'static str> = hashset!{
        "background-color",
        "orphans",
        "widows",
        "vertical-align",
        "font-family",
        "font-size",
        "line-height",
        "page-break-after",
        "margin",
        "margin-left",
        "margin-right",
        "padding",
        "padding-bottom",
        "padding-left",
        "padding-right",
        "padding-top",
        "text-decoration-skip-ink",
        "-webkit-text-decoration-skip",
        "height",
        "color",
        "content",
        "list-style-type",
        "max-width",
    };
}

fn remove_parent_span(node: &mut ego_tree::NodeMut<scraper::Node>) {
    let node_id = node.id();
    let mut parent = node.parent().unwrap();
    if let Some(elt) = parent.value().as_element() {
        if elt.name.local == local_name!("span") {
            // FIXME: should also check that the node is the only child
            parent.insert_id_after(node_id);
            parent.detach();
        }
    }
}
