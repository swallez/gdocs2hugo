use chrono::TimeZone;
use chrono::Utc;
use gdocs2hugo::SiteData;
use gdocs2hugo::gdocs_site::DateTimeWithDefault;
use gdocs2hugo::gdocs_site::DocData;
use std::path::Path;
use gdocs2hugo::from_web_pub::publish;

/// Generate a test page and compare the result with the expected version.
///
/// The generated file is left on disk to allow further inspection in case of test failure.
///
#[test]
fn publish_test() {
    /// Read a generated page file and return its yaml front-matter and html parts.
    fn read_fm_and_content(path: &str) -> (serde_yaml::Value, scraper::Html) {
        let content = std::fs::read_to_string(Path::new(path)).unwrap();

        let mut parts = content.splitn(3, "---\n");
        parts.next(); // Skip initial empty block

        let yaml = serde_yaml::from_str(parts.next().unwrap()).unwrap();
        let html = scraper::Html::parse_document(parts.next().unwrap());
        (yaml, html)
    }

    let record = DocData {
        publish: true,
        slug: "test-doc".into(),
        category: Some("category-1".into()),
        author: Some("John Doe".into()),
        publish_date: DateTimeWithDefault(
            Utc.datetime_from_str("24/09/2020 11:12:13", "%d/%m/%Y %H:%M:%S").unwrap()
        ),
        weight: None,
        update_date: None,
        gdoc_pub_url: "https://docs.google.com/document/d/e/2PACX-1vR6BpAlYnSiCdEELnNtnnK0rejYCDpn6rX-jumwZ9zQbacHiO3TC6Uq6KbC0vhet1Brw2f9Udk6qWM6/pub".into(),
        gdoc_url: None,
        download_path: Default::default()
    };

    // Publish this doc
    publish::publish_doc(
        record,
        Path::new("tests/data/gdoc"),
        Path::new("tests/data/site"),
        &SiteData::default(),
        false,
    )
    .unwrap();

    let (expected_yaml, expected_html) = read_fm_and_content("tests/data/site/content/posts/test-doc-reference.html");
    let (actual_yaml, actual_html) = read_fm_and_content("tests/data/site/content/posts/test-doc.html");

    assert_eq!(expected_yaml, actual_yaml);
    assert_eq!(expected_html, actual_html);
}
