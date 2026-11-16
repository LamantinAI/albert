-- A workspace-wide choice, independent from model availability and transient
-- memory connection state. Existing ledgers are adopted by the migration owner.
CREATE TABLE albert_memory_setup (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    state TEXT NOT NULL CHECK(state IN ('uninitialized','pending','adopted','selected')),
    revision INTEGER NOT NULL CHECK(revision>0)
);
