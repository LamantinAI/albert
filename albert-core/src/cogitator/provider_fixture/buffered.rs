use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use std::{sync::Arc, sync::Mutex, time::Duration};
use tokio::time::sleep;

pub(super) async fn buffered_completion(
    State(requests): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let model = body["model"].as_str().unwrap();
    let count = {
        let mut requests = requests.lock().unwrap();
        requests.push(body.clone());
        requests.iter().filter(|r| r["model"] == model).count()
    };
    if matches!(model, "credential-handoff" | "credential-cancel") && count >= 2 {
        sleep(Duration::from_millis(250)).await;
    }
    if model.starts_with("title-")
        && body["messages"]
            .to_string()
            .contains("Create a short, meaningful conversation title")
    {
        if model == "title-delayed" {
            sleep(Duration::from_millis(350)).await;
        }
        if model == "title-failing" {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":{"message":"test unavailable"}})),
            );
        }
        return (
            StatusCode::OK,
            Json(
                json!({"id":"title-test","object":"chat.completion","created":0,"model":model,"choices":[{"index":0,"message":{"role":"assistant","content":"SQLite journal recovery"},"finish_reason":"stop"}],"usage":{"prompt_tokens":32,"completion_tokens":6,"total_tokens":38}}),
            ),
        );
    }
    if model == "message-native-policy" && count == 2 {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if model == "message-after-tool" && count > 1 {
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    if model == "nav-feedback-tools" {
        tokio::time::sleep(Duration::from_millis(1200)).await;
    }
    if model.starts_with("never") {
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    if model == "feedback-phases" && count == 1 {
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
    if model == "visibility-native" {
        tokio::time::sleep(Duration::from_millis(350)).await;
    }
    if model == "session-delayed" {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    if model == "compact-delayed"
        && body["tools"]
            .as_array()
            .is_none_or(|tools| tools.is_empty())
    {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    if model == "account-hold" {
        sleep(Duration::from_secs(10)).await;
    }
    if model == "hanging" {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if model == "failing" || (model == "writes" && count > 1) || (model == "recovers" && count == 2)
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"code":503,"message":"overloaded"}})),
        );
    }
    if model == "incompatible" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"code":400,"message":"This model does not support images"}})),
        );
    }
    let (message, finish) = if model == "message-progress" && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"progress","type":"function","function":{"name":"report_progress","arguments":json!({"content":"Deliberate verified stage summary"}).to_string()}}]}),
            "tool_calls",
        )
    } else if (model == "message-native-policy" && matches!(count, 1 | 3))
        || (model == "message-native-delivery" && count == 1)
    {
        let arguments = if count == 1 {
            let wire = body["messages"].to_string();
            let value = wire.split_once("POLICY_TARGET:").unwrap().1;
            let target: String = value
                .chars()
                .take_while(|character| character.is_ascii_hexdigit() || *character == '-')
                .collect();
            let display = if model == "message-native-delivery" {
                json!({"connector":"telegram","channel":"7"})
            } else {
                Value::Null
            };
            json!({"action":"reports","policy":{"revision":1,"intermediate":null,"final_result":{"kind":"fixed_session","session":target,"display":display}}})
        } else {
            json!({"action":"reports"})
        };
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("reports-{count}"),"type":"function","function":{"name":"session_messaging","arguments":arguments.to_string()}}]}),
            "tool_calls",
        )
    } else if matches!(model, "sql-create" | "sql-list") && count == 1 {
        let (kind, payload) = if model == "sql-create" {
            (
                "sql.store.create",
                json!({"name":"root entry","purpose":"integration test","idempotency_key":"root-create"}),
            )
        } else {
            ("sql.store.list", json!({}))
        };
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"sql-dispatch","type":"function","function":{
                "name":"dispatch_to_connector","arguments":json!({"target":"sql","kind":kind,"payload":payload}).to_string()
            }}]}),
            "tool_calls",
        )
    } else if model == "mcp-native" && count == 1 {
        let name = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .find(|name| name.starts_with("mcp_"))
            .expect("native MCP definition");
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"mcp-call","type":"function","function":{"name":name,"arguments":"{\"version\":0}"}}]}),
            "tool_calls",
        )
    } else if model.starts_with("confirmation-") && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[
                {"id":"human-question","type":"function","function":{"name":"request_confirmation","arguments":json!({"title":"Continue the review?","action":"Review the proposed change","effects":["Start the next phase"]}).to_string()}},
                {"id":"blocked-write","type":"function","function":{"name":"write","arguments":json!({"path":"should-not-run.txt","content":"This sibling must not execute"}).to_string()}}
            ]}),
            "tool_calls",
        )
    } else if matches!(model, "credential-handoff" | "credential-cancel") && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"credential-request","type":"function","function":{"name":"request_credential","arguments":"{\"name\":\"EXPECTED_TOKEN\"}"}}]}),
            "tool_calls",
        )
    } else if model == "waiting-parent" && count == 1 {
        let wire = body["messages"].to_string();
        let rest = wire.split_once("WAIT_RUN:").unwrap().1.trim_start();
        let run_id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || "/-_".contains(*c))
            .collect();
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"wait-child","type":"function","function":{
                "name":"subagent","arguments":json!({"action":"wait","run_id":run_id,"seconds":60}).to_string()
            }}]}),
            "tool_calls",
        )
    } else if matches!(model, "budget-aware" | "budget-defiant")
        && (model == "budget-defiant"
            || body["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty()))
    {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("budget-{count}"),"type":"function","function":{
                "name":"scratchpad_note","arguments":json!({"text":format!("evidence-{count}")}).to_string()
            }}]}),
            "tool_calls",
        )
    } else if model == "delegates" && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"spawn-child","type":"function","function":{
                "name":"subagent","arguments":json!({"action":"spawn","task":{"task":"Write a short answer","context":"Only this explicit context","models":["healthy"],"connectors":[],"tools":[]}}).to_string()
            }}]}),
            "tool_calls",
        )
    } else if matches!(
        model,
        "child-dispatch"
            | "large-output"
            | "volatile-output"
            | "message-after-tool"
            | "nav-feedback-tools"
    ) && count == 1
    {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"child-search","type":"function","function":{
                "name":"dispatch_to_connector","arguments":json!({"target":"search","kind":"search.web","payload":{"query":"test"}}).to_string()
            }}]}),
            "tool_calls",
        )
    } else if model == "large-output" && count == 2 {
        let result = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["role"] == "tool")
            .unwrap();
        let content: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
        assert_eq!(content["offloaded"], true);
        assert!(result.to_string().len() < 14000);
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"read-evidence","type":"function","function":{"name":"artifact","arguments":json!({"action":"read","id":content["artifact_id"],"field":["result","text"],"offset":20000,"limit":100}).to_string()}}]}),
            "tool_calls",
        )
    } else if model == "discovery" && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"find-connector","type":"function","function":{"name":"connector_search","arguments":"{\"query\":\"inventory\"}"}}]}),
            "tool_calls",
        )
    } else if matches!(model, "file-dispatch" | "file-native") && count == 1 {
        let (name, args) = if model == "file-dispatch" {
            (
                "dispatch_to_connector",
                json!({"target":"telegram","kind":"chat.send_file","payload":{"path":"photo.jpg"}}),
            )
        } else {
            ("send_file", json!({"path":"photo.jpg"}))
        };
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"file-call","type":"function","function":{"name":name,"arguments":args.to_string()}}]}),
            "tool_calls",
        )
    } else if model == "switches" && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_select","type":"function","function":{"name":"model_select","arguments":"{\"model_id\":\"healthy\"}"}}]}),
            "tool_calls",
        )
    } else if model == "writes" || (model == "recovers" && count == 1) {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_note","type":"function","function":{"name":"scratchpad_note","arguments":"{\"text\":\"recorded once\"}"}}]}),
            "tool_calls",
        )
    } else {
        (
            json!({"role":"assistant","content":if model == "empty" { " ".to_string() } else if model == "visibility-native" { if count == 1 { "Native full report with table\n\n| Result | Evidence |\n| --- | --- |\n| Main | preserved |".to_owned() } else { "Short inter-session response".to_owned() } } else { format!("answer from {model}") }}),
            "stop",
        )
    };
    (
        StatusCode::OK,
        Json(
            json!({"id":"test-completion","object":"chat.completion","created":1,"model":model,
            "choices":[{"index":0,"message":message,"finish_reason":finish}],
            "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}),
        ),
    )
}
