//! Actual adapter instances and host-only delivery attempts. Capability is not
//! connectivity. Telegram observations are chunks, never whole-send receipts.

use std::{collections::HashMap, sync::Arc, sync::Mutex};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::spawn;
use tracing::warn;

use crate::{
    messaging::contract::MessageId, messaging::store::DeliveryEvent,
    messaging::store::DeliveryObservation, messaging::store::MessageStore,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    Telegram,
    Console,
}
impl AdapterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Console => "console",
        }
    }
}
#[derive(Clone, Default)]
pub struct AdapterRegistry(Arc<Mutex<HashMap<String, AdapterKind>>>);
impl AdapterRegistry {
    pub fn merge_verified(&self, verified: &Self) {
        let instances = verified.0.lock().expect("adapter registry").clone();
        self.0.lock().expect("adapter registry").extend(instances);
    }

    pub fn register(&self, connector: &str, kind: AdapterKind) {
        self.0
            .lock()
            .expect("adapter registry")
            .insert(connector.into(), kind);
    }
    pub fn get(&self, connector: &str) -> Option<AdapterKind> {
        self.0.lock().ok()?.get(connector).copied()
    }
}

/// Fresh for one committed send-start; never deserialized from model/tool JSON.
/// No owner/Ingress, temporary cleanup scope or native typing state is carried.
#[derive(Clone)]
pub struct ReportAttempt {
    store: MessageStore,
    pub message: MessageId,
    pub token: String,
    pub adapter: AdapterKind,
    pub connector: String,
    pub channel: String,
    max_observations: usize,
}
impl ReportAttempt {
    pub fn new(
        store: MessageStore,
        message: MessageId,
        token: String,
        adapter: AdapterKind,
        connector: String,
        channel: String,
        max_observations: usize,
    ) -> Self {
        Self {
            store,
            message,
            token,
            adapter,
            connector,
            channel,
            max_observations,
        }
    }
    pub fn view_suppressed(&self) {
        let attempt = self.clone();
        spawn(async move {
            attempt
                .record(DeliveryObservation::ViewSuppressed, vec![])
                .await;
        });
    }
    pub fn chunk(
        &self,
        adapter: AdapterKind,
        channel: &str,
        observation: DeliveryObservation,
        ids: Vec<String>,
    ) {
        if self.adapter != adapter || self.channel != channel {
            return;
        }
        let attempt = self.clone();
        spawn(async move {
            attempt.record(observation, ids).await;
        });
    }
    pub async fn console(&self, success: bool) {
        if self.adapter != AdapterKind::Console {
            return;
        }
        self.record(
            if success {
                DeliveryObservation::StreamWritten
            } else {
                DeliveryObservation::StreamUnknown
            },
            vec![],
        )
        .await;
    }
    pub async fn record(&self, outcome: DeliveryObservation, ids: Vec<String>) {
        if let Err(error) = self
            .store
            .observe_delivery(
                &self.message,
                DeliveryEvent {
                    token: &self.token,
                    adapter: self.adapter,
                    outcome,
                    native_ids: &ids,
                    now: Utc::now().timestamp_millis(),
                    max_observations: self.max_observations,
                },
            )
            .await
        {
            warn!(%error, message_id=self.message.as_str(), "delivery observation was not recorded");
        }
    }
}
