use anyhow::Context;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use structopt::StructOpt;

//----- Command line parameters

/// From Google Docs to Hugo
#[derive(StructOpt, Debug)]
pub struct RootCommand {
    /// Path to the config file
    #[structopt(global = true, long, default_value = "gdocs2hugo.yml")]
    pub config: PathBuf,

    #[structopt(subcommand)]
    pub command: Commands,
}

impl RootCommand {
    // Avoids importing StructOpt in main and solves some IntelliJ type inference issue
    pub fn read() -> RootCommand {
        RootCommand::from_args()
    }
}

#[derive(StructOpt, Debug)]
pub enum Commands {
    /// Download gdocs content
    Download {
        /// Download all pages (ignore publication status)
        #[structopt(long)]
        all: bool,
    },

    /// Publish downloaded gdocs content to the Hugo content dir
    Publish {
        /// Download gdocs content before publishing
        #[structopt(long)]
        download: bool,
        /// Publish all pages (ignore publication status)
        #[structopt(long)]
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
    pub concurrency: Option<usize>,
    pub default_author: Option<String>,
}

fn default_download_dir() -> PathBuf {
    "gdoc_data".into()
}

impl Config {
    pub fn read(path: &Path) -> anyhow::Result<Config> {
        let file = File::open(path).with_context(|| format!("Failed to open {:?}", path))?;
        let config = serde_yaml::from_reader(file).with_context(|| format!("Failed to read {:?}", path))?;
        Ok(config)
    }
}
