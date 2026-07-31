//! Crate error type — explicit variants with `#[from]` (no `anyhow`), mirroring
//! `octo_core::OctoError` / octolab style.

use std::{io::Error as IoError, result::Result as StdResult};

use octo_core::{ConfigError, OctoError};
use octo_history::HistoryError;
use rig::{completion::PromptError, http_client::Error as HttpError};
use thiserror::Error;
use toml::de::Error as TomlError;

#[derive(Debug, Error)]
pub enum Error {
    /// Config problems that aren't a TOML parse error (missing file, missing
    /// secret env var, …), stringified at the boundary.
    #[error("config: {0}")]
    Config(String),

    #[error("config toml: {0}")]
    Toml(#[from] TomlError),

    /// Octo's connector-manifest loader (`from_config_file`) errors.
    #[error("octo config: {0}")]
    OctoConfig(#[from] ConfigError),

    #[error("octo runtime: {0}")]
    Octo(#[from] OctoError),

    #[error("history: {0}")]
    History(#[from] HistoryError),

    /// OpenAI ChatGPT-subscription auth: reading/parsing the token store,
    /// an expired access token, or a missing account id.
    #[error("subscription auth: {0}")]
    Auth(String),

    #[error("llm client: {0}")]
    LlmClient(#[from] HttpError),

    #[error("llm prompt: {0}")]
    LlmPrompt(#[from] PromptError),

    #[error("io: {0}")]
    Io(#[from] IoError),

    /// kaeru substrate errors are stringified at the boundary (we don't depend on
    /// kaeru's error enum directly).
    #[error("kaeru: {0}")]
    Kaeru(String),

    /// Voice transcription (the subscription dictation endpoint): a refused token, an
    /// over-long recording, or an unreadable response.
    #[error("transcribe: {0}")]
    Transcribe(String),
}

pub type Result<T> = StdResult<T, Error>;
