use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use rig::completion::Message;
use tokio::time::timeout;
use tracing::{info, warn};

use super::{AlbertCogitator, CompactOptions, CompactOutcome, INSTRUCTIONS};
use crate::{
    context::{batches, compact_message, window_messages},
    models::needs_vision,
    status::StatusFeed,
};

impl AlbertCogitator {
    /// A live turn keeps its latest user message uncompressed. A manual compact
    /// includes all accepted records. The database compare-and-swap is the only
    /// point where the active prefix changes.
    pub(in crate::cogitator) async fn compact_with_options(
        &self,
        channel: &str,
        force: bool,
        preserve_latest: bool,
        progress: &StatusFeed,
        options: &CompactOptions,
    ) -> Result<CompactOutcome, String> {
        if force {
            progress
                .progress(if options.dry_run {
                    "Preparing a compaction dry run. The existing compact will not change."
                } else {
                    "Preparing conversation compact. Original messages will remain in history."
                })
                .await;
        }
        let settings = self.context_settings();
        if !self.history.retains_context() {
            return if force {
                Err("/compact requires retained SQLite history.".into())
            } else {
                Ok(CompactOutcome::Unchanged)
            };
        }
        if !settings.enabled && !force {
            return Ok(CompactOutcome::Unchanged);
        }
        let mut window = self
            .history
            .context(channel)
            .await
            .map_err(|e| e.to_string())?;
        let fresh = settings.new_tokens(&window);
        let summary_size = window
            .compact
            .as_ref()
            .map(|c| settings.message_tokens(&compact_message(&c.content)))
            .unwrap_or(0);
        if !force && fresh < settings.new_budget() && summary_size <= settings.compact_budget() {
            return Ok(CompactOutcome::Unchanged);
        }
        if preserve_latest {
            window.messages.pop();
        }
        if window.messages.is_empty() && window.compact.is_none() {
            return Ok(CompactOutcome::Unchanged);
        }
        if !force {
            progress.progress("Compacting conversation history before continuing. Original messages will remain in history.").await;
        }
        let boundary = window.through_id();
        let messages = window_messages(&window);
        let before = settings.messages_tokens(&messages);
        let max_tokens = settings.compact_output_tokens.min(
            settings
                .compact_budget()
                .saturating_sub(settings.message_tokens(&compact_message("")) + 16),
        );
        if max_tokens == 0 {
            return Err("No room for a compact under the configured context budget.".into());
        }
        let prompt = Message::system(format!("Compact the preceding conversation into at most {max_tokens} tokens. Preserve actionable detail and uncertainty; do not pad the result."));
        let guidance = format!("{INSTRUCTIONS}\n\nConfigured compaction guidance:\n{}\n\nOwner instructions for this invocation:\n{}", settings.compact_prompt, options.instructions);
        let snapshot = self.models.snapshot();
        let batches = batches(messages, &settings)?;
        let total_passes = batches.len();
        let result = timeout(Duration::from_secs(settings.timeout_secs), async {
            let mut summary = None::<String>;
            for (index, mut history) in batches.into_iter().enumerate() {
                progress
                    .progress(format!(
                        "Compacting conversation history: part {}/{}...",
                        index + 1,
                        total_passes
                    ))
                    .await;
                if let Some(previous) = &summary {
                    history.insert(0, compact_message(previous));
                }
                let vision = needs_vision(history.iter());
                let attempts = AtomicUsize::new(0);
                let text = snapshot
                    .run_with_tools(
                        vision,
                        false,
                        |model, refresh| {
                            let attempt = attempts.fetch_add(1, Ordering::Relaxed) + 1;
                            let prompt = prompt.clone();
                            let history = history.clone();
                            let guidance = &guidance;
                            async move {
                                if attempt > 1 {
                                    progress
                                        .progress(format!(
                                            "Retrying compaction request (attempt {attempt})..."
                                        ))
                                        .await;
                                }
                                self.compact_attempt(
                                    model, refresh, prompt, history, max_tokens, guidance,
                                )
                                .await
                            }
                        },
                        || false,
                        |status| async move {
                            info!(channel, pass=index+1, %status, "compaction model");
                        },
                    )
                    .await?;
                if settings.message_tokens(&compact_message(&text)) > settings.compact_budget() {
                    return Err(
                        "Compaction result exceeds its partition; previous context is unchanged."
                            .to_owned(),
                    );
                }
                summary = Some(text);
            }
            summary.ok_or_else(|| "No messages to compact.".to_owned())
        })
        .await
        .map_err(|_| {
            warn!(
                channel,
                timeout_secs = settings.timeout_secs,
                "compaction total deadline reached"
            );
            "Compaction exceeded its total time budget; previous context is unchanged.".to_string()
        })??;
        let after = settings.message_tokens(&compact_message(&result));
        if options.dry_run {
            progress.progress(format!("Compaction dry run complete: ~{before} -> ~{after} tokens (estimated). Current compact unchanged.")).await;
            info!(
                channel,
                boundary, before, after, "conversation compact dry run; not saved"
            );
            return Ok(CompactOutcome::DryRun { before, after });
        }

        if after > settings.compact_budget() || after >= before {
            return Err(
                "Compaction did not fit or reduce the context; previous context is unchanged."
                    .into(),
            );
        }
        let saved = self
            .history
            .save_compact(
                channel,
                window.compact.as_ref().map(|c| c.id),
                boundary,
                &result,
            )
            .await
            .map_err(|e| e.to_string())?;
        info!(
            channel,
            boundary, before, after, saved, "conversation compact"
        );
        if !saved {
            return Err(
                "Conversation compact changed concurrently; retry with the latest context.".into(),
            );
        }
        progress
            .progress(format!(
                "Conversation compact saved: ~{before} -> ~{after} tokens (estimated)."
            ))
            .await;
        Ok(CompactOutcome::Saved)
    }
}
