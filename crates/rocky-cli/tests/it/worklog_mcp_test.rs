//! `mcp worklog` 표면 가드 — TS `src/index.test.ts` 의 도구 목록 검사 대응.

use rocky_cli::worklog_mcp::WorklogMcp;

#[test]
fn exposes_exactly_the_four_worklog_tools() {
    let mut names = WorklogMcp::tool_names();
    names.sort();
    assert_eq!(
        names,
        vec![
            "worklog_append",
            "worklog_read",
            "worklog_search",
            "worklog_status"
        ]
    );
}

/// 채널 선언 — `capabilities.experimental["claude/channel"]` 이 있어야 Claude Code 가 알림
/// 리스너를 건다. 도구 표면(4개)은 그대로다.
#[test]
fn declares_the_claude_channel_capability_and_instructions() {
    use rmcp::ServerHandler;
    use rocky_cli::channel::{channel_notification, CHANNEL_CAPABILITY, CHANNEL_METHOD};
    let dir = tempfile::tempdir().unwrap();
    let server = WorklogMcp::new(rocky_core::worklog::Worklog::from_env(
        Some(dir.path().to_string_lossy().as_ref()),
        None,
        Some(dir.path().to_path_buf()),
    ));
    let info = server.get_info();
    let experimental = info.capabilities.experimental.expect("experimental");
    assert!(experimental.contains_key(CHANNEL_CAPABILITY));
    assert!(info.capabilities.tools.is_some(), "도구는 그대로");
    assert!(info.instructions.unwrap().contains("<channel"));
    let n = channel_notification(
        "#3 머지 후보",
        &std::collections::BTreeMap::from([("kind".to_string(), "ready".to_string())]),
    );
    let wire = serde_json::to_value(&n).unwrap();
    assert_eq!(wire["method"], CHANNEL_METHOD);
    assert_eq!(wire["params"]["content"], "#3 머지 후보");
    assert_eq!(wire["params"]["meta"]["kind"], "ready");
}

/// `--roots` 시험용 클라이언트 — 서버와 JSON-RPC 줄을 직접 주고받는다(rmcp client 기능을 켜지 않으려고).
struct RawClient {
    lines: tokio::io::Lines<tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>>,
    write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    /// 2026-07-28 수명주기면 요청마다 싣는 `_meta`(capability 가 여기로 온다).
    meta: Option<serde_json::Value>,
}

/// 클라이언트가 세션을 여는 방식.
#[derive(Clone, Copy)]
enum Lifecycle {
    /// `initialize` 한 번에 capability 를 알린다.
    Initialize,
    /// `server/discover` 로 열고 capability 는 요청마다 `_meta` 로 — Antigravity 가 이렇게 연다.
    Discover,
}

impl RawClient {
    /// `ProjectSource::Roots` 서버를 띄우고 세션을 연다.
    async fn start(lifecycle: Lifecycle, capabilities: serde_json::Value) -> Self {
        use rmcp::ServiceExt;
        use rocky_cli::worklog_mcp::{worklog_for_cwd, ProjectSource};
        use tokio::io::AsyncBufReadExt;
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = WorklogMcp::with_source(worklog_for_cwd(), ProjectSource::Roots);
        tokio::spawn(async move {
            if let Ok(service) = server.serve(tokio::io::split(server_io)).await {
                let _ = service.waiting().await;
            }
        });
        let (read, write) = tokio::io::split(client_io);
        let mut client = RawClient {
            lines: tokio::io::BufReader::new(read).lines(),
            write,
            meta: None,
        };
        let client_info = serde_json::json!({"name": "roots-test", "version": "0"});
        match lifecycle {
            Lifecycle::Initialize => {
                client
                    .send(
                        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                        "params": {
                            "protocolVersion": "2025-06-18",
                            "capabilities": capabilities,
                            "clientInfo": client_info
                        }}),
                    )
                    .await;
                assert_eq!(client.next().await["id"], 1);
                client
                    .send(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                    .await;
            }
            Lifecycle::Discover => {
                let meta = serde_json::json!({
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": capabilities,
                    "io.modelcontextprotocol/clientInfo": client_info
                });
                client
                    .send(
                        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover",
                        "params": {"_meta": meta}}),
                    )
                    .await;
                let opened = client.next().await;
                assert!(opened.get("result").is_some(), "discover 실패: {opened}");
                client.meta = Some(meta);
            }
        }
        client
    }

    async fn send(&mut self, message: serde_json::Value) {
        use tokio::io::AsyncWriteExt;
        let mut line = message.to_string();
        line.push('\n');
        self.write.write_all(line.as_bytes()).await.unwrap();
    }

    async fn next(&mut self) -> serde_json::Value {
        let line = tokio::time::timeout(std::time::Duration::from_secs(10), self.lines.next_line())
            .await
            .expect("서버가 10초 안에 답하지 않았다")
            .unwrap()
            .expect("서버가 연결을 닫았다");
        serde_json::from_str(&line).unwrap()
    }

    async fn call_status(&mut self) {
        let mut params = serde_json::json!({"name": "worklog_status", "arguments": {}});
        if let Some(meta) = &self.meta {
            params["_meta"] = meta.clone();
        }
        self.send(serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": params}))
            .await;
    }
}

/// 도구 결과(`content[0].text`)와 에러 여부.
fn tool_text(response: &serde_json::Value) -> (bool, String) {
    let result = &response["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    (result["isError"] == true, text.to_string())
}

/// Antigravity 처럼 서버를 플러그인 폴더에서 띄우는 호스트 — 프로젝트는 클라이언트가 알려 준 root 다.
/// 두 수명주기 모두에서 같아야 한다(agy 는 `server/discover` 로 연다).
#[tokio::test]
async fn roots_mode_keys_the_worklog_by_the_client_root() {
    for lifecycle in [Lifecycle::Initialize, Lifecycle::Discover] {
        let base = tempfile::tempdir().unwrap();
        let project = base.path().join("agy root");
        std::fs::create_dir(&project).unwrap();
        let uri = format!("file://{}", project.display()).replace(' ', "%20");

        let mut client = RawClient::start(
            lifecycle,
            serde_json::json!({"roots": {"listChanged": true}}),
        )
        .await;
        client.call_status().await;
        let request = client.next().await;
        assert_eq!(
            request["method"], "roots/list",
            "도구 호출 전에 작업 폴더를 묻는다"
        );
        client
            .send(serde_json::json!({"jsonrpc": "2.0", "id": request["id"],
                "result": {"roots": [{"uri": uri}]}}))
            .await;
        let response = client.next().await;
        assert_eq!(response["id"], 2);
        let (is_error, text) = tool_text(&response);
        assert!(!is_error, "{text}");
        let status: serde_json::Value = serde_json::from_str(&text).unwrap();
        let key = status["projectKey"].as_str().unwrap();
        assert!(
            key.starts_with("agy-root-"),
            "프로젝트 키가 root 폴더 이름이 아니다: {key}"
        );
    }
}

/// roots 를 모르는 클라이언트면 cwd 칸에 쓰지 않고 에러로 끝낸다.
#[tokio::test]
async fn roots_mode_refuses_without_roots_capability() {
    for lifecycle in [Lifecycle::Initialize, Lifecycle::Discover] {
        let mut client = RawClient::start(lifecycle, serde_json::json!({})).await;
        client.call_status().await;
        let response = client.next().await;
        assert_eq!(response["id"], 2, "roots/list 를 묻지 않고 바로 답한다");
        let (is_error, text) = tool_text(&response);
        assert!(is_error);
        assert!(text.contains("roots capability"), "{text}");
    }
}
