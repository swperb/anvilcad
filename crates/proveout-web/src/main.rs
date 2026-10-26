//! The Proveout web site.
//!
//! ```text
//! proveout [serve]                  run the site (default)
//! proveout set-plan EMAIL PLAN      set a user's plan by hand (free, pro, shop)
//! ```
//!
//! Configuration is read from the environment:
//!
//! - `PROVEOUT_BIND` (default `127.0.0.1:8080`) and `PROVEOUT_PUBLIC_URL` (the origin
//!   users see, default `http://localhost:8080`).
//! - `PROVEOUT_DB` (default `proveout.sqlite3`).
//! - `ANTHROPIC_API_KEY`: without it the site runs with the deterministic check only.
//! - `PROVEOUT_DAILY_REVIEW_LIMIT` (default 200): AI reviews per 24 hours across all
//!   users, a brake on spending.
//! - `PROVEOUT_TRUST_PROXY=1` behind a reverse proxy that sets `X-Forwarded-For`.
//! - `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_PRICE_PRO`,
//!   `STRIPE_PRICE_SHOP` for paid plans (see `billing`).

mod app;
mod billing;
mod db;
mod util;

use app::{AppState, Config, api_sender, router};
use db::Db;
use proveout_core::plan::Plan;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

fn env(k: &str, default: &str) -> String {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let db_path = env("PROVEOUT_DB", "proveout.sqlite3");
    match args.first().map(String::as_str) {
        None | Some("serve") => serve(&db_path),
        Some("set-plan") if args.len() == 3 => {
            let Some(plan) = Plan::parse(&args[2]) else {
                eprintln!("plan must be free, pro or shop");
                return ExitCode::FAILURE;
            };
            match Db::open(&db_path).and_then(|db| db.set_plan_by_email(&args[1].to_ascii_lowercase(), plan))
            {
                Ok(1) => {
                    println!("{} is now on {}", args[1], plan.as_str());
                    ExitCode::SUCCESS
                }
                Ok(_) => {
                    eprintln!("no user {}", args[1]);
                    ExitCode::FAILURE
                }
                Err(e) => {
                    eprintln!("{e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: proveout [serve] | proveout set-plan EMAIL PLAN");
            ExitCode::FAILURE
        }
    }
}

fn serve(db_path: &str) -> ExitCode {
    let bind = env("PROVEOUT_BIND", "127.0.0.1:8080");
    let public_url = env("PROVEOUT_PUBLIC_URL", "http://localhost:8080").trim_end_matches('/').to_string();
    let cfg = Config {
        secure_cookies: public_url.starts_with("https://"),
        public_url,
        trust_proxy: env("PROVEOUT_TRUST_PROXY", "0") == "1",
        daily_review_limit: env("PROVEOUT_DAILY_REVIEW_LIMIT", "200").parse().unwrap_or(200),
        stripe: billing::StripeConfig::from_env(),
    };
    let db = match Db::open(db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let sender = std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.trim().is_empty()).map(api_sender);
    eprintln!(
        "{} on {bind} ({}; AI review {}; billing {})",
        proveout_core::APP_NAME,
        cfg.public_url,
        if sender.is_some() { "on" } else { "off: ANTHROPIC_API_KEY is not set" },
        if cfg.stripe.is_some() { "on" } else { "off" }
    );
    let addr: SocketAddr = match bind.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("PROVEOUT_BIND: {e}");
            return ExitCode::FAILURE;
        }
    };
    let state = Arc::new(AppState::new(db, cfg, sender));
    let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let result: Result<(), std::io::Error> = rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, router(state).into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
