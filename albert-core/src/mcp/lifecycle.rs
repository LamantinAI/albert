use std::{collections::BTreeMap, sync::atomic::Ordering, sync::Arc};

use octo_core::control::CancellationToken;
use octo_mcp::ServerSession;
use tokio::{select, spawn, sync::Semaphore};

use super::{Entry, McpRegistry, McpRegistryError, RegistryConfig, Result, ServerPhase};

impl McpRegistry {
    pub async fn configure(self: &Arc<Self>, config: RegistryConfig) -> Result<()> {
        config.validate(&self.base)?;
        let _apply = self.apply.lock().await;
        let gate = Arc::new(Semaphore::new(config.connect_concurrency));
        let mut jobs = Vec::new();
        let mut retired = Vec::new();
        {
            let mut entries = self.entries.write().unwrap();
            let mut old = std::mem::take(&mut *entries);
            let mut next = BTreeMap::new();
            for spec in config.servers {
                let previous = old.remove(&spec.id);
                match previous {
                    Some(mut entry)
                        if entry.spec.connection == spec.connection
                            && entry.spec.enabled == spec.enabled
                            && entry.limits == config.limits =>
                    {
                        entry.grants.retain(|name, token| {
                            if spec.allowed_tools.contains(name) {
                                true
                            } else {
                                token.cancel();
                                false
                            }
                        });
                        for name in &spec.allowed_tools {
                            entry
                                .grants
                                .entry(name.clone())
                                .or_insert_with(|| entry.cancel.child_token());
                        }
                        entry.spec = spec.clone();
                        next.insert(spec.id.clone(), entry);
                        continue;
                    }
                    Some(entry) => retired.push(entry),
                    None => {}
                }
                // Changed endpoints/credentials and disabling a server revoke its
                // lifecycle. Permission-only edits preserve other in-flight calls.
                let generation = self.generation.fetch_add(1, Ordering::Relaxed);
                let id = spec.id.clone();
                let enabled = spec.enabled;
                let cancel = CancellationToken::new();
                let grants = spec
                    .allowed_tools
                    .iter()
                    .map(|name| (name.clone(), cancel.child_token()))
                    .collect();
                let entry = Entry {
                    grants,
                    spec,
                    generation,
                    limits: config.limits.clone(),
                    client: None,
                    phase: if enabled {
                        ServerPhase::Connecting
                    } else {
                        ServerPhase::Disabled
                    },
                    error: None,
                    contract_valid: true,
                    cancel,
                };
                if enabled {
                    jobs.push((id.clone(), generation, entry.cancel.clone()));
                }
                next.insert(id, entry);
            }
            retired.extend(old.into_values());
            *entries = next;
        }
        for entry in retired {
            entry.cancel.cancel();
            if let Some(client) = entry.client {
                spawn(async move {
                    let _ = client.close().await;
                });
            }
        }
        for (id, generation, cancel) in jobs {
            let registry = self.clone();
            let gate = gate.clone();
            spawn(async move {
                let _permit = select! {biased;_=cancel.cancelled()=>return,permit=gate.acquire()=>match permit{Ok(permit)=>permit,Err(_)=>return}};
                registry.connect_entry(&id, generation).await;
            });
        }
        Ok(())
    }
    async fn connect_entry(&self, id: &str, generation: u64) {
        let settings = self
            .entries
            .read()
            .unwrap()
            .get(id)
            .filter(|entry| entry.generation == generation)
            .map(|entry| {
                (
                    entry.spec.connection.clone(),
                    entry.limits.clone(),
                    entry.cancel.clone(),
                )
            });
        let Some((connection, limits, cancel)) = settings else {
            return;
        };
        let result = ServerSession::connect(
            connection,
            &self.base,
            limits,
            self.credentials.clone(),
            cancel,
        )
        .await;
        let mut entries = self.entries.write().unwrap();
        let Some(entry) = entries
            .get_mut(id)
            .filter(|entry| entry.generation == generation)
        else {
            if let Ok(client) = result {
                spawn(async move {
                    let _ = client.close().await;
                });
            }
            return;
        };
        match result {
            Ok(client) => {
                entry.contract_valid = true;
                entry.client = Some(client);
                entry.phase = ServerPhase::Connected;
                entry.error = None;
            }
            Err(error) => {
                entry.phase = ServerPhase::Error;
                entry.error = Some(error.to_string());
            }
        }
    }
    pub async fn reconnect(self: &Arc<Self>, id: &str) -> Result<()> {
        let _apply = self.apply.lock().await;
        let (generation, old) = {
            let mut entries = self.entries.write().unwrap();
            let entry = entries
                .get_mut(id)
                .filter(|entry| entry.spec.enabled)
                .ok_or(McpRegistryError::Stale)?;
            entry.cancel.cancel();
            entry.cancel = CancellationToken::new();
            entry.grants = entry
                .spec
                .allowed_tools
                .iter()
                .map(|name| (name.clone(), entry.cancel.child_token()))
                .collect();
            entry.generation = self.generation.fetch_add(1, Ordering::Relaxed);
            entry.phase = ServerPhase::Connecting;
            entry.error = None;
            (entry.generation, entry.client.take())
        };
        if let Some(old) = old {
            spawn(async move {
                let _ = old.close().await;
            });
        }
        let registry = self.clone();
        let id = id.to_owned();
        spawn(async move {
            registry.connect_entry(&id, generation).await;
        });
        Ok(())
    }
    pub async fn refresh(&self, id: &str) -> Result<()> {
        let (client, generation) = {
            let entries = self.entries.read().unwrap();
            let entry = entries
                .get(id)
                .filter(|entry| entry.spec.enabled)
                .ok_or(McpRegistryError::Stale)?;
            (
                entry.client.clone().ok_or(McpRegistryError::Stale)?,
                entry.generation,
            )
        };
        let result = client.refresh().await;
        let mut entries = self.entries.write().unwrap();
        let entry = entries
            .get_mut(id)
            .filter(|entry| entry.generation == generation)
            .ok_or(McpRegistryError::Stale)?;
        match result {
            Ok(_) => {
                entry.contract_valid = true;
                entry.phase = ServerPhase::Connected;
                entry.error = None;
                Ok(())
            }
            Err(error) => {
                entry.phase = ServerPhase::Error;
                entry.error = Some(error.to_string());
                Err(error.into())
            }
        }
    }
}
