# SQLite connector

The Octo SQL connector stores structured records in a dedicated SQLite
file. Albert can create and search purpose stores, inspect table/column meaning,
and issue parameterized SQL. The connector runs no extra language model.
Internal conversation history and memory databases keep their existing ownership.

## Configuration

The shipped `config/connectors/sql/sql.toml` enables the connector without extra
setup. On first startup it creates `state/sql/sql-data.sqlite` and missing parent
directories; existing records survive restart. No user stores are prepopulated.
An existing deployment needs the new manifest added when updating the binary.
The Docker layout maps `state/` onto the persistent `/data/state` volume path.
To disable the connector, remove its manifest or rename it without a `.toml`
extension. The `.example` file provides a reference configuration.

Set `path` in the manifest and restart Albert to use another location. Relative
paths resolve from that manifest's directory. Any configured path must be
writable by the service, including missing parent directories and its
SQLite sidecar files, and must not alias internal history or memory storage.
Changing the path does not migrate records. No automatic backups are configured.

There are at most 20 logical stores inside this one file. Archived stores count
toward the quota. Committed removal of a store releases its slot. The service
catalog, metadata, approval state, receipts and audit are protected from agent SQL.

Telegram SQL access starts with the ACL owner's real private-chat identity.
Origin tags, a guessed store ID or a model-provided user ID grant no access.
Trusted local console input uses its own principal. A model call without verified
host context is rejected even if its turn claims owner permissions.

## Records and schema

Use `sql.store.list` / `sql.store.search` before creating a duplicate store.
`sql.store.create` takes `name`, `purpose`, optional `tags`, and an
`idempotency_key`. It returns the store ID, generated table namespace and revision.
`sql.store.describe` returns actual object/column IDs, schema and semantic
metadata: descriptions, units, NULL meaning, enums, time basis and provenance.
Keep a selected ID in the conversation, not global connector state.

`sql.schema.apply` creates a table with semantic descriptions. For example, after
a store was created, use its actual ID and current revision:

```json
{
  "store_id": "<returned-store-id>",
  "expected_revision": 1,
  "idempotency_key": "nutrition-schema-1",
  "table": "meals",
  "columns": [
    {"name": "id", "data_type": "INTEGER", "primary_key": true},
    {"name": "protein_g", "data_type": "REAL", "semantics": {"units": "g", "description": "Protein consumed"}},
    {"name": "eaten_at", "data_type": "TEXT", "semantics": {"time_basis": "UTC ISO 8601"}}
  ],
  "semantics": {"description": "One consumed meal"}
}
```

`sql.execute` takes `store_id`, `expected_revision`, native SQLite `sql`, optional
`parameters` (`null`, `integer`, `real`, `text`, `blob`) and an idempotency key for
writes/schema changes. Use generated physical names returned by discovery.
Successful mutations advance the revision; refresh descriptions after a
stale-revision error or schema drift. Missing/invalidated meaning stays unknown.
`sql.store.update_metadata` addresses stable object/column IDs at a revision.

Reuse a key only with the exact same request. For an uncertain dispatch outcome,
inspect `sql.operation.status` by its returned operation ID rather than submitting
the write under a fresh key. Results and schema descriptions are bounded.

## Human approval

Destructive effects, including indirect deletion by triggers, cascades or
conflict handling, require an immutable operation-specific approval. The tool
returns `approval_required`, not a successful mutation. In Telegram the connector
shows a private card naming the store, action and consequences, with Confirm,
Cancel and Details buttons. Details retrieves the exact persisted operation,
parameters, limits and technical identity without consuming approval or extending
its expiry. A timeout is shown as an unknown outcome with a status check.
The approval interface uses English; user-authored store names are preserved.

Buttons authenticate the real private-chat owner at click time; forwarded cards,
other users and guessed callbacks grant no permission. Manual commands remain
available in Telegram and the local console:

```text
/sqlapprove <connector-id> <operation-id>
/sqldeny <connector-id> <operation-id>
```

Telegram processes those real human commands before cognition. Forwarded text,
model tool arguments and guessed bus events cannot approve anything. Console
operators use the same commands locally. Approval is single-use, expiring and
bound to the request, parameters, database, principal, schema/metadata and policy.
No transaction is held open while waiting. Changed requests need fresh approval.

Unsupported or unclassifiable SQL effects are rejected, including under approval.
The connector cannot attach another file, load extensions, edit its policy or
execute caller-controlled transaction/session commands. SQLite has no native
`TRUNCATE TABLE`, `DROP SCHEMA` or `DROP DATABASE` in this interface.

A transaction rollback does not undo committed destruction. The audit is not a
backup. Cancellation racing commit may produce a committed or unknown outcome;
never assume dropping a tool future rolled back an action.

## Subagents and limits

A connector grant alone gives a child no SQL stores. Supply `sql_stores` explicitly
in its task. `sql_write` and `sql_schema` default to false; grants cannot exceed
the parent, create stores, or give a child human approval authority. Deadlines
also bound the delegated authority.

Manifest limits bound stores, rows, bytes, affected effects, query duration and
pending operations. Large results should be inspected through bounded artifacts.
A future backend/backup feature requires separate configuration and support.

## Developing across repositories

All Octo crates use the same immutable Git revision in `Cargo.toml`. Normal
builds need no sibling checkout or local Cargo source overrides.
