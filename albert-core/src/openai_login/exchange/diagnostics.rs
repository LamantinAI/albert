use std::{error::Error as StdError, fmt::Display, fmt::Formatter, fmt::Result as FmtResult};

use crate::error::Error;

/// No provider-controlled text, request details, or underlying errors are retained.
#[derive(Clone, Copy, Debug)]
pub struct ExchangeError {
    category: Category,
    status: Option<u16>,
    oauth_code: Option<&'static str>,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Category {
    InvalidCode,
    ClientSetup,
    Timeout,
    Connect,
    Transport,
    Rejected,
    ResponseTooLarge,
    ResponseRead,
    ResponseJson,
    MissingCredentials,
    MissingAccount,
}
impl ExchangeError {
    pub(super) fn new(category: Category, status: Option<u16>) -> Self {
        Self {
            category,
            status,
            oauth_code: None,
        }
    }
    pub(super) fn rejected(status: u16, oauth_code: Option<&str>) -> Self {
        let oauth_code = match oauth_code {
            Some("invalid_grant") => Some("invalid_grant"),
            Some("token_expired") => Some("token_expired"),
            Some("invalid_client") => Some("invalid_client"),
            Some("unsupported_country_region_territory") => {
                Some("unsupported_country_region_territory")
            }
            Some("invalid_request") => Some("invalid_request"),
            Some("unauthorized_client") => Some("unauthorized_client"),
            Some("access_denied") => Some("access_denied"),
            Some("temporarily_unavailable") => Some("temporarily_unavailable"),
            Some("server_error") => Some("server_error"),
            Some("rate_limit_exceeded") => Some("rate_limit_exceeded"),
            _ => None,
        };
        Self {
            category: Category::Rejected,
            status: Some(status),
            oauth_code,
        }
    }
    pub fn category(&self) -> &'static str {
        match self.category {
            Category::InvalidCode => "invalid_code",
            Category::ClientSetup => "client_setup",
            Category::Timeout => "transport_timeout",
            Category::Connect => "transport_connect",
            Category::Transport => "transport_failure",
            Category::Rejected => "http_rejected",
            Category::ResponseTooLarge => "response_too_large",
            Category::ResponseRead => "response_read",
            Category::ResponseJson => "response_json",
            Category::MissingCredentials => "missing_credentials",
            Category::MissingAccount => "missing_account",
        }
    }
    pub fn http_status(&self) -> Option<u16> {
        self.status
    }
    pub fn oauth_code(&self) -> Option<&'static str> {
        self.oauth_code
    }
    pub fn public_message(&self) -> &'static str {
        match self.category {
            Category::InvalidCode => "The authorization code is malformed. Start a new sign-in attempt.",
            Category::ClientSetup => "Authorization could not initialize. Ask the workspace owner to review the server setup.",
            Category::Timeout => "The token exchange timed out. Check the server connection, then start a new sign-in attempt.",
            Category::Connect => "The server could not connect to OpenAI for the token exchange. Check server connectivity or proxy settings, then start a new sign-in attempt.",
            Category::Transport => "The token exchange connection failed. Check server connectivity or proxy settings, then start a new sign-in attempt.",
            Category::Rejected => match self.oauth_code {
                Some("unsupported_country_region_territory") => "OpenAI blocked the token exchange because the server connection is from an unsupported country or region. The workspace owner needs to review the server network or proxy settings before you start a new sign-in attempt.",
                Some("invalid_grant" | "token_expired") => "OpenAI rejected the authorization code. It may be expired, already used, or belong to another attempt. Start a new sign-in attempt.",
                Some("invalid_client" | "unauthorized_client") => "OpenAI rejected the OAuth client configuration. Ask the workspace owner to review the server setup before trying again.",
                Some("access_denied") => "OpenAI denied authorization. Review account access, then start a new sign-in attempt.",
                _ if self.status == Some(429) || self.oauth_code == Some("rate_limit_exceeded") => "OpenAI rate-limited the token exchange. Wait briefly, then start a new sign-in attempt.",
                _ if self.status.is_some_and(|status| status >= 500) || matches!(self.oauth_code, Some("temporarily_unavailable" | "server_error")) => "OpenAI is temporarily unable to complete the token exchange. Try a new sign-in attempt later.",
                _ => "OpenAI rejected the token exchange. Ask the workspace owner to review the server diagnostics, then start a new sign-in attempt.",
            },
            Category::ResponseTooLarge => "OpenAI returned an oversized token response. Credentials were not saved. Start a new sign-in attempt later.",
            Category::ResponseRead => "The token response could not be read. Credentials were not saved. Start a new sign-in attempt.",
            Category::ResponseJson => "OpenAI returned an invalid token response. Credentials were not saved. Start a new sign-in attempt later.",
            Category::MissingCredentials => "The token response did not include the required credentials. Nothing was saved. Start a new sign-in attempt.",
            Category::MissingAccount => "The token response did not include a subscription account. Nothing was saved. Review account access, then start a new sign-in attempt.",
        }
    }
}
impl Display for ExchangeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        formatter.write_str(self.public_message())
    }
}
impl StdError for ExchangeError {}
impl From<ExchangeError> for Error {
    fn from(error: ExchangeError) -> Self {
        Self::Auth(error.public_message().into())
    }
}
