use config::Commands::*;
use config::*;
use gdocs2hugo::*;

fn main() -> anyhow::Result<()> {
    unsafe { backtrace_on_stack_overflow::enable() };
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
        Download { all: _ } => {
            println!("Please use publish2");
            // let docs = gdocs_site::download_toc(&config.toc_spreadsheet_url, &config.download_dir)?;
            // gdocs_site::download_html_docs(&docs, &config.download_dir, all)?;
        }
        Publish { download: _, all: _ } => {
            println!("Please use publish2");
            // if download {
            //     let docs = gdocs_site::download_toc(&config.toc_spreadsheet_url, &config.download_dir)?;
            //     gdocs_site::download_html_docs(&docs, &config.download_dir, all)?;
            // }
            // from_web_pub::publish::publish(&config.download_dir, &config.hugo_site_dir, config.default_author, all)?;
        }

        // Test
        Gdoc => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(experiments::gdocs_api::_download())?;
        }

        Publish2 { store, all } => {
            publish::publish(&config, store, all)?;
        }
    }

    Ok(())
}
