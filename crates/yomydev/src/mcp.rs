use std::io::{self, BufRead, Read, Write};

use serde_json::{Map, Value, json};

use crate::verdict::{self, Verdict};

const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_LINE: usize = 64 * 1024;
const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const SERVER_NOT_INITIALIZED: i32 = -32002;
const TOOL: &str = "presence";
const TITLE: &str = "Owner presence";
const DESCRIPTION: &str = "Report whether the owner is at this Mac: present, absent, or unknown. \
Checks screen lock, display sleep, and input idle time, and only when those are inconclusive \
analyses a few camera frames in memory. Treat unknown as present.";
const INSTRUCTIONS: &str = "yomydev reports whether the owner is physically at this Mac. \
Call presence only when you are about to stop and wait for the owner's answer and have reason to \
think they are away. Do not poll it, because a call may briefly turn on the camera. \
When it reports present or unknown, ask as usual and wait. \
When it reports absent, decide only what is low-risk, reversible, and within the task's agreed \
scope by taking the option you would have recommended, keep working, and list each such decision \
with its reason in your reply so the owner can review or undo it. \
Absence never grants authority. Even when the owner is absent, wait for their explicit request \
before you commit, push, merge, reply to or resolve pull request threads, delete files or data, \
or take any action visible outside this machine, and never widen the scope or choose an \
irreversible option because they are away. \
A result describes one moment, so check again before the next question you would wait on. \
Camera frames are analysed in memory and never stored.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Negotiated,
    Ready,
}

#[derive(Debug)]
struct Request {
    id: Option<Value>,
    method: String,
    params: Map<String, Value>,
}

fn error(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn result(id: &Value, value: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": value})
}

fn parse(line: &[u8]) -> Result<Request, Value> {
    let invalid = || error(&Value::Null, INVALID_REQUEST, "Invalid Request");
    let value: Value = serde_json::from_slice(line)
        .map_err(|_malformed| error(&Value::Null, PARSE_ERROR, "Parse error"))?;
    let Value::Object(mut fields) = value else {
        return Err(invalid());
    };
    if fields.get("jsonrpc") != Some(&json!("2.0")) {
        return Err(invalid());
    }
    let id = fields.remove("id");
    if id
        .as_ref()
        .is_some_and(|id| !id.is_string() && !id.is_i64() && !id.is_u64())
    {
        return Err(invalid());
    }
    let Some(Value::String(method)) = fields.remove("method") else {
        return Err(invalid());
    };
    let params = match fields.remove("params") {
        None => Map::new(),
        Some(Value::Object(params)) => params,
        Some(_) => return Err(invalid()),
    };
    Ok(Request { id, method, params })
}

fn tool() -> Value {
    json!({
        "name": TOOL,
        "title": TITLE,
        "description": DESCRIPTION,
        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
        "outputSchema": verdict::schema(),
        "annotations": {
            "title": TITLE,
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false,
        },
    })
}

fn initialize(phase: &mut Phase, id: &Value, params: &Map<String, Value>) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let valid = requested.is_some()
        && params.get("capabilities").is_some_and(Value::is_object)
        && params
            .get("clientInfo")
            .and_then(|client| client.get("name"))
            .is_some_and(Value::is_string);
    if !valid {
        return error(id, INVALID_PARAMS, "Invalid initialization");
    }
    *phase = Phase::Negotiated;
    let version = match requested {
        Some(requested) if VERSIONS.contains(&requested) => requested,
        Some(_) | None => VERSIONS[0],
    };
    result(
        id,
        &json!({
            "protocolVersion": version,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "yomydev", "version": env!("CARGO_PKG_VERSION")},
            "instructions": INSTRUCTIONS,
        }),
    )
}

fn call(id: &Value, params: &Map<String, Value>, observe: &mut impl FnMut() -> Verdict) -> Value {
    if params.get("name").and_then(Value::as_str) != Some(TOOL) {
        return error(id, INVALID_PARAMS, "Unknown tool");
    }
    match params.get("arguments") {
        None => {},
        Some(Value::Object(arguments)) if arguments.is_empty() => {},
        Some(_) => return error(id, INVALID_PARAMS, "presence takes no arguments"),
    }
    let value = observe().to_json();
    result(
        id,
        &json!({
            "content": [{"type": "text", "text": value.to_string()}],
            "structuredContent": value,
            "isError": false,
        }),
    )
}

fn respond(
    phase: &mut Phase,
    request: &Request,
    id: &Value,
    observe: &mut impl FnMut() -> Verdict,
) -> Value {
    match (request.method.as_str(), *phase) {
        ("initialize", Phase::New) => initialize(phase, id, &request.params),
        ("initialize", Phase::Negotiated | Phase::Ready) => {
            error(id, INVALID_REQUEST, "Already initialized")
        },
        ("ping", Phase::New | Phase::Negotiated | Phase::Ready) => result(id, &json!({})),
        ("tools/list", Phase::Ready) => result(id, &json!({"tools": [tool()]})),
        ("tools/call", Phase::Ready) => call(id, &request.params, observe),
        ("tools/list" | "tools/call", Phase::New | Phase::Negotiated) => {
            error(id, SERVER_NOT_INITIALIZED, "Server not initialized")
        },
        (_, Phase::New | Phase::Negotiated | Phase::Ready) => {
            error(id, METHOD_NOT_FOUND, "Method not found")
        },
    }
}

fn read_line(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let limit = u64::try_from(MAX_LINE)
        .map_err(io::Error::other)?
        .saturating_add(1);
    let mut line = Vec::new();
    if input.take(limit).read_until(b'\n', &mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(line))
}

fn send(output: &mut impl Write, value: &Value) -> io::Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")?;
    output.flush()
}

pub(crate) fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    mut observe: impl FnMut() -> Verdict,
) -> io::Result<()> {
    let mut phase = Phase::New;
    while let Some(line) = read_line(&mut input)? {
        if line.len() > MAX_LINE {
            send(
                &mut output,
                &error(&Value::Null, INVALID_REQUEST, "Request exceeds 64 KiB"),
            )?;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized MCP request",
            ));
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let request = match parse(&line) {
            Ok(request) => request,
            Err(reply) => {
                send(&mut output, &reply)?;
                continue;
            },
        };
        match &request.id {
            Some(id) => send(
                &mut output,
                &respond(&mut phase, &request, id, &mut observe),
            )?,
            None => {
                if request.method == "notifications/initialized" && phase == Phase::Negotiated {
                    phase = Phase::Ready;
                }
            },
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use yomydev_core::{Basis, Idle, IdleSeconds};

    use super::*;

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test"}}}"#;
    const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

    fn run(lines: &[&str]) -> (io::Result<()>, Vec<Value>, usize) {
        let mut input = lines.join("\n");
        input.push('\n');
        let mut output = Vec::new();
        let mut calls = 0_usize;
        let outcome = serve(Cursor::new(input), &mut output, || {
            calls = calls.saturating_add(1);
            Verdict {
                basis: Basis::NobodySeen,
                idle: Idle::Since(IdleSeconds::new(412)),
            }
        });
        let replies = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        (outcome, replies, calls)
    }

    #[test]
    fn initialize_carries_the_absent_owner_policy() {
        let (outcome, replies, _) = run(&[INITIALIZE]);
        outcome.unwrap();
        let reply = &replies[0]["result"];
        assert_eq!(reply["protocolVersion"], "2025-06-18");
        let instructions = reply["instructions"].as_str().unwrap();
        assert!(instructions.contains("Absence never grants authority"));
        assert!(instructions.contains("commit, push, merge"));
    }

    #[test]
    fn an_unsupported_version_gets_the_latest_supported_one() {
        let request = INITIALIZE.replace("2025-06-18", "2099-01-01");
        let (_, replies, _) = run(&[&request]);
        assert_eq!(replies[0]["result"]["protocolVersion"], VERSIONS[0]);
    }

    #[test]
    fn tools_wait_for_the_initialized_notification() {
        let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        let (_, replies, calls) = run(&[INITIALIZE, list]);
        assert_eq!(replies[1]["error"]["code"], SERVER_NOT_INITIALIZED);
        assert_eq!(calls, 0);
    }

    #[test]
    fn the_listed_tool_is_read_only_and_takes_no_input() {
        let list = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        let (_, replies, _) = run(&[INITIALIZE, INITIALIZED, list]);
        let tool = &replies[1]["result"]["tools"][0];
        assert_eq!(tool["name"], TOOL);
        assert_eq!(tool["annotations"]["readOnlyHint"], true);
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert_eq!(tool["outputSchema"], verdict::schema());
    }

    #[test]
    fn calling_presence_observes_once_and_returns_structured_content() {
        let call = r#"{"jsonrpc":"2.0","id":"c","method":"tools/call","params":{"name":"presence","arguments":{}}}"#;
        let (_, replies, calls) = run(&[INITIALIZE, INITIALIZED, call]);
        assert_eq!(calls, 1);
        let reply = &replies[1];
        assert_eq!(reply["id"], "c");
        let expected = json!({"presence": "absent", "basis": "nobody_seen", "idle_seconds": 412});
        assert_eq!(reply["result"]["structuredContent"], expected);
        assert_eq!(reply["result"]["content"][0]["text"], expected.to_string());
        assert_eq!(reply["result"]["isError"], false);
    }

    #[test]
    fn unknown_tools_and_arguments_are_refused_without_observing() {
        let unknown =
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"camera"}}"#;
        let arguments = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"presence","arguments":{"force":true}}}"#;
        let (_, replies, calls) = run(&[INITIALIZE, INITIALIZED, unknown, arguments]);
        assert_eq!(replies[1]["error"]["code"], INVALID_PARAMS);
        assert_eq!(replies[2]["error"]["code"], INVALID_PARAMS);
        assert_eq!(calls, 0);
    }

    #[test]
    fn malformed_requests_get_json_rpc_errors() {
        let (_, replies, _) = run(&[
            "{",
            r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":[]}"#,
            r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"resources/list"}"#,
            r#"{"jsonrpc":"2.0","id":6,"method":"ping"}"#,
        ]);
        let codes: Vec<_> = replies
            .iter()
            .map(|reply| reply["error"]["code"].clone())
            .collect();
        assert_eq!(
            codes,
            [
                json!(PARSE_ERROR),
                json!(INVALID_REQUEST),
                json!(INVALID_REQUEST),
                json!(INVALID_REQUEST),
                json!(METHOD_NOT_FOUND),
                Value::Null,
            ]
        );
        assert_eq!(replies[5]["result"], json!({}));
    }

    #[test]
    fn a_second_initialize_is_refused() {
        let (_, replies, _) = run(&[INITIALIZE, INITIALIZE]);
        assert_eq!(replies[1]["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn an_oversized_line_ends_the_session() {
        let line = "x".repeat(MAX_LINE + 1);
        let (outcome, replies, _) = run(&[&line, INITIALIZE]);
        assert_eq!(outcome.unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["error"]["code"], INVALID_REQUEST);
    }
}
