//! Declarative deterministic slash-commands (`[commands]` in `albert.toml`). Each
//! `/name` maps to a skill script run via forkd whose JSON stdout is rendered through a
//! `reply` template — answered as a reflex, before (and instead of) the LLM. This is how
//! a user with no agent/LLM turn still drives Albert's existing skills.
//!
//! Security: an `owner_only` command from a non-owner is refused HERE, upstream of any
//! forkd dispatch — the same gate the ACL admin reflex uses. `/help` is owned by the
//! cogitator (it composes the built-in help); this module only supplies the listing of
//! configured commands via [`help_listing`].

use std::collections::HashMap;
use std::time::Duration;

use octo_core::{CogitatorContext, ConnectorId, Envelope, EventKind};
use serde_json::{json, Value};
use tracing::warn;

use crate::acl::is_owner;
use crate::config::CommandSpec;

/// Wall-clock ceiling a single command may hold `respond()` for — matches forkd's own
/// `max_timeout_secs` default. Clamps a misconfigured `timeout_secs` so a stuck run
/// can't block the reflex path indefinitely.
const MAX_TIMEOUT_SECS: u64 = 300;

/// Cap on a rendered reply, in characters. Telegram rejects messages past ~4096; a
/// script dumping large stdout would otherwise fail at the connector. Left below the
/// limit with room for the connector's own framing.
const MAX_REPLY_CHARS: usize = 3500;

/// Handle a declarative command from `text` against the configured command table.
/// Returns the user-facing reply when `text`'s first word is a configured `/command`,
/// else `None` (the caller then falls through to the LLM). `source` is the cogitator's
/// own connector id — the reply address for the forkd round-trip.
pub async fn command(
    source: &ConnectorId,
    commands: &HashMap<String, CommandSpec>,
    text: &str,
    incoming: &Envelope,
    ctx: &CogitatorContext,
) -> Option<String> {
    // Nothing configured → not a command surface at all.
    if commands.is_empty() {
        return None;
    }
    let trimmed = text.trim();
    let (cmd, rest) = match trimmed.split_once(char::is_whitespace) {
        Some((c, a)) => (c, a.trim()),
        None => (trimmed, ""),
    };
    let spec = commands.get(cmd)?;

    if spec.owner_only && !is_owner(incoming) {
        warn!(cmd, source = %incoming.source, "owner-only command from non-owner refused");
        return Some("Эта команда только для владельца.".to_string());
    }

    let args = match build_args(&spec.args, rest) {
        Some(args) => args,
        // A slot carries the user's text but none was given — a usage hint beats handing
        // the script a stray empty token it would choke on.
        None => return Some(format!("Использование: {cmd} <аргумент>")),
    };

    let payload = json!({
        "skill_path": spec.skill_path,
        "interpreter": spec.interpreter,
        "args": args,
        "timeout_secs": spec.timeout_secs,
    });
    let req = Envelope::new(source.clone(), EventKind::new("forkd.run"), payload)
        .with_target(ConnectorId::new("forkd"));
    // Wait a hair past the script's own (clamped) ceiling, so a run that times out still
    // hands us its result instead of us giving up first.
    let budget = Duration::from_secs(spec.timeout_secs.min(MAX_TIMEOUT_SECS).saturating_add(5));
    match ctx.publish_and_await_response(req, budget).await {
        Ok(resp) => Some(render(spec, resp.payload_as::<Value>())),
        Err(e) => Some(format!("Команда не выполнена: {e}")),
    }
}

/// Build the argv for a command: substitute the user's `rest` into every `{arg}` slot.
/// Returns `None` when a slot needs the user's text but `rest` is empty (the caller
/// turns that into a usage hint), so the script never receives a stray empty token.
fn build_args(args: &[String], rest: &str) -> Option<Vec<String>> {
    if rest.is_empty() && args.iter().any(|a| a.contains("{arg}")) {
        return None;
    }
    Some(args.iter().map(|a| a.replace("{arg}", rest)).collect())
}

/// Render forkd's `{exit_code, stdout, stderr, timed_out}` result into a reply, filling
/// the spec's `reply` template from the script's JSON stdout. A timeout or a non-zero
/// exit surfaces as a short error; stdout that isn't a JSON object is echoed raw. The
/// result is capped to Telegram's message ceiling.
fn render(spec: &CommandSpec, result: Option<&Value>) -> String {
    let null = Value::Null;
    let r = result.unwrap_or(&null);
    if r.get("timed_out").and_then(Value::as_bool) == Some(true) {
        return "Команда не успела за отведённое время.".to_string();
    }
    let stdout = r.get("stdout").and_then(Value::as_str).unwrap_or("").trim();
    if r.get("exit_code").and_then(Value::as_i64) != Some(0) {
        let stderr = r.get("stderr").and_then(Value::as_str).unwrap_or("").trim();
        // Prefer stderr, fall back to stdout; show only the last line to keep it short.
        let detail = [stderr, stdout]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("нет вывода");
        let detail = detail.lines().last().unwrap_or(detail);
        return format!("Скрипт вернул ошибку: {detail}");
    }
    let reply = match serde_json::from_str::<Value>(stdout) {
        Ok(data) => fill(&spec.reply, &data),
        // Not a JSON object — echo the raw stdout rather than a template full of dashes.
        Err(_) => stdout.to_string(),
    };
    capped(reply)
}

/// Clamp a reply to Telegram's message ceiling, marking that it was cut.
fn capped(s: String) -> String {
    if s.chars().count() <= MAX_REPLY_CHARS {
        return s;
    }
    let mut out: String = s.chars().take(MAX_REPLY_CHARS).collect();
    out.push_str("… (обрезано)");
    out
}

/// Fill `{field}` / `{nested.field}` placeholders in `tmpl` from `data`. An unresolved
/// path renders as `—`; a `{` with no closing `}` is emitted literally. A filled value
/// is never re-scanned, so a value that itself contains `{...}` is not re-expanded.
fn fill(tmpl: &str, data: &Value) -> String {
    let mut out = String::with_capacity(tmpl.len());
    let mut rest = tmpl;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open + 1..];
        match rest.find('}') {
            Some(close) => {
                out.push_str(&resolve(data, &rest[..close]));
                rest = &rest[close + 1..];
            }
            None => {
                out.push('{');
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Resolve a dotted path (`remaining.kcal`) against `data`. Strings render as-is, numbers
/// and bools via their JSON form; a missing key or a null renders as `—`.
fn resolve(data: &Value, path: &str) -> String {
    let mut cur = data;
    for key in path.trim().split('.') {
        match cur.get(key) {
            Some(v) => cur = v,
            None => return "—".to_string(),
        }
    }
    match cur {
        Value::String(s) => s.clone(),
        Value::Null => "—".to_string(),
        other => other.to_string(),
    }
}

/// The configured-command listing for `/help`: each `/name` with its one-line help,
/// sorted, owner-only marked. `None` when nothing is configured, so the cogitator's
/// built-in help then stands on its own.
pub fn help_listing(commands: &HashMap<String, CommandSpec>) -> Option<String> {
    if commands.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = commands
        .iter()
        .map(|(name, spec)| {
            let help = if spec.help.is_empty() {
                "—"
            } else {
                spec.help.as_str()
            };
            let lock = if spec.owner_only {
                " (только владелец)"
            } else {
                ""
            };
            format!("{name}{lock} — {help}")
        })
        .collect();
    lines.sort();
    Some(format!("Команды:\n{}", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(reply: &str) -> CommandSpec {
        CommandSpec {
            skill_path: "x/scripts/x.py".to_string(),
            interpreter: "python3".to_string(),
            args: vec![],
            reply: reply.to_string(),
            owner_only: false,
            timeout_secs: 15,
            help: String::new(),
        }
    }

    // --- build_args -------------------------------------------------------------

    #[test]
    fn build_args_without_placeholder_ignores_rest() {
        let args = vec!["balance".to_string()];
        assert_eq!(build_args(&args, ""), Some(vec!["balance".to_string()]));
        // Extra text after a no-arg command is simply not consumed.
        assert_eq!(build_args(&args, "junk"), Some(vec!["balance".to_string()]));
    }

    #[test]
    fn build_args_substitutes_every_placeholder() {
        let args = vec![
            "add-weight".to_string(),
            "--kg".to_string(),
            "{arg}".to_string(),
        ];
        assert_eq!(
            build_args(&args, "82.1"),
            Some(vec![
                "add-weight".to_string(),
                "--kg".to_string(),
                "82.1".to_string()
            ])
        );
    }

    #[test]
    fn build_args_missing_required_arg_is_none() {
        let args = vec![
            "add-weight".to_string(),
            "--kg".to_string(),
            "{arg}".to_string(),
        ];
        assert_eq!(build_args(&args, ""), None);
    }

    // --- fill / resolve ---------------------------------------------------------

    #[test]
    fn fill_flat_and_nested_fields() {
        let data = json!({"sum": "17 000 000", "rate": 0.0072, "remaining": {"kcal": 640}});
        assert_eq!(
            fill(
                "На счету {sum} сум, курс {rate}, осталось {remaining.kcal} ккал",
                &data
            ),
            "На счету 17 000 000 сум, курс 0.0072, осталось 640 ккал"
        );
    }

    #[test]
    fn fill_missing_path_renders_dash() {
        let data = json!({"a": 1});
        assert_eq!(fill("{a} {b} {a.deep}", &data), "1 — —");
    }

    #[test]
    fn fill_unclosed_brace_is_literal_and_stops() {
        let data = json!({"a": 1});
        assert_eq!(
            fill("value={a} and a stray {brace", &data),
            "value=1 and a stray {brace"
        );
    }

    #[test]
    fn fill_does_not_reexpand_a_value_containing_braces() {
        let data = json!({"a": "{b}", "b": "SECRET"});
        assert_eq!(fill("{a}", &data), "{b}");
    }

    #[test]
    fn fill_adjacent_placeholders() {
        let data = json!({"a": "X", "b": "Y"});
        assert_eq!(fill("{a}{b}", &data), "XY");
    }

    #[test]
    fn resolve_renders_types_without_quotes() {
        let data = json!({"s": "hi", "n": 42, "f": 1.5, "b": true, "nul": null});
        assert_eq!(resolve(&data, "s"), "hi");
        assert_eq!(resolve(&data, "n"), "42");
        assert_eq!(resolve(&data, "f"), "1.5");
        assert_eq!(resolve(&data, "b"), "true");
        assert_eq!(resolve(&data, "nul"), "—");
        assert_eq!(resolve(&data, "absent"), "—");
    }

    // --- render -----------------------------------------------------------------

    #[test]
    fn render_happy_path_fills_from_stdout_json() {
        let result = json!({
            "exit_code": 0,
            "stdout": "{\"kg\": 82.1, \"trend_kg\": -0.3}",
            "stderr": "",
            "timed_out": false
        });
        assert_eq!(
            render(&spec("вес {kg}, тренд {trend_kg}"), Some(&result)),
            "вес 82.1, тренд -0.3"
        );
    }

    #[test]
    fn render_timed_out_takes_priority_over_exit() {
        // A killed run reports exit_code null + timed_out true — must read as a timeout.
        let result =
            json!({"exit_code": null, "stdout": "", "stderr": "(killed)", "timed_out": true});
        assert_eq!(
            render(&spec("{x}"), Some(&result)),
            "Команда не успела за отведённое время."
        );
    }

    #[test]
    fn render_nonzero_exit_prefers_last_stderr_line() {
        let result = json!({
            "exit_code": 2,
            "stdout": "",
            "stderr": "Traceback ...\nValueError: bad --kg",
            "timed_out": false
        });
        assert_eq!(
            render(&spec("{x}"), Some(&result)),
            "Скрипт вернул ошибку: ValueError: bad --kg"
        );
    }

    #[test]
    fn render_nonzero_exit_falls_back_to_stdout_then_placeholder() {
        let only_stdout =
            json!({"exit_code": 1, "stdout": "boom", "stderr": "", "timed_out": false});
        assert_eq!(
            render(&spec("{x}"), Some(&only_stdout)),
            "Скрипт вернул ошибку: boom"
        );
        let nothing = json!({"exit_code": 1, "stdout": "", "stderr": "", "timed_out": false});
        assert_eq!(
            render(&spec("{x}"), Some(&nothing)),
            "Скрипт вернул ошибку: нет вывода"
        );
    }

    #[test]
    fn render_non_json_stdout_is_echoed_raw() {
        let result =
            json!({"exit_code": 0, "stdout": "plain line\n", "stderr": "", "timed_out": false});
        assert_eq!(render(&spec("ignored {x}"), Some(&result)), "plain line");
    }

    #[test]
    fn render_null_result_is_an_error_not_a_panic() {
        // A dropped/utterly-empty response must not fill the template nor panic.
        assert_eq!(
            render(&spec("{x}"), None),
            "Скрипт вернул ошибку: нет вывода"
        );
    }

    #[test]
    fn render_caps_an_oversized_reply() {
        let big = "x".repeat(MAX_REPLY_CHARS + 500);
        let result = json!({"exit_code": 0, "stdout": big, "stderr": "", "timed_out": false});
        let out = render(&spec("ignored"), Some(&result));
        assert!(out.chars().count() <= MAX_REPLY_CHARS + 12);
        assert!(out.ends_with("(обрезано)"));
    }

    // --- help_listing -----------------------------------------------------------

    #[test]
    fn help_listing_empty_is_none() {
        assert_eq!(help_listing(&HashMap::new()), None);
    }

    #[test]
    fn help_listing_sorts_marks_owner_only() {
        let mut cmds = HashMap::new();
        let mut b = spec("");
        b.help = "баланс".to_string();
        cmds.insert("/balance".to_string(), b);
        let mut w = spec("");
        w.help = "вес".to_string();
        w.owner_only = true;
        cmds.insert("/weight".to_string(), w);

        assert_eq!(
            help_listing(&cmds),
            Some("Команды:\n/balance — баланс\n/weight (только владелец) — вес".to_string())
        );
    }

    #[test]
    fn help_listing_empty_help_renders_dash() {
        let mut cmds = HashMap::new();
        cmds.insert("/ping".to_string(), spec(""));
        assert!(help_listing(&cmds).unwrap().contains("/ping — —"));
    }
}
