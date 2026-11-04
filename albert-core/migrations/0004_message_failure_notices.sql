-- Restart-safe terminal notice bookkeeping; canonical notice content stays in
-- the source session journal. A notification never invokes a model.
ALTER TABLE albert_message_outbox ADD COLUMN failure_notified INTEGER NOT NULL DEFAULT 0 CHECK (failure_notified IN (0,1));
CREATE INDEX albert_message_deadlines ON albert_messages(deadline_unix_ms);
CREATE INDEX albert_message_inbox_state ON albert_message_inbox(work_state,message_id);
CREATE INDEX albert_message_failure_notice ON albert_message_outbox(failure_notified,state);
