use std::fs;
use std::path::Path;
use crate::gdoc_to_html::ImageReference;
use anyhow::Result;

pub fn import_image2(
        img_ref: &ImageReference,
        slug: &str,
        hugo_dir: impl AsRef<Path>,
        store_dir: Option<&Path>
    ) -> Result<String> {

    let hugo_dir = hugo_dir.as_ref();

    let url = img_ref.url;
    if !img_ref.url.contains("googleusercontent.com") {
        println!("{} - Found a regular image link (!?): {}", slug, url);
        return Ok(url.to_string());
    }

    // Have we already processed this image?
    // Note: we previously used the globwalk crate here, but it's overkill
    let dir_path = hugo_dir
        .join("static/post-images")
        .join(&slug[1..]);

    if dir_path.is_dir() {
        for entry in fs::read_dir(&dir_path)? {
            let path = entry?.path();
            let file_name = path
                .file_name().unwrap()
                .to_string_lossy();

            if file_name.starts_with(img_ref.image_id) {
                return Ok(format!("/post-images{}/{}", slug, file_name))
            }
        }
    }

    let base_path = dir_path.join(img_ref.image_id);
    let extension = download_and_store(url, base_path, |_bytes, _ext| {
        if let Some(_store_dir) = store_dir {
            // TODO: store raw image
        }
    })?;

    Ok(format!("/post-images{}/{}.{}", slug, img_ref.image_id, extension))

}


pub fn import_image(url: &str, hugo_dir: &Path) -> anyhow::Result<String> {
    // gdocs image URLs are like https://lh3.googleusercontent.com/<some long id>

    if !url.contains("googleusercontent.com") {
        println!("Found a regular image link (!?): {}", url);
        return Ok(url.to_string());
    }

    let name = url.rsplit('/').next().unwrap();

    // Have we already processed this image?
    // Note: we previously used the globwalk crate here, but it's overkill
    let dir_path = hugo_dir.join(format!("static/post-images/{}", name));
    if dir_path.is_dir() {
        for entry in fs::read_dir(&dir_path)? {
            let path = entry?.path();
            let file_name = path
                .file_name().unwrap()
                .to_string_lossy();

            if file_name.starts_with("image.") {
                return Ok(format!("/post-images/{}/{}", name, file_name))
            }
        }
    }

    // New image: download it
    let resp = reqwest::blocking::get(url)?;

    let mut extension = {
        let mime_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        let extension = mime_guess::get_mime_extensions_str(mime_type).unwrap()[0];
        if extension == "jpe" {
            // jpe is the first extension listed for jpeg. Although it's a valid extension, not all
            // tools recognize it.
            "jpg"
        } else {
            extension
        }
    };

    let mut bytes = resp.bytes()?;

    // Copying an image in a browser window on MacOS results in a TIFF image that gdocs republishes
    // as a big PNG image. Let's compress to JPEG all PNG images greater than 256 kB.
    if extension == "png" && bytes.len() > 256 * 1024 {
        bytes = compress_png(bytes)?;
        extension = "jpg";
    }

    fs::create_dir_all(dir_path)?;
    let result = format!("/post-images/{}/image.{}", name, extension);

    let image_path = hugo_dir.join(format!("static{}", result));
    println!("Downloaded image to {:?}", image_path);
    fs::write(image_path, bytes)?;

    Ok(result)
}

/// Download an image to a base path (relative path without extension) and returns the extension
/// that was chosen according to the mime-type.
///
/// Images are stored in `dir`. Large PNGs are compressed to JPG, and `handle_raw` is called
/// with the original image and extension, e.g. to keep it somewhere.
fn download_and_store(
        url: &str,
        base_path: impl AsRef<Path>,
        handle_raw: impl FnOnce(&bytes::Bytes, &str)
    ) -> Result<&'static str> {
    let base_path = base_path.as_ref();

    let resp = reqwest::blocking::get(url)?;

    let mut extension = {
        let mime_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        let extension = mime_guess::get_mime_extensions_str(mime_type).unwrap()[0];
        if extension == "jpe" {
            // jpe is the first extension listed for jpeg. Although it's a valid extension, not all
            // tools recognize it.
            "jpg"
        } else {
            extension
        }
    };

    let mut bytes = resp.bytes()?;

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
