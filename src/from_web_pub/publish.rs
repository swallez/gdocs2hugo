// Other DOM crates:
// https://crates.io/crates/soup -- no DOM modification
// https://crates.io/crates/kuchiki -- no DOM modification
// https://crates.io/crates/markup5ever_rcdom -- no DOM modification, no select
// https://crates.io/crates/lol_html -- only streaming update (no ancestor/child modifications)
// https://crates.io/crates/html5ever_ext - may help, but outdated

use cssparser::CowRcStr;

use crate::SiteData;
use crate::gdocs_site::DocData;
use crate::hugo_site::FrontMatter;

use html5ever::QualName;
use indoc::indoc;
use maplit::*;
use html5ever::{local_name, namespace_url, ns};
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

    let mut url_to_slug = HashMap::<String, String>::new();
    for record in &records {
        if let Some(url) = record.gdoc_pub_url.as_ref() {
            url_to_slug.insert(url.clone(), record.slug.clone());
        }
        if let Some(url) = record.gdoc_url.as_ref() {
            url_to_slug.insert(url.clone(), record.slug.clone());
        }
    }

    let site_data = SiteData {
        url_to_slug
    };

    records
        .into_par_iter()
        .map(|mut record: DocData| {
            if record.author.is_none() {
                record.author = default_author.clone();
            }
            publish_doc(record, download_dir, hugo_dir, &site_data, all)
        })
        .collect::<anyhow::Result<Vec<()>>>()?;

    Ok(())
}

pub fn publish_doc(
    record: DocData,
    download_dir: &Path,
    hugo_dir: &Path,
    site_data: &SiteData,
    all: bool,
) -> anyhow::Result<()> {
    if !record.publish && !all {
        println!("Skipping '{}' (not published)", record.slug);
        return Ok(());
    }

    println!("Processing '{}'", record.slug);

    let post_path = if record.category.is_none() {
        // See https://gohugo.io/content-management/page-bundles/
        let has_children = site_data.url_to_slug.values()
            .any(|s| s.len() != record.slug.len() && s.starts_with(&record.slug));
        if has_children {
            // Branch bundle
            hugo_dir.join(format!("content{}/_index.html", record.slug))
        } else {
            // Leaf page
            hugo_dir.join(format!("content{}.html", record.slug))
        }
    } else {
        hugo_dir.join(format!("content/posts{}.html", record.slug))
    };

    let categories: Vec<String> = if let Some(category) = record.category {
        vec![category]
    } else {
        Vec::new()
    };

    let flat_slug = record.slug.replace('/', "_");
    let mut fm = FrontMatter {
        markup: "html",
        date: record.publish_date,
        lastmod: record.update_date,
        author: record.author,
        slug: flat_slug,
        url: Some(record.slug),
        gdoc_pub_url: record.gdoc_pub_url.unwrap(),
        weight: record.weight,
        categories,
        ..FrontMatter::default()
    };

    let html_path = download_dir.join(record.download_path);
    let html = fs::read_to_string(&html_path).with_context(|| format!("Cannot read content from {:?}", &html_path))?;
    let cleaned_html = cleanup_html(&mut fm, site_data, &html, &hugo_dir)?;

    //println!("Writing {:?}", &post_path);

    fs::create_dir_all(post_path.parent().unwrap())?;

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
    site_data: &SiteData,
    html: &str,
    hugo_dir: &Path,
) -> anyhow::Result<String> {
    let mut doc = scraper::Html::parse_document(html);

    // Remove script elements
    remove_script_elts(&mut doc);

    // Cleanup inline style
    cleanup_style_elts(&mut doc, fm);

    // Remove all <meta>
    remove_meta_elts(&mut doc);

    // "subtitle" is the only gdoc style beyond h1-h6 but needs some cleanup
    cleanup_subtitle_class(&mut doc);

    // Import all images and rewrite their 'src'
    import_img_elts(&mut doc, hugo_dir)?;

    // Extract title to front matter and banner from <h1> and remove it
    cleanup_h1_and_get_meta(&mut doc, fm);

    // Remove class from nested <hx><span>
    cleanup_headers(&mut doc, fm);

    // Rewrite links and cleanup <a> structure
    rewrite_and_cleanup_a_elts(&mut doc, fm, site_data)?;

    // Done!
    crate::from_web_pub::serialize::stable_html(&doc)
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

            if selector.starts_with("ul.lst-kix_") {
                // GDocs doesn't nest lists: child lists are flattened and given a bigger indent.
                // Class names look like "lst-kix_x48xhles5rch-1" where the "-1" suffix is the
                // nesting level (starts at zero)
                let depth = i8::from_str(&selector[selector.rfind('-').unwrap()+1 ..]).unwrap();

                new_style.push_str(selector);
                new_style.push_str(" { padding-left: ");

                new_style.push_str(&format!("{}", (depth as f32)*2.0 + 2.0));
                new_style.push_str("em; } ");

            } else if !REMOVED_SELECTORS.contains(selector)
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

fn remove_script_elts(doc: &mut scraper::Html) {
    let selector = scraper::Selector::parse("script").unwrap();
    let ids = doc.select(&selector).map(|elt| elt.id()).collect::<Vec<_>>();
    for id in ids {
        doc.tree.get_mut(id).unwrap().detach();
    }
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
    let h1_selector = scraper::Selector::parse("h1").unwrap();
    let img_selector = scraper::Selector::parse("img").unwrap();
    let mut ids = Vec::new();

    if let Some(h1) = doc.select(&h1_selector).next() {
        let txt = h1.text().collect::<Vec<_>>().join(" ");
        ids.push(h1.id());
        fm.title = txt;

        // Summary is all the text preceding <h1>
        let mut summary = String::new();
        for sibling in h1.prev_siblings().collect::<Vec<_>>().into_iter().rev() { // prev_siblings iterates in reverse order
            if let Some(elt) = scraper::ElementRef::wrap(sibling) {
                summary.push_str(&elt.text().collect::<Vec<_>>().join(" "));
                summary.push_str(" ");

                // Img above <h1> becomes the article banner
                if let Some(img) = elt.select(&img_selector).next() {
                    let src = QualName::new(None, ns!(), local_name!("src"));
                    fm.banner = img.value().attrs.get(&src).map(|s| s.to_string());
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
}

/// Remove the class attribute from the child <span> of header tags
///
fn cleanup_headers(doc: &mut scraper::Html, _fm: &mut FrontMatter) {
    let selector = scraper::Selector::parse("h2").unwrap();
    let ids = doc.select(&selector).map(|elt| elt.id()).collect::<Vec<_>>();

    for id in ids {
        let mut node = doc.tree.get_mut(id).unwrap();
        remove_child_span_classes(&mut node);
    }
}

/// `<a>` - reformat links (internal and external) and remove enclosing span
///
fn rewrite_and_cleanup_a_elts(doc: &mut scraper::Html, _fm: &mut FrontMatter, site_data: &SiteData) -> anyhow::Result<()> {
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
                    let (url, frag) = href.split_once('#').unwrap_or_else(|| (href, ""));
                    let translated_url = site_data.translate_url(url)?;

                    if frag.is_empty() {
                        *href = translated_url.into();
                    } else {
                        let frag = frag.to_string();
                        *href = translated_url.into();
                        href.push_char('#');
                        href.push_slice(&frag);
                    }
                }
            }
        }
    }

    Ok(())
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
use std::str::FromStr;
use crate::from_web_pub::css_rules;
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
        "counter-reset",
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

fn remove_child_span_classes(node: &mut ego_tree::NodeMut<scraper::Node>) {
    if let Some(mut child) = node.first_child() {
        if let scraper::Node::Element(elt) = child.value() {
            elt.attrs.clear();
        }
    }
}
