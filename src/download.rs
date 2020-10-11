use std::path::Path;

use anyhow::Context;
use rayon::prelude::*;
use std::fs;

use crate::DocData;

pub fn download(toc_url: &str, download_dir: &Path, all: bool) -> anyhow::Result<()> {
    fs::create_dir_all(download_dir).with_context(|| format!("Cannot create directory {:?}", download_dir))?;

    let csvtext = reqwest::blocking::get(toc_url)?
        .text()
        .context("Failed to download ToC spreadsheet")?;

    let toc_path = download_dir.join("pages.csv");
    fs::write(&toc_path, &csvtext).with_context(|| format!("Failed to write ToC spreadsheet {:?}", &toc_path))?;

    let docs = DocData::read_csv(csvtext.as_bytes())?;

    docs.par_iter()
        .map(|doc| download_doc(&doc, download_dir, all))
        .collect::<anyhow::Result<Vec<()>>>()?; // Report any error downloading docs

    Ok(())
}

fn download_doc(doc: &DocData, download_dir: &Path, all: bool) -> anyhow::Result<()> {
    if !doc.publish && !all {
        println!("Skipping '{}' (not published)", doc.slug);
        return Ok(());
    }

    println!("Fetching '{}'", doc.slug);

    let content = reqwest::blocking::get(&format!("{}?embedded=true", &doc.gdoc_pub_url))
        .with_context(|| format!("Failed to download {}", doc.slug))?
        .text()?;

    let doc_path = download_dir.join(format!("{}.html", doc.slug));
    fs::write(doc_path, content)?;

    Ok(())
}
