use std::path::Path;
use gdocs2hugo::config::Config;

fn main() -> anyhow::Result<()> {

    let config = Config::read(Path::new("../site/site-pro/gdocs2hugo.yml"))?;

    gdocs2hugo::publish::publish(&config, true, false)?;

    Ok(())
}
