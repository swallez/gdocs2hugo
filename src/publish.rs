use std::collections::{BTreeMap, HashMap};
use crate::{config, SiteData, tweaks};
use crate::gdocs_site;
use crate::gdocs_site::DocData;
use crate::gdoc_to_html;
use std::fs;
use std::path::Path;
use anyhow::Result;
use anyhow::Context;
use anyhow::anyhow;
use bytes::Buf;
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;
use indoc::indoc;
use rayon::prelude::*;
use tendril::fmt::Slice;
use crate::gdoc_to_html::ImageReference;
use crate::hugo_site::FrontMatter;
use crate::images;
use itertools::Itertools;

#[derive(Serialize, Deserialize)]
struct DataItem {
    id: String,
    #[serde(flatten)]
    fields: BTreeMap<String, String>,
}

pub fn publish(config: &config::Config, store: bool, all: bool) -> Result<()> {

    // Build a tokio runtime to call GDoc API async functions.
    // Use the default multi-threaded runtime.
    let rt = tokio::runtime::Runtime::new()?;
    let _guard = rt.enter();

    //----- Create GDrive & GDocs API client

    let config_path = &config.credentials.as_ref().ok_or(anyhow!("Missing credentials in config"))?;
    let gdrive_api = rt.block_on(create_gdrive_client(config_path))?;
    let gdocs_api = rt.block_on(create_gdocs_client(config_path))?;

    //----- Read ToC

    println!("Loading table of contents.");
    let docs = download_toc(config, &gdrive_api, store)?;

    //----- Build site data

    let site_data = SiteData::new(&docs)?;

    //----- Publish docs

    //docs.into_par_iter()
    docs.into_iter()
        .map(|site_doc| {
            if !site_doc.publish && !all {
                println!("Skipping '{}' (not published)", site_doc.slug);
                return Ok(());
            }

            let rt_guard = rt.enter();


            if site_doc.slug.starts_with("/#data/") {

                let sheet_id = gdocs_site::get_doc_id(site_doc.gdoc_url.as_ref().unwrap())
                    .ok_or_else(|| anyhow!("Failed to extract doc id from {:?}", site_doc.gdoc_url))?;

                let bytes: bytes::Bytes = tokio::runtime::Handle::current().block_on(async {
                    let mut response = gdrive_api.files().export(sheet_id, "text/csv").doit().await?;
                    let bytes = hyper::body::to_bytes(response.body_mut()).await?;
                    <Result<_>>::Ok(bytes)
                })
                    .context("Failed to download data document")?;

                // let mut data = DataItem::read_csv(bytes.clone().reader())
                //     .context("Problem reading ToC spreadsheet")?;

                let mut rdr = csv::ReaderBuilder::new().from_reader(bytes.reader());

                let items = rdr.deserialize()
                    // First line after the header is the human-readable column names: skip it
                    .skip(1)
                    .map_ok(|mut data: DataItem| {
                        (data.id, data.fields)
                    })
                    .collect::<csv::Result<BTreeMap<String, BTreeMap<String, String>>>>()?;

                let path = config.hugo_site_dir.join(&site_doc.slug[2..]).with_extension("yml");

                serde_yaml::to_writer(std::fs::File::create(&path)?, &items)?;

                println!("Saved data file to {:?}", path);
                return Ok(());
            }

            //----- Load doc JSON
            println!("Downloading {}", &site_doc.slug);
            let gdoc = download_gdoc_json(&site_doc, &config, &gdocs_api, &rt, store)?;

            //----- Convert doc JSON to HTML and DOM
            let html = gdoc_to_html::render(&gdoc)?;

            if store {
                let doc_path = &config.download_dir
                    .join(rel_path_or_index(&site_doc.slug))
                    .with_extension("html");
                fs::write(&doc_path, &html)
                    .with_context(|| format!("Failed to write html rendering {:?}", &doc_path))?;

                println!("Saved rendered html to {:?}", doc_path);
            }

            let mut dom = scraper::Html::parse_document(&html);

            //----- Prepare Front matter
            let categories: Vec<String> = if let Some(category) = site_doc.category {
                vec![category]
            } else {
                Vec::new()
            };

            let flat_slug = site_doc.slug.replace('/', "_");
            let doc_id = gdocs_site::get_doc_id(site_doc.gdoc_url.as_ref().unwrap())
                .unwrap().to_owned();

            let mut fm = FrontMatter {
                markup: "html",
                date: site_doc.publish_date,
                lastmod: site_doc.update_date,
                author: site_doc.author,
                slug: flat_slug,
                url: Some(site_doc.slug),
                gdoc_url: site_doc.gdoc_url,
                weight: site_doc.weight,
                categories,
                other: site_doc.other,
                ..FrontMatter::default()
            };

            //----- Apply tweaks

            tweak_dom(&doc_id, &mut dom, &mut fm, &site_data, &config, store)?;

            //----- And store to its final location

            let hugo_dir = &config.hugo_site_dir;

            write_doc(&dom, &fm, &site_data, hugo_dir)?;

            Ok(())

        })
        .collect::<anyhow::Result<Vec<()>>>()?;

    Ok(())
}

//--------------------------------------------------------------------------------------------------
///
/// Write doc
///
pub fn write_doc(dom: &scraper::Html, fm: &FrontMatter, site_data: &SiteData, hugo_dir: impl AsRef<Path>) -> Result<()> {
    let hugo_dir = hugo_dir.as_ref().to_owned();
    let doc_slug = fm.url.as_ref().unwrap();
    let post_path = if fm.categories.is_empty() {
        // See https://gohugo.io/content-management/page-bundles/
        let has_children = site_data.id_to_slug.values()
            .any(|s| s.len() != doc_slug.len() && s.starts_with(doc_slug));
        if has_children {
            // Branch bundle
            hugo_dir.join(format!("content{}/_index.html", doc_slug))
        } else {
            // Leaf page
            hugo_dir.join(format!("content{}/index.html", doc_slug))
        }
    } else {
        hugo_dir.join(format!("content/posts{}.html", doc_slug))
    };

    let cleaned_html = crate::from_web_pub::serialize::stable_html(&dom)?;
    println!("Writing {:?}", &post_path);

    fs::create_dir_all(post_path.parent().unwrap())?;

    fs::write(
        &post_path,
        format!(
            indoc! {r#"
                ---
                {}
                ---

                {}
            "#},
            serde_yaml::to_string(&fm)?,
            &cleaned_html
        ),
    ).with_context(|| format!("Cannot write to {:?}", &post_path))?;

    Ok(())
}

//--------------------------------------------------------------------------------------------------
///
/// Tweak the raw document, extracting front-matter information, downloading images, etc
///
pub fn tweak_dom(_doc_id: &str, dom: &mut scraper::Html, fm: &mut FrontMatter, site_data: &SiteData, config: &config::Config, store: bool) -> Result<()> {

    tweaks::remove_head(dom);

    tweaks::import_img_elts(dom, |img| download_image(
            img,
            fm.url.as_ref().unwrap(),
            &config.hugo_site_dir,
            store.then(|| config.download_dir.as_path()))
    )?;

    tweaks::rewrite_links(dom, site_data)?;

    // Must be done last, after image and link URL rewriting
    tweaks::extract_title_and_summary(dom, fm)?;

    tweaks::move_bootstrap_btn_classes(dom)?;

    Ok(())
}

pub fn download_image(img: &ImageReference, url: &str, site_dir: impl AsRef<Path>, store_path: Option<&Path>) -> Result<String> {

    // if let Some(path) = store_path {
    //     let json_path = config.download_dir
    //         .join(&site_doc.slug[1..])
    //         .with_extension(".json");
    // }

    let base_path = site_dir.as_ref().join("content").join(&url[1..]).join(img.id);

    let extension = images::download_and_store(img.src, base_path, |_img_bytes, extension| {
        if let Some(path) = store_path {
            let img_path = path
                .join(&url[1..])
                .with_extension(extension);

            println!("Would store original image at {:?}", img_path);
        }
    })?;

    // Image is stored in the page's directory, so return a relative url
    Ok(format!("{}.{}", img.id, extension))

}
//--------------------------------------------------------------------------------------------------
///
/// Download the site's table of content from the CVS export of the ToC spreadsheet
///
pub fn download_toc (config: &config::Config, gdrive: &google_drive3::DriveHub<HyperC>, store: bool) -> Result<Vec<DocData>> {

    let toc_id = gdocs_site::get_doc_id(&config.toc_spreadsheet_url)
        .ok_or_else(|| anyhow!("Cannot extract ToC doc id from {}", config.toc_spreadsheet_url))?;

    let bytes: bytes::Bytes = tokio::runtime::Handle::current().block_on(async {
            let mut response = gdrive.files().export(toc_id, "text/csv").doit().await?;
            let bytes = hyper::body::to_bytes(response.body_mut()).await?;
            <Result<_>>::Ok(bytes)
        })
        .context("Failed to download ToC spreadsheet")?;

    let mut docs = DocData::read_csv(bytes.clone().reader())
        .context("Problem reading ToC spreadsheet")?;

    for doc in &mut docs {
        if doc.author.is_none() {
            doc.author = config.default_author.clone();
        }
    }

    if store {
        std::fs::create_dir_all(&config.download_dir)?;
        let toc_path = &config.download_dir.join("pages.csv");
        let byte_array = bytes.as_bytes();
        fs::write(&toc_path, &byte_array)
            .with_context(|| format!("Failed to write ToC spreadsheet {:?}", &toc_path))?;

        println!("Saved table of contents to {:?}", toc_path);
    }

    Ok(docs)
}

//--------------------------------------------------------------------------------------------------
///
/// Create the Google Docs client
///
pub async fn create_gdocs_client(creds_path: impl AsRef<Path>) -> Result<google_docs1::Docs> {

    let creds = google_docs1::oauth2::read_service_account_key(creds_path).await
        .context("Problem loading credentials")?;

    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    let client = hyper::Client::builder()
        .build::<_, hyper::Body>(connector);

    let auth = google_docs1::oauth2::ServiceAccountAuthenticator::builder(creds)
        .hyper_client(client.clone())
        .build()
        .await
        .context("Problem creating GDocs client")?;

    let gdocs_api = google_docs1::Docs::new(client.clone(), auth);

    Ok(gdocs_api)
}

type HyperC = HttpsConnector<HttpConnector>;

//--------------------------------------------------------------------------------------------------
///
/// Create the Google Docs client
///
pub async fn create_gdrive_client(creds_path: impl AsRef<Path>) -> Result<google_drive3::DriveHub<HyperC>> {

    let creds = google_drive3::oauth2::read_service_account_key(creds_path).await
        .context("Problem loading credentials")?;

    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    let client = hyper::Client::builder()
        .build::<_, hyper::Body>(connector);

    let auth = google_drive3::oauth2::ServiceAccountAuthenticator::builder(creds)
        .hyper_client(client.clone())
        .build()
        .await
        .context("Problem creating GDocs client")?;

    let gdrive_api = google_drive3::DriveHub::new(client.clone(), auth);

    Ok(gdrive_api)
}
//--------------------------------------------------------------------------------------------------
///
/// Download a Google doc in its JSON form from its URL, and optionally stores it for debugging
/// purposes.
///
pub fn download_gdoc_json(
    site_doc: &DocData,
    config: &config::Config,
    gdocs_api: &google_docs1::Docs,
    rt: &tokio::runtime::Runtime,
    store: bool,
) -> Result<google_docs1::api::Document> {
    let url = site_doc.gdoc_url.as_ref()
        .ok_or_else(|| anyhow!("{} - No GDoc URL in table of contents", site_doc.slug))?;

    let doc_id = gdocs_site::get_doc_id(url)
        .ok_or_else(|| anyhow!("{} - URL is not a GDoc: {}", site_doc.slug, url))?;

    let gdoc = rt.block_on(gdocs_api.documents().get(doc_id).doit())
        .with_context(|| format!("{} - Failed to load document.", site_doc.slug))?
        .1;

    if store {
        let json_path = config.download_dir
            .join(rel_path_or_index(&site_doc.slug))
            .with_extension("json");

        fs::create_dir_all(json_path.parent().unwrap())?;
        serde_json::to_writer_pretty(fs::File::create(json_path)?, &gdoc)?;
    }

    Ok(gdoc)
}

fn rel_path_or_index(slug: &str) -> &str {
    if slug == "/" {
        "index"
    } else {
        &slug[1..]
    }
}
