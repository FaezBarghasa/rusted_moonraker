use std::path::{Path, PathBuf};
use ini::Ini;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub ssl_enabled: bool,
    pub max_upload_size_mb: u64,
    pub trusted_clients: Vec<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KlippyConfig {
    pub uds_path: PathBuf,
    pub api_timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub db_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct MoonrakerConfig {
    pub server: ServerConfig,
    pub klippy: KlippyConfig,
    pub database: DatabaseConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("INI error: {0}")]
    Ini(#[from] ini::Error),

    #[error("INI parse error: {0}")]
    IniParse(#[from] ini::ParseError),

    #[error("Validation error at {section}.{key}: {message}")]
    Validation {
        section: String,
        key: String,
        message: String,
    },
}

fn resolve_path(path_str: &str) -> PathBuf {
    if path_str.starts_with("~/") {
        if let Some(mut home) = dirs::home_dir() {
            home.push(&path_str[2..]);
            home
        } else {
            PathBuf::from(path_str)
        }
    } else {
        PathBuf::from(path_str)
    }
}

impl MoonrakerConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let ini = Ini::load_from_file(path)?;
        Self::load_from_ini(ini)
    }

    pub fn load_from_str(content: &str) -> Result<Self, ConfigError> {
        let ini = Ini::load_from_str(content)?;
        Self::load_from_ini(ini)
    }

    fn load_from_ini(ini: Ini) -> Result<Self, ConfigError> {
        // [server] parsing
        let server_section = ini.section(Some("server"));
        let host = server_section
            .and_then(|s| s.get("host"))
            .map(|h| h.to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string());

        let raw_port = server_section
            .and_then(|s| s.get("port"))
            .and_then(|p| p.parse::<u32>().ok());

        let port = match raw_port {
            Some(p) => {
                if (1024..=65535).contains(&p) {
                    p as u16
                } else {
                    7125
                }
            }
            None => 7125,
        };

        let ssl_enabled = server_section
            .and_then(|s| s.get("ssl_enabled"))
            .and_then(|ssl| ssl.parse::<bool>().ok())
            .unwrap_or(false);

        let max_upload_size_mb = server_section
            .and_then(|s| s.get("max_upload_size"))
            .or_else(|| server_section.and_then(|s| s.get("max_upload_size_mb")))
            .and_then(|sz| sz.parse::<u64>().ok())
            .unwrap_or(1024);

        // [klippy] parsing
        let klippy_section = ini.section(Some("klippy"));
        let uds_path_str = klippy_section
            .and_then(|s| s.get("uds_address"))
            .or_else(|| klippy_section.and_then(|s| s.get("uds_path")))
            .unwrap_or("/tmp/klippy_uds");
        let uds_path = resolve_path(uds_path_str);

        let api_timeout_secs = klippy_section
            .and_then(|s| s.get("api_timeout_secs"))
            .or_else(|| klippy_section.and_then(|s| s.get("api_timeout")))
            .and_then(|t| t.parse::<u64>().ok())
            .unwrap_or(30);

        // [database] parsing
        let db_section = ini.section(Some("database"));
        let db_path_str = db_section
            .and_then(|s| s.get("database_path"))
            .or_else(|| db_section.and_then(|s| s.get("db_path")))
            .unwrap_or("~/.printer_data/database");
        let db_path = resolve_path(db_path_str);

        let trusted_clients_str = server_section
            .and_then(|s| s.get("trusted_clients"))
            .unwrap_or("127.0.0.1/32");
        let trusted_clients = trusted_clients_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<_>>();

        let api_key = server_section
            .and_then(|s| s.get("api_key"))
            .map(|k| k.to_string());

        Ok(MoonrakerConfig {
            server: ServerConfig {
                host,
                port,
                ssl_enabled,
                max_upload_size_mb,
                trusted_clients,
                api_key,
            },
            klippy: KlippyConfig {
                uds_path,
                api_timeout_secs,
            },
            database: DatabaseConfig { db_path },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_parsing_defaults() {
        let conf_str = "";
        let config = MoonrakerConfig::load_from_str(conf_str).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 7125);
        assert_eq!(config.klippy.uds_path, PathBuf::from("/tmp/klippy_uds"));
    }

    #[test]
    fn test_config_parsing_custom() {
        let conf_str = r#"
[server]
host = 10.0.0.5
port = 8888
max_upload_size = 500

[klippy]
uds_address = /var/run/klippy

[database]
database_path = /var/lib/moonraker/db
"#;
        let config = MoonrakerConfig::load_from_str(conf_str).unwrap();
        assert_eq!(config.server.host, "10.0.0.5");
        assert_eq!(config.server.port, 8888);
        assert_eq!(config.server.max_upload_size_mb, 500);
        assert_eq!(config.klippy.uds_path, PathBuf::from("/var/run/klippy"));
        assert_eq!(config.database.db_path, PathBuf::from("/var/lib/moonraker/db"));
    }

    #[test]
    fn test_config_port_fallback() {
        let conf_str = r#"
[server]
port = 80
"#;
        let config = MoonrakerConfig::load_from_str(conf_str).unwrap();
        // 80 is below 1024, should fallback to 7125
        assert_eq!(config.server.port, 7125);

        let conf_str_high = r#"
[server]
port = 70000
"#;
        let config_high = MoonrakerConfig::load_from_str(conf_str_high).unwrap();
        assert_eq!(config_high.server.port, 7125);
    }
}
