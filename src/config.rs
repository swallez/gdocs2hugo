use anyhow::Context;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use clap::Parser;
use serde::Deserialize;

//----- Command line parameters

/// From Google Docs to Hugo
#[derive(Parser, Debug)]
pub struct RootCommand {
    /// Path to the config file
    #[arg(global = true, long, default_value = "gdocs2hugo.yml")]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

impl RootCommand {
    // Avoids importing Clap in main and solves some IntelliJ type inference issue
    pub fn read() -> RootCommand {
        RootCommand::parse()
    }
}

#[derive(Parser, Debug)]
pub enum Commands {
    /// Download gdocs content
    Download {
        /// Download all pages (ignore publication status)
        #[arg(long)]
        all: bool,
    },

    /// Publish downloaded gdocs content to the Hugo content dir
    Publish {
        /// Download gdocs content before publishing
        #[arg(long)]
        download: bool,
        /// Publish all pages (ignore publication status)
        #[arg(long)]
        all: bool,
    },
    Gdoc,

    /// Publish downloaded gdocs content to the Hugo content dir
    Publish2 {
        /// Keep a copy of json docs and raw images
        #[arg(long)]
        store: bool,
        /// Publish all pages (ignore publication status)
        #[arg(long)]
        all: bool,
    },
}

//----- Config file

#[derive(Deserialize, Debug)]
pub struct Config {
    pub toc_spreadsheet_url: String,
    #[serde(default = "default_download_dir")]
    pub download_dir: PathBuf,
    pub hugo_site_dir: PathBuf,
    pub credentials: Option<PathBuf>,
    pub concurrency: Option<usize>,
    pub default_author: Option<String>,
    /// Domain names whose links must not get a `target` attribute
    #[serde(default)]
    pub no_target: Vec<String>,
}

fn default_download_dir() -> PathBuf {
    "gdoc_data".into()
}

impl Config {
    pub fn read(path: &Path) -> anyhow::Result<Config> {
        let file = File::open(path).with_context(|| format!("Failed to open {:?}", path))?;
        let mut config: Config = serde_yaml::from_reader(file).with_context(|| format!("Failed to read {:?}", path))?;
        // Paths in the config are relative to the config file location.
        let config_dir = path.parent().unwrap();
        config.download_dir = config_dir.join(config.download_dir);
        config.hugo_site_dir = config_dir.join(config.hugo_site_dir);
        Ok(config)
    }
}
