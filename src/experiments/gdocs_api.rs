use std::fs::File;

pub async fn _download() -> anyhow::Result<()> {
    _download_doc().await
}

pub async fn _download_sheet() -> anyhow::Result<()> {

    let creds = google_drive3::oauth2::read_service_account_key("../credentials-gdocs-cms.json").await?;

    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    let client = hyper::Client::builder()
        //.pool_max_idle_per_host(0)
        .build::<_, hyper::Body>(connector);

    let auth = google_drive3::oauth2::ServiceAccountAuthenticator::builder(creds)
        .hyper_client(client.clone())
        .build()
        .await?;

    let hub = google_drive3::DriveHub::new(client.clone(), auth);
    let sheet = hub.files().export("1ZDNb8kcWRZf8Arw6D3NKksDlQVWb_7PEnvyHJ85YBNA", "text/csv")
        .param("gid", "26826342")
        .doit()
        .await?;

    let sheet = sheet.into_body();

    let sheet = hyper::body::to_bytes(sheet).await?;
    let sheet = String::from_utf8_lossy(sheet.as_ref());
    println!("{}", sheet);

    Ok(())
}


pub async fn _download1() -> anyhow::Result<()> {

    let creds = google_drive3::oauth2::read_service_account_key("../credentials-gdocs-cms.json").await?;

    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    let client = hyper::Client::builder()
        //.pool_max_idle_per_host(0)
        .build::<_, hyper::Body>(connector);

    let auth = google_drive3::oauth2::ServiceAccountAuthenticator::builder(creds)
        .hyper_client(client.clone())
        .build()
        .await?;

    let hub = google_drive3::DriveHub::new(client.clone(), auth);
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

    let creds = google_docs1::oauth2::read_service_account_key("../credentials-gdocs-cms.json").await?;

    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();

    let client = hyper::Client::builder()
        //.pool_max_idle_per_host(0)
        .build::<_, hyper::Body>(connector);

    let auth = google_docs1::oauth2::ServiceAccountAuthenticator::builder(creds)
        .hyper_client(client.clone())
        .build()
        .await?;

    // // https://developers.google.com/identity/protocols/oauth2/scopes
    // let scopes = &["https://www.googleapis.com/auth/drive.file"];
    // //let scopes = &["https://www.googleapis.com/auth/documents"];
    // //let scopes = &["https://www.googleapis.com/auth/documents.readonly"];
    //
    // let token: yup_oauth2::AccessToken = auth.token(scopes).await?;

    let hub = google_docs1::Docs::new(client.clone(), auth);
    let doc = hub.documents().get("1X3gv_DywN_u2yUzbOEmvWrUSGSdveGGDaGxuPmDyXus").doit().await?;

    let doc : google_docs1::api::Document = doc.1;

    let file = File::create("test-doc.json")?;
    serde_json::to_writer_pretty(file, &doc)?;

    //println!("{}", serde_json::to_string(&doc)?);

    Ok(())
}


#[cfg(test)]
mod tests {
    //#[test]
    #[tokio::test]
    async fn it_works() {
        super::_download().await.unwrap();
        println!("Done");
    }
}

