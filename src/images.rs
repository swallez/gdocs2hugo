use std::fs;
use std::path::Path;

pub fn import_image(url: &str, hugo_dir: &Path) -> anyhow::Result<String> {
    // gdocs image URLs are like https://lh3.googleusercontent.com/<some long id>

    if !url.contains("googleusercontent.com") {
        print!("Found a regular image link (!?): {}", url);
        return Ok(url.to_string());
    }

    let name = url.rsplit('/').next().unwrap();

    let dir_path = hugo_dir.join(format!("static/post-images/{}", name));
    if dir_path.exists() {
        let walker = globwalk::GlobWalkerBuilder::new(dir_path, "image.*").build()?;
        let first_file = walker.into_iter().next().unwrap()?;
        let image_name = first_file.file_name().to_str().unwrap();

        return Ok(format!("/post-images/{}/{}", name, image_name));
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

fn compress_png(bytes: bytes::Bytes) -> anyhow::Result<bytes::Bytes> {
    let cursor = std::io::Cursor::new(&bytes);
    let img = image::load(cursor, image::ImageFormat::Png)?;

    let mut result: Vec<u8> = Vec::new();
    let mut encoder = image::jpeg::JpegEncoder::new_with_quality(&mut result, 75);
    encoder.encode_image(&img)?;

    println!("Compressed {} kB to {} kB", bytes.len() / 1024, result.len() / 1024);

    Ok(bytes::Bytes::from(result))
}
