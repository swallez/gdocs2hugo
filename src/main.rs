use config::Commands::*;
use config::*;
use gdocs2hugo::*;

fn main() -> anyhow::Result<()> {
    let args = RootCommand::read();
    let config = Config::read(&args.config)?;

    // The thread pool size is roughly equivalent to the number of concurrent http requests
    // since most of the time should be on blocking http request rather than actual CPU
    // utilization.
    // Rayon makes parallel computation so easy that it's not worth using async requests and joining
    // futures for a CLI tool like this one.
    rayon::ThreadPoolBuilder::new()
        .num_threads(config.concurrency.unwrap_or(20))
        .build()?
        .install(|| main0(args, config))
}

fn main0(args: RootCommand, config: Config) -> anyhow::Result<()> {
    match args.command {
        Download { all } => {
            download::download(&config.toc_spreadsheet_url, &config.download_dir, all)?;
        }
        Publish { download, all } => {
            if download {
                download::download(&config.toc_spreadsheet_url, &config.download_dir, all)?;
            }
            publish::publish(&config.download_dir, &config.hugo_site_dir, config.default_author, all)?;
        }
    }

    Ok(())
}
