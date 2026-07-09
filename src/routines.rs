//! Albert's proactive routines — recurring self-care driven by the scheduler
//! (not user reminders). A routine alarm carries `{ routine: <name> }` in its
//! payload; the cogitator routes it to `run_routine`. This module seeds the base
//! ones on startup.

use std::{sync::Arc, time::Duration};

use octo_core::{ConnectorId, Envelope, EventBus, EventKind, InProcessBus};
use serde_json::{json, Value};
use tokio::time::sleep;
use tracing::{info, warn};

use crate::cogitator::{ROUTINE_MEMORY_REFLECTION, SCHEDULER_ID};

/// Ensure Albert's base routines exist on the scheduler. Idempotent: checks the
/// alarm list first and only adds a missing routine, so restarts don't duplicate.
/// Retries a few times to let the scheduler's control subscription come up. Runs
/// as a detached task so it never blocks the cogitator's event loop.
pub async fn seed_base_routine(bus: Arc<InProcessBus>, source: ConnectorId, period: u64) {
    for _ in 0..6u32 {
        sleep(Duration::from_millis(500)).await;

        let list = Envelope::new(
            source.clone(),
            EventKind::from_static("octo.scheduler.list_alarms"),
            json!({}),
        )
        .with_target(ConnectorId::new(SCHEDULER_ID));
        let Ok(resp) = bus
            .publish_and_await_response(list, Duration::from_secs(3))
            .await
        else {
            continue; // scheduler not up yet — retry
        };

        let already = resp
            .payload_as::<Value>()
            .and_then(|v| v.get("alarms").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            .any(|a| {
                a.get("payload")
                    .and_then(|p| p.get("routine"))
                    .and_then(Value::as_str)
                    == Some(ROUTINE_MEMORY_REFLECTION)
            });
        if already {
            info!("base memory-reflection routine already present");
            return;
        }

        let add = Envelope::new(
            source.clone(),
            EventKind::from_static("octo.scheduler.add_alarm"),
            json!({
                "trigger": { "type": "interval", "period_secs": period },
                "payload": { "routine": ROUTINE_MEMORY_REFLECTION }
            }),
        )
        .with_target(ConnectorId::new(SCHEDULER_ID));
        let _ = bus
            .publish_and_await_response(add, Duration::from_secs(3))
            .await;
        info!(period_secs = period, "seeded base memory-reflection routine");
        return;
    }
    warn!("could not seed base routine (scheduler not reachable)");
}
