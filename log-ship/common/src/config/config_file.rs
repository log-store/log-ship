use std::fs::{create_dir_all, OpenOptions};
use std::path::PathBuf;

use anyhow::bail;
use serde::{Serialize, Deserialize};

use crate::logging::debug;

pub const SAVED_SEARCHES_FILE_NAME: &str = "saved_searches.json";
pub const DASHBOARDS_FILE_NAME: &str = "dashboards.json";


#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum TimestampFormat {
    EPOCH,
    RFC2822,    // Tue, 1 Jul 2003 10:52:37 +0200
    RFC3339,    // 1996-12-19T16:39:57-08:00

    // Unfortunately, not supported by TOML :-(
    // FORMAT(String)
}

// TODO: Wrap in Spanned so we can save comments
// see: https://docs.rs/toml/latest/toml/struct.Spanned.html
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConfigFile {
    // TODO: add hidden debug flag here
    pub log_file: Option<PathBuf>,

    pub data_dir: PathBuf,

    pub retention_days: usize,

    pub unix_socket: Option<PathBuf>,

    pub tcp_input_address: Option<String>,

    pub syslog_address: Option<String>,

    #[serde(default = "default_syslog_protocol")]
    pub syslog_protocol: String,

    #[serde(default = "default_web_address")]
    pub web_address: String,

    #[serde(default = "default_timestamp_field")]
    pub timestamp_field: String,

    #[serde(default = "default_timestamp_format")]
    pub timestamp_format: TimestampFormat,

    /// Can be one of: server, browser, (things like MySQL in the future)
    #[serde(default = "default_save_location")]
    pub save_location: String,

    // #[serde(default)]
    // pub saved_searches: Vec<SavedSearch>,
    //
    // #[serde(default)]
    // pub dashboards: Vec<Dashboard>,
}

// see corresponding struct in types.ts
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SavedSearch {
    pub name: String,
    pub description: Option<String>,
    pub search_query: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Dashboard {
    pub name: String,
    pub description: Option<String>,
    pub widgets: String,
}

pub fn default_syslog_protocol() -> String {
    "tcp".to_string()
}

pub fn default_web_address() -> String {
    "localhost:8181".to_string()
}

/// Default to saving searches and dashboards on the server
pub fn default_save_location() -> String {
    "server".to_string()
}

pub fn default_timestamp_field() -> String {
    "t".to_string()
}

pub fn default_timestamp_format() -> TimestampFormat {
    TimestampFormat::EPOCH
}

/// Creates the default ConfigFile
impl Default for ConfigFile {
    fn default() -> Self {
        ConfigFile {
            log_file: None,
            data_dir: PathBuf::from("/var/lib/log-store"),
            retention_days: 30,
            unix_socket: Some(PathBuf::from("/tmp/log-store.socket")),
            tcp_input_address: Some("0.0.0.0:1234".to_string()),
            syslog_address: None,
            syslog_protocol: default_syslog_protocol(),
            web_address: default_web_address(),
            timestamp_field: default_timestamp_field(),
            timestamp_format: default_timestamp_format(),
            save_location: default_save_location(),
            // saved_searches: Vec::new(),
            // dashboards: Vec::new(),
        }
    }
}

impl ConfigFile {
    pub fn sanity_check(&self) -> anyhow::Result<()> {
        debug!("{:?}", self);

        if !self.data_dir.exists() {
            if let Err(e) = create_dir_all(&self.data_dir) {
                bail!("The data directory ({}) does not exist, and could not be created: {}", self.data_dir.display(), e);
            }
        }

        // make sure the data directory is a directory
        if !self.data_dir.is_dir() {
            bail!("Path to the data directory is not a directory: {}", self.data_dir.display());
        }

        // make sure the retention is not zero
        if self.retention_days == 0 {
            bail!("Cannot set retention_days to zero")
        }

        // make sure the timestamp field

        // make sure saved_dashboard_location is one of server or browser
        match self.save_location.to_ascii_lowercase().as_str() {
            "browser" => (),
            "server" => {
                // ensure we can save to the required files
                for file in &[SAVED_SEARCHES_FILE_NAME, DASHBOARDS_FILE_NAME] {
                    let file_path = self.data_dir.join(file);

                    if let Err(e) = OpenOptions::new().create(true).write(true).truncate(false).open(&file_path) {
                        bail!("Cannot create or open {}; please make sure permissions to that location are setup properly: {}", file_path.display(), e);
                    }
                }
            }
            _ => bail!("The config 'saved_dashboard_location' must be one of the strings: \"server\" or \"browser\"")
        }

        // make sure every saved-search and dashboard has a valid search_query
        // for saved_search in &self.saved_searches {
        //     parse_query(saved_search.search_query.as_str())?;
        // }

        return Ok( () )
    }
}


