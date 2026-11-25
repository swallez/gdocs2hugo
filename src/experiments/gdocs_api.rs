use std::fs::File;
use http_body_util::BodyExt;
use crate::publish::{create_gdocs_client, create_gdrive_client};

const CREDENTIALS: &str = "../credentials-gdocs-cms.json";

pub async fn _download() -> anyhow::Result<()> {
    _download_doc().await
}

pub async fn _download_sheet() -> anyhow::Result<()> {
    let hub = create_gdrive_client(CREDENTIALS).await?;

    let sheet = hub.files().export("1ZDNb8kcWRZf8Arw6D3NKksDlQVWb_7PEnvyHJ85YBNA", "text/csv")
        .param("gid", "26826342")
        .doit()
        .await?;

    let sheet = sheet.into_body().collect().await?.to_bytes();
    let sheet = String::from_utf8_lossy(sheet.as_ref());
    println!("{}", sheet);

    Ok(())
}

pub async fn _download1() -> anyhow::Result<()> {
    let hub = create_gdrive_client(CREDENTIALS).await?;

    let files = hub.files().list()
        //.corpora("drive")
        //.drive_id("1NvDFyAcVY-0HClOKd6IrKw6fRmtarJJe")
        .q("'1NvDFyAcVY-0HClOKd6IrKw6fRmtarJJe' in parents")
        //.include_items_from_all_drives(true)
        //.supports_all_drives(true)
        .doit().await?;

    let files : google_drive3::api::FileList = files.1;

    println!("{}", serde_json::to_string(&files)?);

    Ok(())
}

pub async fn _download_doc() -> anyhow::Result<()> {
    let hub = create_gdocs_client(CREDENTIALS).await?;

    let doc = hub.documents().get("1X3gv_DywN_u2yUzbOEmvWrUSGSdveGGDaGxuPmDyXus").doit().await?;

    let doc : google_docs1::api::Document = doc.1;

    let file = File::create("test-doc.json")?;
    serde_json::to_writer_pretty(file, &doc)?;

    Ok(())
}
