//! Albert runtime and embedded web control plane.

mod acl;
mod api;
mod artifacts;
mod bootstrap;
mod codex_http;
mod codex_model;
mod cogitator;
mod commands;
mod config;
mod connector_catalog;
mod console;
mod context;
mod db;
mod error;
mod history;
mod manifests;
mod memory;
mod messaging;
mod models;
mod openai_login;
mod openrouter_http;
mod prompt;
mod routines;
mod scratchpad;
mod selfconfig;
mod service;
mod sessions;
mod skills;
mod sql;
mod status;
mod subagents;
mod transport;

mod web;

use std::{
    env::{args, args_os, current_dir, current_exe, set_var},
    fs::create_dir_all,
    os::unix::process::CommandExt,
    process::Command,
    time::Duration,
};

use dotenvy::{dotenv, from_path, from_path_iter};
use octo_code::WORKSPACE_ENV;
use octo_connector_forkd::SKILLS_ENV;
use octo_connector_mail::ensure_crypto_provider;
use tokio::runtime::Runtime;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

use crate::{config::control::ControlSettings, error::Result, web::run};

fn main() -> Result<()> {
    // Environment setup happens before worker threads exist, never during a live
    // request. Relocated binaries do not read the developer source checkout.
    let settings = ControlSettings::load()?;
    let _ = from_path(settings.base.deploy_dir.join(".env"));
    if current_dir()? == settings.base.deploy_dir {
        let _ = dotenv();
    }
    let _ = create_dir_all(&settings.base.code_workspace);
    set_var(WORKSPACE_ENV, &settings.base.code_workspace);
    set_var(SKILLS_ENV, &settings.base.skills_dir);
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "albert=info,octo_core=warn".into()),
        )
        .with_target(true)
        .init();
    ensure_crypto_provider();
    info!("albert starting");
    let executor = Runtime::new()?;
    if args().nth(1).as_deref() == Some("login") {
        return executor.block_on(openai_login::run(&settings.base.subscription_auth_json));
    }
    let directory = settings.base.deploy_dir.clone();
    let restart = executor.block_on(run(settings));
    executor.shutdown_timeout(Duration::from_secs(2));
    if restart? {
        // Re-exec also works without systemd. Saved secrets override inherited
        // values in the new process; the live process environment stays frozen.
        let mut command = Command::new(current_exe()?);
        command.args(args_os().skip(1));
        if let Ok(entries) = from_path_iter(directory.join(".env")) {
            for (name, value) in entries.flatten() {
                command.env(name, value);
            }
        }
        return Err(command.exec().into());
    }
    Ok(())
}
