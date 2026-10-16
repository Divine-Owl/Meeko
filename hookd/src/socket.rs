// ╭──────────────────────────────────────────────────────────────────────────╮
// │   S O C K E T                                                            │
// │   meeko-hookd · the 0600 NDJSON unix socket (spec §6.1)                  │
// │                                                                          │
// │   SO_PEERCRED uid gate · role = /proc/<pid>/exe match (defense-in-depth  │
// │   only, NOT authentication) · ≤8 connections · 1 MiB line · per-verb     │
// │   caps before parse · 5s idle read timeout · zero clients = buffer nil.  │
// │   GPL-3.0-only                                                           │
// ╰──────────────────────────────────────────────────────────────────────────╯

use crate::host::Host;
use crate::projection::{Projection, Push};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, Semaphore};
use tokio::time::timeout;

const MAX_CONNS: usize = 8;
const MAX_LINE: usize = 1024 * 1024; // 1 MiB hard ceiling
const IDLE_TIMEOUT: Duration = Duration::from_secs(5);
/// Per-verb payload caps, enforced on the raw line before JSON parsing
/// (CR&S audit item: junk payloads die before allocation).
const CAP_CONTROL: usize = 1024; // hello/ping/lists/verdicts
const CAP_SEND: usize = 8 * 1024 + 256; // send.text ≤ 8 KiB + JSON slack

#[derive(PartialEq, Clone, Copy)]
enum Role {
    Shell,
    Observer,
}

pub async fn serve(
    sock_path: &Path,
    projection: Arc<tokio::sync::RwLock<Projection>>,
    host: Arc<Host>,
    shell_bin: String,
    tx: tokio::sync::broadcast::Sender<Push>,
) -> Result<(), String> {
    // Parent dir must be 0700; the socket itself 0600 (spec §6.1).
    if let Some(dir) = sock_path.parent() {
        let meta = std::fs::metadata(dir).map_err(|e| format!("socket dir: {e}"))?;
        use std::os::unix::fs::MetadataExt;
        if meta.mode() & 0o077 != 0 {
            return Err(format!(
                "socket dir {} mode {:#o} is group/other accessible",
                dir.display(),
                meta.mode() & 0o7777
            ));
        }
    }
    // Stale socket from a previous run would block bind; only remove if it
    // really is a socket (never follow a replaced path blindly).
    if let Ok(meta) = std::fs::symlink_metadata(sock_path) {
        use std::os::unix::fs::FileTypeExt;
        if meta.file_type().is_socket() {
            std::fs::remove_file(sock_path).map_err(|e| format!("stale socket: {e}"))?;
        } else {
            return Err(format!(
                "{} exists and is not a socket",
                sock_path.display()
            ));
        }
    }
    let listener = UnixListener::bind(sock_path).map_err(|e| format!("bind: {e}"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(sock_path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod socket: {e}"))?;

    let slots = Arc::new(Semaphore::new(MAX_CONNS));
    eprintln!("meeko-hookd: listening on {}", sock_path.display());

    loop {
        let (stream, _addr) = listener
            .accept()
            .await
            .map_err(|e| format!("accept: {e}"))?;
        // Connection cap: refuse immediately rather than queue (a restarted
        // shell must never starve the cap — dead clients are also dropped
        // by the per-connection idle timeout).
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };

        // uid gate BEFORE any parse (spec §6.1).
        match peer_uid(&stream) {
            Ok(uid) if uid == unsafe { libc::getuid() } => {}
            Ok(_) => {
                drop(stream);
                continue;
            }
            Err(_) => {
                drop(stream);
                continue;
            }
        }

        let role = classify(&stream, &shell_bin);
        let projection = projection.clone();
        let host = host.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = handle(stream, role, projection, host, tx).await;
            drop(permit);
        });
    }
}

fn peer_uid(stream: &UnixStream) -> Result<u32, ()> {
    use std::os::unix::io::AsRawFd;
    let mut ucred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut ucred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc == 0 {
        Ok(ucred.uid)
    } else {
        Err(())
    }
}

/// Role is defense-in-depth only — any same-uid process can exec the real
/// quickshell binary. Protection rests on 0600 + SO_PEERCRED uid==uid under
/// the same-uid-trusted threat model (spec §12). Do not call this auth.
fn classify(stream: &UnixStream, shell_bin: &str) -> Role {
    use std::os::unix::io::AsRawFd;
    let mut ucred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut ucred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Role::Observer;
    }
    let exe = std::fs::read_link(format!("/proc/{}/exe", ucred.pid)).unwrap_or_default();
    let matches = exe.file_name().map(|n| n == shell_bin).unwrap_or(false)
        || exe.to_string_lossy() == shell_bin;
    if matches {
        Role::Shell
    } else {
        Role::Observer
    }
}

async fn handle(
    stream: UnixStream,
    role: Role,
    projection: Arc<tokio::sync::RwLock<Projection>>,
    host: Arc<Host>,
    tx: broadcast::Sender<Push>,
) -> Result<(), String> {
    let (read, write) = stream.into_split();
    let mut reader = BufReader::new(read);
    // Both the response path and the push fan-out write to the same half;
    // serialize them so NDJSON lines never interleave mid-line.
    let write = Arc::new(tokio::sync::Mutex::new(write));

    // On connect: push the snapshot (spec §6.1). Zero clients elsewhere
    // means nothing is buffered — the broadcast receiver is per-connection.
    let mut rx = tx.subscribe();
    {
        let proj = projection.read().await;
        let line = with_id_snapshot(&proj);
        drop(proj);
        let mut w = write.lock().await;
        write_all(&mut w, &line).await?;
    }
    // Fan-out task: pushes from the host loop for this connection.
    let push_write = write.clone();
    let pusher = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(push) => {
                    let mut w = push_write.lock().await;
                    if write_all(&mut w, &push.to_line()).await.is_err() {
                        break;
                    }
                }
                // Lagged: the client fell behind; it will re-snapshot on
                // reconnect. Closed: the host loop is gone.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Request loop: NDJSON lines in, one envelope out per line.
    let mut buf = Vec::with_capacity(256);
    loop {
        buf.clear();
        let n = match timeout(IDLE_TIMEOUT, reader.read_until(b'\n', &mut buf)).await {
            Ok(Ok(n)) => n,
            Ok(Err(_)) | Err(_) => break, // dead/idle client: drop (slot freed)
        };
        if n == 0 {
            break;
        }
        if buf.len() > MAX_LINE {
            let resp = json!({"id": null, "ok": false, "error": "line_too_long"});
            let mut w = write.lock().await;
            let _ = write_all(&mut w, &resp.to_string()).await;
            break;
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let resp = dispatch(line, role, &projection, &host).await;
        let mut w = write.lock().await;
        if write_all(&mut w, &resp).await.is_err() {
            break;
        }
    }
    pusher.abort();
    Ok(())
}

async fn write_all(w: &mut tokio::net::unix::OwnedWriteHalf, s: &str) -> Result<(), String> {
    w.write_all(s.as_bytes()).await.map_err(|e| e.to_string())?;
    w.write_all(b"\n").await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())
}

/// Pre-parse verb extraction: read the verb and raw length off the line
/// WITHOUT full JSON parsing, enforce the per-verb cap, then parse.
/// Control verbs are ≤1 KiB; send.text ≤8 KiB (spec §6.1).
fn precheck(line: &str) -> Result<(String, Value), (Option<Value>, String)> {
    let id = peek_id(line);
    if line.len() > MAX_LINE {
        return Err((id, "line_too_long".into()));
    }
    let verb = peek_verb(line).ok_or_else(|| (id.clone(), "bad_request".to_string()))?;
    let cap = if verb == "send" {
        CAP_SEND
    } else {
        CAP_CONTROL
    };
    if line.len() > cap {
        return Err((id, format!("{verb}_payload_too_large")));
    }
    let value: Value =
        serde_json::from_str(line).map_err(|_| (id.clone(), "bad_request".to_string()))?;
    Ok((verb, value))
}

/// Cheap `"verb": "x"` scan that works on unparseable junk.
fn peek_verb(line: &str) -> Option<String> {
    let idx = line.find("\"verb\"")?;
    let rest = &line[idx + 6..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn peek_id(line: &str) -> Option<Value> {
    let idx = line.find("\"id\"")?;
    let rest = &line[idx + 4..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let num: i64 = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()?;
    Some(json!(num))
}

fn with_id_snapshot(proj: &Projection) -> String {
    let mut s = proj.snapshot();
    if let Value::Object(ref mut m) = s {
        m.insert("id".into(), Value::Null);
    }
    s.to_string()
}

async fn dispatch(
    line: &str,
    role: Role,
    projection: &Arc<tokio::sync::RwLock<Projection>>,
    host: &Arc<Host>,
) -> String {
    let (verb, req) = match precheck(line) {
        Ok(x) => x,
        Err((id, err)) => return err_envelope(id, err),
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let args = |k: &str| params.get(k).and_then(Value::as_str).map(str::to_string);

    match verb.as_str() {
        "ping" => ok_envelope(id, json!({"pong": true})),
        "hello" => {
            let proj = projection.read().await;
            ok_envelope(id, proj.snapshot())
        }
        "approvals.list" => {
            let proj = projection.read().await;
            ok_envelope(id, json!({"approvals": proj.approvals()}))
        }
        "agents.list" => {
            let proj = projection.read().await;
            ok_envelope(id, json!({"agents": proj.agents()}))
        }
        "respondApproval" => {
            if role != Role::Shell {
                return err_envelope(id, "peer_not_shell");
            }
            let Some(entry_id) = args("entryId") else {
                return err_envelope(id, "bad_request");
            };
            // optionId must be string-or-null and, when present, a member of
            // the pending approval's projected options — arbitrary JSON never
            // reaches the host call body (CR&S audit item 1).
            let option_id = match params.get("optionId") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.as_str()),
                Some(_) => return err_envelope(id, "bad_option"),
            };
            // Local projection gate: verdicts for stale/unknown entries and
            // unknown options are rejected here, never proxied (spec §12).
            {
                let proj = projection.read().await;
                if proj.pending_approval(&entry_id).is_none() {
                    return err_envelope(id, "stale_entry");
                }
                if !proj.option_is_pending(&entry_id, option_id) {
                    return err_envelope(id, "bad_option");
                }
            }
            let body = json!({
                "entryId": entry_id,
                "optionId": option_id.map(|s| Value::String(s.to_string())).unwrap_or(Value::Null),
            });
            match host.call("respondPermission", body).await {
                Ok(_) => ok_envelope(id, json!({"asked": "respondPermission"})),
                Err(e) => err_envelope(id, e),
            }
        }
        "send" => {
            if role != Role::Shell {
                return err_envelope(id, "peer_not_shell");
            }
            let (Some(bot_id), Some(text)) = (args("botId"), args("text")) else {
                return err_envelope(id, "bad_request");
            };
            {
                let proj = projection.read().await;
                if !proj.agent_exists(&bot_id) {
                    return err_envelope(id, "unknown_bot");
                }
            }
            match host
                .call("send", json!({"botId": bot_id, "text": text}))
                .await
            {
                Ok(_) => ok_envelope(id, json!({"asked": "send"})),
                Err(e) => err_envelope(id, e),
            }
        }
        "stop" => {
            if role != Role::Shell {
                return err_envelope(id, "peer_not_shell");
            }
            let Some(bot_id) = args("botId") else {
                return err_envelope(id, "bad_request");
            };
            {
                let proj = projection.read().await;
                if !proj.agent_exists(&bot_id) {
                    return err_envelope(id, "unknown_bot");
                }
            }
            match host.call("stop", json!({"botId": bot_id})).await {
                Ok(_) => ok_envelope(id, json!({"asked": "stop"})),
                Err(e) => err_envelope(id, e),
            }
        }
        _ => err_envelope(id, "unknown_verb"),
    }
}

fn ok_envelope(id: Value, result: Value) -> String {
    json!({"id": id, "ok": true, "result": result}).to_string()
}

fn err_envelope(id: impl Into<Value>, err: impl std::fmt::Display) -> String {
    json!({"id": id.into(), "ok": false, "error": err.to_string()}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precheck_caps_control_verb() {
        let big = format!(
            "{{\"id\":1,\"verb\":\"hello\",\"pad\":\"{}\"}}",
            "x".repeat(2000)
        );
        let Err((id, err)) = precheck(&big) else {
            panic!("expected cap rejection");
        };
        assert_eq!(err, "hello_payload_too_large");
        assert_eq!(id, Some(json!(1)));
    }

    #[test]
    fn precheck_caps_send_at_8k() {
        let ok_line = format!(
            "{{\"id\":2,\"verb\":\"send\",\"params\":{{\"botId\":\"b\",\"text\":\"{}\"}}}}",
            "y".repeat(7000)
        );
        assert!(precheck(&ok_line).is_ok());
        let big = format!(
            "{{\"id\":2,\"verb\":\"send\",\"params\":{{\"botId\":\"b\",\"text\":\"{}\"}}}}",
            "y".repeat(9000)
        );
        let Err((_, err)) = precheck(&big) else {
            panic!("expected cap rejection");
        };
        assert_eq!(err, "send_payload_too_large");
    }

    #[test]
    fn precheck_rejects_junk_without_panic() {
        assert!(precheck("not json at all").is_err());
        assert!(precheck("{\"verb\": 42}").is_err());
        assert!(precheck("{}").is_err());
        // huge junk line dies on the cap before parsing
        let junk = "z".repeat(MAX_LINE + 1);
        assert!(matches!(precheck(&junk), Err((_, e)) if e == "line_too_long"));
    }

    #[test]
    fn peek_verb_extracts_from_valid_line() {
        assert_eq!(
            peek_verb("{\"id\":1, \"verb\":\"respondApproval\"}").as_deref(),
            Some("respondApproval")
        );
        assert_eq!(peek_verb("{\"no\":\"verb\"}"), None);
    }

    #[test]
    fn envelopes_carry_id_and_shape() {
        let ok = ok_envelope(json!(7), json!({"pong": true}));
        assert!(ok.contains("\"id\":7") && ok.contains("\"ok\":true"));
        let err = err_envelope(Some(json!(7)), "stale_entry");
        assert!(err.contains("\"ok\":false") && err.contains("stale_entry"));
    }
}
