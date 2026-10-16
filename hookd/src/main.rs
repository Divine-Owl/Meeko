// ╭──────────────────────────────────────────────────────────────────────────╮
// │   M A I N                                                                │
// │   meeko-hookd · the Meeko agent bridge                                   │
// │                                                                          │
// │   One new process for Meeko: a memory-only projection of the Codync      │
// │   host (SSE), served over a 0600 unix socket with SO_PEERCRED gating.    │
// │   Spec: docs/superpowers/specs/2026-10-15-meeko-shell-design.md §5–6.    │
// │   GPL-3.0-only · github.com/meeko-shell                                  │
// ╰──────────────────────────────────────────────────────────────────────────╯

mod config;
mod host;
mod projection;
mod socket;
mod token;

use projection::Projection;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

fn main() {
    let cfg = match config::Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("meeko-hookd: {e}");
            std::process::exit(1);
        }
    };

    // Token: validate up front; failure is not fatal — we start in degraded
    // read-only mode (spec §6.3) and the host loop retries via backoff.
    let token = Arc::new(token::Token::new(cfg.token_path.clone()));
    if let Some(reason) = token.degraded_reason() {
        eprintln!(
            "meeko-hookd: starting degraded — token at {} refused: {reason}",
            cfg.token_path.display()
        );
    }

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("meeko-hookd: runtime: {e}");
            std::process::exit(1);
        }
    };

    rt.block_on(async move {
        let projection: Arc<RwLock<Projection>> = Arc::new(RwLock::new(Projection::new()));
        // Bounded fan-out: pushes to connected shells. Zero receivers is a
        // normal state (shell not running) and buffers nothing.
        let (tx, _first_rx) = broadcast::channel::<projection::Push>(256);
        drop(_first_rx);

        let host = Arc::new(host::Host::new(cfg.host_url.clone(), token));
        tokio::spawn(host::run(
            host.as_ref().clone(),
            projection.clone(),
            tx.clone(),
        ));

        let serve = socket::serve(
            &cfg.socket_path,
            projection,
            host,
            cfg.shell_bin.clone(),
            tx,
        );
        tokio::select! {
            res = serve => {
                if let Err(e) = res {
                    eprintln!("meeko-hookd: server exited: {e}");
                    std::process::exit(1);
                }
            }
            _ = tokio::signal::ctrl_c() => {
                let _ = std::fs::remove_file(&cfg.socket_path);
            }
        }
    });
}
