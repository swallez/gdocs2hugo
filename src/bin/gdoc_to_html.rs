fn main() -> anyhow::Result<()>{

    let doc = gdocs2hugo::gdoc_to_html::read("test-doc.json")?;

    let mut foo = String::new();

    let html = gdocs2hugo::gdoc_to_html::render(&doc)?;

    println!("{}", html);

    std::fs::write("test-doc.html", html);

    println!("{}", foo);

    Ok(())
}
