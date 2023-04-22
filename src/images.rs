use std::fs;
use std::path::Path;
use anyhow::Result;
use crate::publish::HyperC;

/// Download an image to a base path (relative path without extension) and returns the extension
/// that was chosen according to the mime-type.
///
/// Images are stored in `dir`. Large PNGs are compressed to JPG, and `handle_raw` is called
/// with the original image and extension, e.g. to keep it somewhere.
/// 
pub fn download_and_store(
    gdocs_api: &google_docs1::Docs<HyperC>,
        url: &str,
        base_path: impl AsRef<Path>,
        handle_raw: impl FnOnce(&bytes::Bytes, &str)
    ) -> Result<&'static str> {

    //FIXME: only download if there's no file starting with `base_path`
    let base_path = base_path.as_ref();

    let rt = tokio::runtime::Handle::current();
    let _guard = rt.enter();

    let (mut extension, mut bytes) = rt.block_on(
        crate::publish::download_url(gdocs_api, url)
    )?;

    fs::create_dir_all(base_path.parent().unwrap())?;

    // Copying an image in a browser window on MacOS results in a TIFF image that gdocs republishes
    // as a big PNG image. Let's compress to JPEG all PNG images greater than 256 kB.
    if extension == "png" && bytes.len() > 256 * 1024 {
        // Call back to maybe keep the original
        handle_raw(&bytes, extension);
        bytes = compress_png(bytes)?;
        extension = "jpg";
    }

    let image_path = base_path.with_extension(extension);
    fs::write(&image_path, &bytes)?;

    println!("Downloaded image to {:?}", image_path);

    Ok(extension)
}

fn compress_png(bytes: bytes::Bytes) -> anyhow::Result<bytes::Bytes> {
    let cursor = std::io::Cursor::new(&bytes);
    let img = image::load(cursor, image::ImageFormat::Png)?;

    let mut result: Vec<u8> = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut result, 75);
    encoder.encode_image(&img)?;

    println!("Compressed {} kB to {} kB", bytes.len() / 1024, result.len() / 1024);

    Ok(bytes::Bytes::from(result))
}
