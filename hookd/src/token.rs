// ╭──────────────────────────────────────────────────────────────────────────╮
// │   T O K E N                                                              │
// │   meeko-hookd · single-source credential loading (~/.codync/token)       │
// │                                                                          │
// │   spec §6.3: owner uid == current uid, mode & 0o077 == 0, O_NOFOLLOW.    │
// │   The token NEVER crosses the hookd socket, never in argv/env/logs.      │
// │   Re-read on 401 or file mtime change (rotate-aware).                    │
// │   GPL-3.0-only                                                           │
// ╰──────────────────────────────────────────────────────────────────────────╯

use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// Why a token file was refused. Safe to log — never contains file contents.
#[derive(Debug)]
pub enum TokenError {
    Missing,
    Symlink,
    WrongOwner { owner: u32, current: u32 },
    InsecureMode(u32),
    Unreadable(io::Error),
    Empty,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Log the failure reason, never the contents (spec §6.3).
        match self {
            TokenError::Missing => write!(f, "token file missing"),
            TokenError::Symlink => write!(f, "token path is a symlink"),
            TokenError::WrongOwner { owner, current } => {
                write!(f, "token file owned by uid {owner}, running as {current}")
            }
            TokenError::InsecureMode(m) => {
                write!(f, "token file mode {m:#o} is group/other accessible")
            }
            TokenError::Unreadable(e) => write!(f, "token file unreadable: {e}"),
            TokenError::Empty => write!(f, "token file empty"),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Stamp {
    mtime: SystemTime,
    len: u64,
}

pub struct Token {
    path: PathBuf,
    // Cached credential + file stamp; None while degraded (invalid file).
    inner: Mutex<Option<(String, Stamp)>>,
}

impl Token {
    pub fn new(path: PathBuf) -> Self {
        let t = Token {
            path,
            inner: Mutex::new(None),
        };
        t
    }

    /// Validate and read the token file. Any validation failure is a hard
    /// refusal — there is no "relaxed" path, including under test.
    pub fn read_validated(path: &Path) -> Result<(String, Stamp), TokenError> {
        let meta = fs::symlink_metadata(path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                TokenError::Missing
            } else {
                TokenError::Unreadable(e)
            }
        })?;
        if meta.file_type().is_symlink() {
            return Err(TokenError::Symlink);
        }
        let current = unsafe { libc::getuid() };
        if meta.uid() != current {
            return Err(TokenError::WrongOwner {
                owner: meta.uid(),
                current,
            });
        }
        if meta.mode() & 0o077 != 0 {
            return Err(TokenError::InsecureMode(meta.mode() & 0o7777));
        }

        let mut buf = Vec::with_capacity(meta.len() as usize);
        let mut f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(TokenError::Unreadable)?;
        use io::Read;
        f.read_to_end(&mut buf).map_err(TokenError::Unreadable)?;
        let s = String::from_utf8_lossy(&buf);
        let token = s.trim().to_string();
        if token.is_empty() {
            return Err(TokenError::Empty);
        }
        Ok((
            token,
            Stamp {
                mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                len: meta.len(),
            },
        ))
    }

    /// Current token, re-reading when the file's mtime/len changed.
    pub fn get(&self) -> Result<String, TokenError> {
        let mut guard = self.inner.lock().expect("token mutex poisoned");
        if let Some((tok, stamp)) = guard.as_ref() {
            if let Ok(meta) = fs::symlink_metadata(&self.path) {
                let fresh = Stamp {
                    mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    len: meta.len(),
                };
                if fresh.mtime == stamp.mtime && fresh.len == stamp.len {
                    return Ok(tok.clone());
                }
            }
        }
        // File changed, first read, or previously invalid: retry.
        match Self::read_validated(&self.path) {
            Ok((tok, stamp)) => {
                *guard = Some((tok.clone(), stamp));
                Ok(tok)
            }
            Err(e) => {
                // Cache the failure as "no token" so degraded mode is stable
                // and we don't re-stat spam; retry happens on next get()/401.
                *guard = None;
                Err(e)
            }
        }
    }

    /// Force re-read after a 401 (spec §6.3: rotate-aware).
    pub fn invalidate(&self) {
        *self.inner.lock().expect("token mutex poisoned") = None;
    }

    pub fn degraded_reason(&self) -> Option<TokenError> {
        // Re-attempt once so the caller can log a current reason.
        match Self::read_validated(&self.path) {
            Ok(_) => None,
            Err(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hookd-token-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_token(dir: &Path, body: &str, mode: u32) -> PathBuf {
        let p = dir.join("token");
        fs::write(&p, body).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn accepts_0600_regular_file() {
        let d = tmpdir("accept");
        let p = write_token(&d, "sekrit\n", 0o600);
        let (tok, _) = Token::read_validated(&p).unwrap();
        assert_eq!(tok, "sekrit"); // trimmed
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_group_readable() {
        let d = tmpdir("mode");
        let p = write_token(&d, "sekrit", 0o640);
        assert!(matches!(
            Token::read_validated(&p),
            Err(TokenError::InsecureMode(_))
        ));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_symlink() {
        let d = tmpdir("symlink");
        let real = write_token(&d, "sekrit", 0o600);
        let link = d.join("link");
        symlink(&real, &link).unwrap();
        assert!(matches!(
            Token::read_validated(&link),
            Err(TokenError::Symlink)
        ));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_missing_and_empty() {
        let d = tmpdir("missing");
        assert!(matches!(
            Token::read_validated(&d.join("nope")),
            Err(TokenError::Missing)
        ));
        let p = write_token(&d, "   \n", 0o600);
        assert!(matches!(Token::read_validated(&p), Err(TokenError::Empty)));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn error_display_never_contains_contents() {
        // The Display impl carries only metadata; sanity-check the shape.
        let e = TokenError::InsecureMode(0o644);
        assert!(!e.to_string().contains("sekrit"));
    }
}
