//! Configuration file (`/etc/wallmon/config.toml`). Everything specific to one server lives here.

use std::path::Path;

use serde::Deserialize;

pub const DEFAULT_PATH: &str = "/etc/wallmon/config.toml";

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub general: General,
    pub network: Network,
    pub apache: Option<Apache>,
    pub docker: Option<Docker>,
    pub ups: Option<Ups>,
    pub mariadb: Option<MariaDb>,
    pub postgresql: Option<PostgreSql>,
    pub sites: Vec<Site>,
    pub services: Option<Services>,
    pub fail2ban: Option<Fail2ban>,
    pub sensors: Sensors,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    /// Name shown in the header instead of the system hostname.
    pub name: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Network {
    pub interfaces: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apache {
    #[serde(default = "default_apache_status_url")]
    pub status_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Docker {
    #[serde(default = "default_docker_socket")]
    pub socket: String,
    /// Containers that must be running; a missing one is shown in red.
    #[serde(default)]
    pub expected: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ups {
    /// apcupsd NIS address.
    #[serde(default = "default_ups_address")]
    pub address: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MariaDb {
    #[serde(default = "default_mariadb_socket")]
    pub socket: String,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgreSql {}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub url: String,
    /// Accepted HTTP codes. Empty means 200–399.
    #[serde(default)]
    pub ok: Vec<u16>,
}

impl Site {
    /// Short name for the screen: the URL without scheme and trailing slash.
    pub fn name(&self) -> &str {
        let s = self.url.split_once("://").map_or(self.url.as_str(), |(_, rest)| rest);
        s.trim_end_matches('/')
    }

    pub fn accepts(&self, code: u16) -> bool {
        if self.ok.is_empty() { (200..400).contains(&code) } else { self.ok.contains(&code) }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Services {
    pub units: Vec<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fail2ban {
    /// Jails to show, in this order. Empty means all jails.
    pub jails: Vec<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sensors {
    /// Sensors to skip, as `chip/sensor`, e.g. `dell_smm/temp8`.
    pub ignore: Vec<String>,
}

fn default_apache_status_url() -> String {
    "http://127.0.0.1/server-status?auto".into()
}

fn default_docker_socket() -> String {
    "/var/run/docker.sock".into()
}

fn default_ups_address() -> String {
    "127.0.0.1:3551".into()
}

fn default_mariadb_socket() -> String {
    "/run/mysqld/mysqld.sock".into()
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Configuration used by `--demo` when no file is given.
    pub fn demo() -> Self {
        let site = |url: &str| Site { url: url.into(), ok: vec![] };
        Config {
            general: General { name: Some("demo".into()) },
            network: Network { interfaces: vec!["eth0".into()] },
            apache: Some(Apache { status_url: default_apache_status_url() }),
            docker: Some(Docker { socket: default_docker_socket(), expected: vec!["app".into(), "shop".into()] }),
            ups: Some(Ups { address: default_ups_address() }),
            mariadb: Some(MariaDb { socket: default_mariadb_socket() }),
            postgresql: Some(PostgreSql {}),
            sites: ["https://example.com/", "https://shop.example.com/", "https://blog.example.org/",
                    "https://status.example.net/", "https://api.example.com/"]
                .into_iter()
                .map(site)
                .collect(),
            services: Some(Services {
                units: ["apache2", "mariadb", "postgresql", "docker", "fail2ban", "apcupsd", "ssh", "cron"]
                    .map(String::from)
                    .to_vec(),
            }),
            fail2ban: Some(Fail2ban {
                jails: ["sshd", "apache-auth", "apache-badbots", "apache-noscript"].map(String::from).to_vec(),
            }),
            sensors: Sensors::default(),
        }
    }
}
