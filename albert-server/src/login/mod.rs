//! Interactive CLI for ChatGPT subscription device authorization.
use std::{fs::DirBuilder, os::unix::fs::DirBuilderExt, path::Path};

use albert_core::{
    error::{Error, Result},
    openai_login::{DeviceAuthEndpoints, DeviceLogin, DEVICE_AUTH_TTL},
};
use octo_openai_auth::{plan_from_jwt, SubscriptionAuth, Tokens};
use tokio::{select, signal::ctrl_c, time::Instant};

pub async fn run(store_path: &Path) -> Result<()> {
    let tokens = select! {
        tokens = authorize() => tokens?,
        signal = ctrl_c() => {
            signal?;
            return Err(Error::Auth("Device sign-in cancelled.".into()));
        }
    };
    if let Some(parent) = store_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
    }
    let plan = plan_from_jwt(&tokens.access_token);
    SubscriptionAuth::new(store_path.to_owned())
        .install(tokens)
        .await?;
    println!(
        "Signed in{}. Tokens saved.",
        plan.map(|p| format!(" (plan: {p})")).unwrap_or_default()
    );
    Ok(())
}

async fn authorize() -> Result<Tokens> {
    let endpoints = DeviceAuthEndpoints::startup()?;
    let login = DeviceLogin::start(&endpoints, Instant::now() + DEVICE_AUTH_TTL).await?;
    println!(
        "\nOpen this address and sign in to ChatGPT:\n\n  {}\n\n\
         Enter this one-time code within 15 minutes:\n\n  {}\n\n\
         Enter this code only if you started this sign-in in Albert.\n\
         Waiting for approval. Press Ctrl+C to cancel.\n",
        login.verification_url(),
        login.user_code(),
    );
    Ok(login.complete().await?)
}
