//! 12-factor configuration: everything comes from environment variables.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    pub poll_interval_secs: u64,
    pub history_interval_secs: i64,
    pub history_retention_days: i64,
    pub events_retention_days: i64,
    pub access_token_ttl_secs: i64,
    pub refresh_token_ttl_days: i64,
    pub timezone: chrono_tz::Tz,
    pub jwt_secret: Option<String>,
    pub admin_user: Option<String>,
    pub admin_password: Option<String>,
    /// Airzone Cloud base URL (used only for the optional import / synchronisation).
    pub airzone_cloud_url: String,
    /// Airzone Cloud control API (schedules). Default: prodctrl.airzonecloud.com for the real
    /// cloud, else the same address as `airzone_cloud_url` (fake cloud).
    pub airzone_cloud_ctrl_url: Option<String>,
    /// Subnet scanned to find WebServers (default: the server's /24).
    pub scan_subnet: Option<String>,
    /// Port of the Airzone local API.
    pub scan_port: u16,
    /// Optional bootstrap list: "Maison=192.168.1.25,Bureau=192.168.1.40:3000".
    pub airzone_hosts: Vec<BootstrapInstallation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapInstallation {
    pub name: String,
    pub host: String,
    pub port: u16,
}

fn var(name: &str) -> Option<String> {
    // The project was called FreeZone before: its FREEZONE_* variables still work.
    let legacy = name.replacen("BREEZONE_", "FREEZONE_", 1);
    let value = std::env::var(name).or_else(|_| std::env::var(&legacy)).ok();
    value.filter(|v| !v.trim().is_empty())
}

fn num<T: std::str::FromStr>(name: &str, default: T) -> T {
    var(name).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Self {
        let timezone = var("BREEZONE_TZ")
            .or_else(|| var("TZ"))
            .and_then(|tz| tz.parse().ok())
            .unwrap_or(chrono_tz::Europe::Paris);
        Self {
            bind: var("BREEZONE_BIND").unwrap_or_else(|| "0.0.0.0".into()),
            port: num("BREEZONE_PORT", 8080),
            data_dir: var("BREEZONE_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| "data".into()),
            web_dir: var("BREEZONE_WEB_DIR").map(PathBuf::from).unwrap_or_else(|| "web".into()),
            poll_interval_secs: num("BREEZONE_POLL_INTERVAL_SECS", 15).max(2),
            history_interval_secs: num("BREEZONE_HISTORY_INTERVAL_SECS", 300).max(30),
            history_retention_days: num("BREEZONE_HISTORY_RETENTION_DAYS", 90),
            events_retention_days: num("BREEZONE_EVENTS_RETENTION_DAYS", 30),
            access_token_ttl_secs: num("BREEZONE_ACCESS_TOKEN_TTL_SECS", 900),
            refresh_token_ttl_days: num("BREEZONE_REFRESH_TOKEN_TTL_DAYS", 365),
            timezone,
            jwt_secret: var("BREEZONE_JWT_SECRET"),
            airzone_cloud_url: var("BREEZONE_AIRZONE_CLOUD_URL").unwrap_or_else(|| "https://m.airzonecloud.com".into()),
            airzone_cloud_ctrl_url: var("BREEZONE_AIRZONE_CLOUD_CTRL_URL"),
            scan_subnet: var("BREEZONE_SCAN_SUBNET"),
            scan_port: num("BREEZONE_SCAN_PORT", 3000),
            admin_user: var("BREEZONE_ADMIN_USER"),
            admin_password: var("BREEZONE_ADMIN_PASSWORD"),
            airzone_hosts: var("BREEZONE_AIRZONE_HOSTS")
                .map(|v| parse_bootstrap(&v))
                .unwrap_or_default(),
        }
    }

    /// Defaults suited to tests (no env lookups).
    pub fn cloud_ctrl_url(&self) -> String {
        match &self.airzone_cloud_ctrl_url {
            Some(u) => u.clone(),
            None if self.airzone_cloud_url.contains("m.airzonecloud.com") => "https://prodctrl.airzonecloud.com".into(),
            None => self.airzone_cloud_url.clone(),
        }
    }

    pub fn for_tests(data_dir: PathBuf) -> Self {
        Self {
            bind: "127.0.0.1".into(),
            port: 0,
            data_dir,
            web_dir: "web".into(),
            poll_interval_secs: 3600,
            history_interval_secs: 300,
            history_retention_days: 90,
            events_retention_days: 30,
            access_token_ttl_secs: 900,
            refresh_token_ttl_days: 365,
            timezone: chrono_tz::Europe::Paris,
            jwt_secret: Some("test-secret-test-secret-test-secret".into()),
            airzone_cloud_url: "http://127.0.0.1:9".into(),
            airzone_cloud_ctrl_url: None,
            scan_subnet: Some("127.0.0.0/30".into()),
            scan_port: 3000,
            admin_user: None,
            admin_password: None,
            airzone_hosts: vec![],
        }
    }
}

/// Parses "Name=host[:port],host2" into bootstrap installations.
pub fn parse_bootstrap(value: &str) -> Vec<BootstrapInstallation> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .enumerate()
        .map(|(i, item)| {
            let (name, addr) = match item.split_once('=') {
                Some((n, a)) => (n.trim().to_string(), a.trim()),
                None => (format!("Installation {}", i + 1), item),
            };
            let (host, port) = match addr.rsplit_once(':') {
                Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_string(), p.parse().unwrap()),
                _ => (addr.to_string(), 3000),
            };
            BootstrapInstallation { name, host, port }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bootstrap_hosts() {
        let v = parse_bootstrap("Maison=192.168.1.25, 192.168.1.40:3001");
        assert_eq!(
            v,
            vec![
                BootstrapInstallation { name: "Maison".into(), host: "192.168.1.25".into(), port: 3000 },
                BootstrapInstallation { name: "Installation 2".into(), host: "192.168.1.40".into(), port: 3001 },
            ]
        );
    }
}
