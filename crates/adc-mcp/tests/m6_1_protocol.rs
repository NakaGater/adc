//! M6-1 受入テスト(プロトコル層): stdioで initialize → tools/list(6ツール)→
//! tools/call の往復(実プロセス起動、newline-delimited JSON-RPC)。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

fn fixture_path() -> String {
    let dir = std::env::temp_dir().join("adc-mcp-proto");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("design.ron");
    std::fs::write(
        &path,
        r#"Design(
    schema_version: "0.1",
    intent: "protocol fixture",
    params: [],
    materials: [Material(id: "a5052", density_g_cm3: 2.68, name: "A5052")],
    parts: [
        Part(id: "plate", material: "a5052", process: Machining,
            features: [Block(id: "base", x: 30.0, y: 20.0, z: 4.0)],
            anchors: []),
    ],
    assertions: [Assertion(id: "a_ok", check: BoundingBox(part: "plate", max: (40.0, 30.0, 5.0)), rationale: "r0")],
    rationales: [Rationale(id: "r0", author: Human("t"), basis: Assumption, note: "", timestamp: "2026-07-12T00:00:00Z")],
)"#,
    )
    .unwrap();
    path.to_str().unwrap().to_string()
}

fn send(child: &mut Child, msg: &serde_json::Value) {
    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "{msg}").unwrap();
    stdin.flush().unwrap();
}

/// id一致の応答が来るまで行を読む
fn recv(reader: &mut BufReader<std::process::ChildStdout>, id: u64) -> serde_json::Value {
    for _ in 0..50 {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            panic!("サーバーがEOF");
        }
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line.trim()).expect("JSON-RPC行");
        if v["id"] == id {
            return v;
        }
    }
    panic!("id={id} の応答が来ない");
}

#[test]
fn stdio_round_trip_lists_six_tools_and_calls_design_read() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_adc-mcp"))
        .args(["--design", &fixture_path()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("adc-mcp起動");
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    // initialize
    send(
        &mut child,
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"m6-1-test","version":"0"}}}),
    );
    let init = recv(&mut reader, 1);
    assert!(init["result"]["capabilities"]["tools"].is_object(), "{init}");
    assert!(
        init["result"]["instructions"].as_str().unwrap().contains("非gated"),
        "gated用途限定の案内: {init}"
    );
    send(&mut child, &serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}));

    // tools/list → 6ツール
    send(&mut child, &serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let list = recv(&mut reader, 2);
    let tools: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in [
        "design_read",
        "design_patch",
        "build_and_check",
        "evidence_query",
        "narrow_param",
        "explain",
    ] {
        assert!(tools.contains(&expected), "{expected} がない: {tools:?}");
    }
    assert_eq!(tools.len(), 6, "{tools:?}");

    // tools/call design_read
    send(
        &mut child,
        &serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
            "name":"design_read","arguments":{}}}),
    );
    let call = recv(&mut reader, 3);
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).expect("JSONペイロード");
    assert_eq!(payload["valid"], true, "{payload}");
    assert_eq!(payload["sha256"].as_str().unwrap().len(), 64);

    // tools/call build_and_check → 全Pass
    send(
        &mut child,
        &serde_json::json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{
            "name":"build_and_check","arguments":{}}}),
    );
    let call = recv(&mut reader, 4);
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(payload["exit_code"], 0, "{payload}");
    assert_eq!(payload["results"][0]["assert_id"], "a_ok");

    drop(child.stdin.take());
    let _ = child.wait();
}
