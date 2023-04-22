#[test]
fn test_render_index_page() -> anyhow::Result<()> {

    let json = std::fs::read_to_string("tests/data/gdoc/index.json")?;
    let doc = serde_json::from_str(&json)?;
    let html = gdocs2hugo::gdoc_to_html::render(&doc)?;

    insta::assert_snapshot!("index_page", html);

    assert_eq!(0, 0);

    Ok(())
}

