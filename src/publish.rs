use crate::config;
use crate::gdocs_site;
use crate::gdocs_site::DocData;
use crate::gdoc_to_html;
use std::fs;
use std::path::Path;
use anyhow::Result;
use anyhow::Context;
use anyhow::anyhow;
use rayon::prelude::*;
use crate::gdoc_to_html::ImageReference;

pub fn publish(config: &config::Config, store: bool, all: bool) -> Result<()> {

    //----- Read ToC

    println!("Loading tabe of contents.");

    let csv_text = reqwest::blocking::get(&config.toc_spreadsheet_url)?
        .text()
        .context("Failed to download ToC spreadsheet")?;

    let mut docs = DocData::read_csv(csv_text.as_bytes())
        .context("Problem reading ToC spreadsheet")?;

    for doc in &mut docs {
        if doc.author.is_none() {
            doc.author = config.default_author.clone();
        }
    }

    if store {
        let toc_path = &config.download_dir.join("pages.csv");
        fs::write(&toc_path, &csv_text)
            .with_context(|| format!("Failed to write ToC spreadsheet {:?}", &toc_path))?;
    }

    //----- Create GDocs API client

    // Build a tokio runtime to call GDoc API async functions.
    // Use the default multi-threaded runtime.
    let rt = tokio::runtime::Runtime::new()?;

    let config_path = &config.credentials.as_ref().ok_or(anyhow!("Missing credentials in config"))?;

    let gdocs_api = rt.block_on(create_gdocs_client(config_path))?;

    //----- Publish docs

    docs.par_iter()
        .map(|site_doc| {
            if !site_doc.publish && !all {
                println!("Skipping '{}' (not published)", site_doc.slug);
                return Ok(());
            }

            let url = site_doc.gdoc_url.as_ref()
                .ok_or_else(|| anyhow!("{} - No GDoc URL in table of contents", site_doc.slug))?;

            let doc_id = gdocs_site::get_doc_id(url)
                .ok_or_else(|| anyhow!("{} - URL is not a GDoc: {}", site_doc.slug, url))?;

            let gdoc = rt.block_on(gdocs_api.documents().get(doc_id).doit())
                .with_context(|| format!("{} - Failed to load document.", site_doc.slug))?
                .1;

            // Build the pipeline from end to start

            let mut result = Vec::<u8>::new();

            let out = crate::html::HtmlSerializer::new(&mut result);
            //let out = crate::html::DevNull;

            let out = gdoc_to_html::ImageRewriter::new(|img| download_image(
                img,
                &site_doc.slug,
                &config.hugo_site_dir,
                store.then(|| config.download_dir.as_path())
            ), out);

            let html = gdoc_to_html::render(&gdoc, out);

            if store {
                let json_path = config.download_dir
                    .join(&site_doc.slug[1..])
                    .with_extension(".json");

                fs::create_dir_all(json_path.parent().unwrap())?;
                serde_json::to_writer(fs::File::create(json_path)?, &gdoc)?;
            }

            Ok(())

        })
        .collect::<anyhow::Result<Vec<()>>>()?;

    Ok(())
}

pub fn download_image(img_ref: &ImageReference, slug: &str, download_image: impl AsRef<Path>, store_path: Option<&Path>) -> String {

    // if let Some(path) = store_path {
    //     let json_path = config.download_dir
    //         .join(&site_doc.slug[1..])
    //         .with_extension(".json");
    // }

    img_ref.url.to_string()

}


pub async fn create_gdocs_client(creds_path: impl AsRef<Path>) -> Result<google_docs1::Docs> {

    let creds = google_docs1::oauth2::read_service_account_key(creds_path).await?;

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
        .await?;

    let gdocs_api = google_docs1::Docs::new(client.clone(), auth);

    Ok(gdocs_api)

}

// fn foo() {
//     let rt = tokio::runtime::Builder::new_current_thread()
//         .enable_all()
//         .build()?;
//     rt.block_on(gdocs_api::download())?;
//
// }


