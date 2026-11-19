use std::time::Duration;

use octo_openai_auth::{Tokens, CLIENT_ID};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::from_slice;
use tokio::time::{sleep, timeout_at, Instant};

use super::{
    exchange::{client, exchange_using, read_response, rejection, transport, Category},
    DeviceAuthEndpoints, ExchangeError,
};

pub const DEVICE_AUTH_TTL: Duration = Duration::from_secs(15 * 60);
const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);

/// A single device challenge. Only the verification URL and user code may be
/// shown to the user. The provider's polling ID never leaves the adapter.
pub struct DeviceLogin {
    endpoints: DeviceAuthEndpoints,
    client: Client,
    challenge: Challenge,
    deadline: Instant,
}

#[derive(Serialize)]
struct IssueRequest {
    client_id: &'static str,
}

#[derive(Deserialize)]
struct Challenge {
    device_auth_id: String,
    #[serde(alias = "usercode")]
    user_code: String,
    interval: Option<PollInterval>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PollInterval {
    Text(String),
    Seconds(u64),
}

impl Challenge {
    fn interval(&self) -> Duration {
        match &self.interval {
            Some(PollInterval::Text(text)) => text.trim().parse().ok(),
            Some(PollInterval::Seconds(seconds)) => Some(*seconds),
            None => None,
        }
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_INTERVAL)
    }

    fn valid(&self) -> bool {
        visible_ascii(&self.device_auth_id)
            && (1..=64).contains(&self.user_code.len())
            && self
                .user_code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }
}

#[derive(Serialize)]
struct PollRequest<'a> {
    device_auth_id: &'a str,
    user_code: &'a str,
}

#[derive(Deserialize)]
struct Grant {
    authorization_code: String,
    code_verifier: String,
}

impl DeviceLogin {
    pub async fn start(
        endpoints: &DeviceAuthEndpoints,
        deadline: Instant,
    ) -> Result<Self, ExchangeError> {
        let client = client()?;
        let challenge = timeout_at(deadline, issue(&client, &endpoints.usercode))
            .await
            .map_err(|_| expired())??;
        Ok(Self {
            endpoints: endpoints.clone(),
            client,
            challenge,
            deadline,
        })
    }

    pub fn verification_url(&self) -> &str {
        &self.endpoints.verification_url
    }

    pub fn user_code(&self) -> &str {
        &self.challenge.user_code
    }

    pub async fn complete(self) -> Result<Tokens, ExchangeError> {
        timeout_at(self.deadline, async {
            let grant = self.poll().await?;
            exchange_using(
                &self.client,
                &self.endpoints.token,
                &self.endpoints.redirect,
                &grant.code_verifier,
                &grant.authorization_code,
            )
            .await
        })
        .await
        .map_err(|_| expired())?
    }

    async fn poll(&self) -> Result<Grant, ExchangeError> {
        let request = PollRequest {
            device_auth_id: &self.challenge.device_auth_id,
            user_code: &self.challenge.user_code,
        };
        let interval = self.challenge.interval();
        loop {
            let response = self
                .client
                .post(&self.endpoints.poll)
                .json(&request)
                .send()
                .await
                .map_err(|error| transport(&error, None))?;
            // Codex's upstream protocol uses both statuses while waiting for approval.
            if matches!(
                response.status(),
                StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
            ) {
                sleep(interval.min(self.deadline.saturating_duration_since(Instant::now()))).await;
                if Instant::now() >= self.deadline {
                    return Err(expired());
                }
                continue;
            }
            let (status, body) = read_response(response).await?;
            if !status.is_success() {
                return Err(rejection(status, &body));
            }
            let grant: Grant = from_slice(&body).map_err(|_| invalid(status))?;
            if !visible_ascii(&grant.authorization_code) || !visible_ascii(&grant.code_verifier) {
                return Err(invalid(status));
            }
            return Ok(grant);
        }
    }
}

async fn issue(client: &Client, endpoint: &str) -> Result<Challenge, ExchangeError> {
    let response = client
        .post(endpoint)
        .json(&IssueRequest {
            client_id: CLIENT_ID,
        })
        .send()
        .await
        .map_err(|error| transport(&error, None))?;
    let (status, body) = read_response(response).await?;
    if status == StatusCode::NOT_FOUND {
        return Err(ExchangeError::new(
            Category::DeviceUnavailable,
            Some(status.as_u16()),
        ));
    }
    if !status.is_success() {
        return Err(rejection(status, &body));
    }
    let challenge: Challenge = from_slice(&body).map_err(|_| invalid(status))?;
    if !challenge.valid() {
        return Err(invalid(status));
    }
    Ok(challenge)
}

fn visible_ascii(value: &str) -> bool {
    (1..=4096).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn invalid(status: StatusCode) -> ExchangeError {
    ExchangeError::new(Category::InvalidChallenge, Some(status.as_u16()))
}

fn expired() -> ExchangeError {
    ExchangeError::new(Category::DeviceExpired, None)
}
