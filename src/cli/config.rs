use std::path::PathBuf;

use config::{Config, ConfigError};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PixhawkConfig {
    pub address: String,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub address: String,
}

#[derive(Debug, Deserialize)]
pub struct PlaneSystemConfig {
    pub pixhawk: PixhawkConfig,
    pub server: ServerConfig,
}

impl PlaneSystemConfig {
    pub fn read() -> Result<Self, ConfigError> {
        let mut c = Config::new();

        c.merge(config::File::with_name("plane-system"))?;
        c.merge(config::Environment::with_prefix("PLANE_SYSTEM"))?;

        c.try_into()
    }

    pub fn read_from_path(path: PathBuf) -> Result<Self, ConfigError> {
        let mut c = Config::new();

        c.merge(config::File::from(path))?;
        c.merge(config::Environment::with_prefix("PLANE_SYSTEM"))?;

        c.try_into()
    }
}