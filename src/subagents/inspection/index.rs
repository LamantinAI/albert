use std::path::Path;

use rig::completion::Message;
use serde_json::{json, Value};

use super::{
    artifacts::{clip, materialize},
    records::{records, Record},
    Settings,
};

pub struct IndexRequest<'a> {
    pub run_id: &'a str,
    pub run: Value,
    pub offset: usize,
    pub limit: Option<usize>,
    pub inline_bytes: usize,
}

/// Fit the complete run/index response before the generic artifact wrapper.
/// Reducing a page changes its real cursor, never just its visual preview.
pub fn inspect_run(
    messages: &[Message],
    root: Option<&Path>,
    settings: &Settings,
    request: IndexRequest<'_>,
) -> Result<Value, String> {
    let requested = request.limit.unwrap_or(settings.page_size);
    if requested == 0 || settings.page_size == 0 {
        return Err("inspect limit must be positive".into());
    }
    if request.inline_bytes < 1024 {
        return Err("Inspection inline budget is below the configured supported minimum.".into());
    }
    let limit = requested.min(settings.page_size);
    let view_settings = Settings {
        preview_chars: settings
            .preview_chars
            .min((request.inline_bytes / 8).max(1)),
        ..settings.clone()
    };
    let records = records(messages);
    let total = records.len();
    let mut response = json!({"run":request.run,"journal":{"offset":request.offset,"requested_limit":requested,"effective_limit":limit,"returned":0,"entries":[],"total":total,
        "next_offset":null,"has_more":false,"byte_limited":false,"inline_bytes":request.inline_bytes}});
    // Run identity/status/result metadata stays visible; no assignment/context or
    // full child answer belongs in this envelope.
    if bytes(&response) > request.inline_bytes / 2 {
        let run = &response["run"];
        response["run"] = json!({"run_id":request.run_id,"status":run["status"],"result_available":run["result_available"],"tool_calls":run["tool_calls"],"metadata_truncated":true});
    }
    let mut returned = 0;
    for (index, record) in records.iter().enumerate().skip(request.offset).take(limit) {
        let detailed = entry(record, index, request.run_id, root, &view_settings, false);
        response["journal"]["entries"]
            .as_array_mut()
            .unwrap()
            .push(detailed);
        update(&mut response, request.offset, returned + 1, total);
        if bytes(&response) > request.inline_bytes {
            response["journal"]["entries"].as_array_mut().unwrap().pop();
            response["journal"]["byte_limited"] = true.into();
            if returned == 0 {
                // An oversized first record still has a small consumable index
                // entry and canonical read reference, rather than an empty loop.
                response["journal"]["entries"]
                    .as_array_mut()
                    .unwrap()
                    .push(entry(
                        record,
                        index,
                        request.run_id,
                        None,
                        &view_settings,
                        true,
                    ));
                update(&mut response, request.offset, 1, total);
                if bytes(&response) > request.inline_bytes {
                    return Err(
                        "Run identity/index metadata exceeds the configured inline budget.".into(),
                    );
                }
                returned = 1;
            }
            break;
        }
        returned += 1;
    }
    update(&mut response, request.offset, returned, total);
    if bytes(&response) > request.inline_bytes {
        return Err("Inspection index exceeds the configured inline budget.".into());
    }
    Ok(response)
}
fn bytes(value: &Value) -> usize {
    value.to_string().len()
}
fn update(response: &mut Value, offset: usize, returned: usize, total: usize) {
    let next = offset.saturating_add(returned);
    let more = next < total;
    response["journal"]["returned"] = returned.into();
    response["journal"]["has_more"] = more.into();
    response["journal"]["next_offset"] = if more { json!(next) } else { Value::Null };
}
fn status(output: Option<&Value>) -> &'static str {
    let Some(output) = output else {
        return "pending";
    };
    if output.get("status").and_then(Value::as_str) == Some("unknown")
        || output.as_str().is_some_and(|s| s.contains("UNKNOWN"))
    {
        "unknown"
    } else if output.as_str().is_some_and(|s| s.contains("NOT executed")) {
        "not_executed"
    } else if output.get("error").is_some()
        || output.get("status").and_then(Value::as_str) == Some("not_sent")
    {
        "error"
    } else {
        "returned"
    }
}
fn read_ref(run: &str, index: usize, output: &Value) -> Value {
    let nested = output.get("result").filter(|value| value.is_object());
    let body = nested.unwrap_or(output);
    let selected = ["text", "html", "error"].into_iter().find(|key| {
        body.get(*key)
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
    });
    let field: Vec<_> = selected
        .map(|key| {
            if nested.is_some() {
                vec!["result", key]
            } else {
                vec![key]
            }
        })
        .unwrap_or_default();
    json!({"action":"read","run_id":run,"entry":index,"part":"result","field":field,"offset":0})
}
fn entry(
    record: &Record,
    index: usize,
    run: &str,
    root: Option<&Path>,
    settings: &Settings,
    compact: bool,
) -> Value {
    let output = record.output.as_ref().unwrap_or(&Value::Null);
    let mut result = json!({"entry":index,"tool":clip(&record.tool,if compact {32} else {128}),"status":status(record.output.as_ref()),
        "result_bytes":output.to_string().len(),"read_ref":read_ref(run,index,output)});
    if compact {
        result["metadata_truncated"] = true.into();
        return result;
    }
    let view = |value: &Value| match root {
        Some(root) => materialize(value, root, settings),
        None => json!({"note":"Payload remains in the journal; use subagent read."}),
    };
    let payload = view(output);
    let arguments = view(&record.arguments);
    let body = output.get("result").unwrap_or(output);
    result["result_metadata"] = json!({"status":body.get("status").and_then(Value::as_str).map(|s|clip(s,96)),"error":body.get("error").and_then(Value::as_str).map(|s|clip(s,96)),
        "url":body.get("url").and_then(Value::as_str).map(|s|clip(s,96)),"text_chars":body.get("text").and_then(Value::as_str).map(|s|s.chars().count()),"html_bytes":body.get("html").and_then(Value::as_str).map(str::len)});
    result["id"] = clip(&record.id, 128).into();
    result["call_id"] = json!(record.call_id.as_ref().map(|id| clip(id, 128)));
    result["arguments_preview"] =
        clip(&record.arguments.to_string(), settings.preview_chars).into();
    result["result_preview"] = clip(&output.to_string(), settings.preview_chars).into();
    result["arguments_ref"] = json!({"action":"read","run_id":run,"entry":index,"part":"arguments","field":[],"offset":0});
    for (name, value) in [
        ("arguments_path", arguments.get("path")),
        ("result_path", payload.get("path")),
        ("files", payload.get("files")),
        ("artifact_error", payload.get("artifact_error")),
    ] {
        result[name] = value.cloned().unwrap_or(Value::Null);
    }
    result
}
