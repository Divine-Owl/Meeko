// ╭──────────────────────────────────────────────────────────────────────────╮
// │   C O N F I G                                                            │
// │   meeko-hookd · env config, loopback pinning, socket path validation     │
// │                                                                          │
// │   spec §6.1 / §6.3 — GPL-3.0-only                                        │
// ╰──────────────────────────────────────────────────────────────────────────╯

use std::env;
use std::path::PathBuf;

/// Hard error on a non-loopback host URL without MEEKO_TEST=1: an env override
/// that silently pointed hookd at a remote host would make it a token-exfil
/// client (CR&S audit item, spec §6.3).
#[derive(Debug)]
pub enum ConfigError {
    NonLoopbackHost(String),
    MissingRuntimeDir,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NonLoopbackHost(u) => {
                write!(
                    f,
                    "refusing non-loopback CODYNC_HOST_URL {u} without MEEKO_TEST=1"
                )
            }
            ConfigError::MissingRuntimeDir => {
                write!(
                    f,
                    "XDG_RUNTIME_DIR is not set and MEEKO_HOOKD_SOCKET was not given"
                )
            }
        }
    }
}

pub struct Config {
    pub socket_path: PathBuf,
    pub host_url: String,
    pub token_path: PathBuf,
    pub shell_bin: String,
    /// Recorded so tests/tools can introspect the guard; the pinning check
    /// itself runs at load time.
    #[allow(dead_code)]
    pub test_mode: bool,
}

fn env_or(key: &str, default: &str) -> String {
    match env::var(key) {
        Ok(v) if !v.is_empty() => v,
        _ => default.to_string(),
    }
}

/// Loopback means literal loopback: 127.0.0.0/8, or ::1.
/// Everything else needs MEEKO_TEST=1. "localhost" is deliberately absent —
/// it is a resolvable hostname, not a literal loopback address (CR&S audit).
fn is_loopback(url: &str) -> bool {
    let host = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let host = host.split('/').next().unwrap_or("");
    // strip port, mindful of [::1]:port
    let host = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    if host == "::1" {
        return true;
    }
    // 127.x.x.x with all-numeric octets only — "127.0.0.1.evil.example" is not.
    let octets: Vec<&str> = host.split('.').collect();
    octets.len() == 4
        && octets[0] == "127"
        && octets
            .iter()
            .all(|o| !o.is_empty() && o.chars().all(|c| c.is_ascii_digit()))
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let test_mode = env::var("MEEKO_TEST").map(|v| v == "1").unwrap_or(false);

        let host_url = env_or("CODYNC_HOST_URL", "http://127.0.0.1:19222");
        if !test_mode && !is_loopback(&host_url) {
            return Err(ConfigError::NonLoopbackHost(host_url));
        }

        let socket_path = match env::var("MEEKO_HOOKD_SOCKET") {
            Ok(p) if !p.is_empty() => PathBuf::from(p),
            _ => {
                let dir =
                    env::var("XDG_RUNTIME_DIR").map_err(|_| ConfigError::MissingRuntimeDir)?;
                PathBuf::from(dir).join("meeko-hookd.sock")
            }
        };

        let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
        let token_path = match env::var("CODYNC_TOKEN_PATH") {
            Ok(p) if !p.is_empty() => PathBuf::from(p),
            _ => PathBuf::from(home).join(".codync").join("token"),
        };

        Ok(Config {
            socket_path,
            host_url,
            token_path,
            shell_bin: env_or("MEEKO_SHELL_BIN", "quickshell"),
            test_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::is_loopback;

    #[test]
    fn loopback_pinning() {
        assert!(is_loopback("http://127.0.0.1:19222"));
        assert!(is_loopback("http://127.5.5.5:1"));
        assert!(is_loopback("http://[::1]:19222"));
        assert!(!is_loopback("http://192.168.1.10:19222"));
        assert!(!is_loopback("http://example.com"));
        assert!(!is_loopback("http://10.0.0.1:19222"));
        // trick cases: 127.0.0.1.evil.example is not loopback
        assert!(!is_loopback("http://127.0.0.1.evil.example"));
    }
}
