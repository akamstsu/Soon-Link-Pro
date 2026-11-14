//! Exercise the shipped stdio entry with an isolated mock workbench. Never read
//! the real profile, start the GUI, enumerate devices, or open an audio stream.
use serde_json::{json, Value};
use soon_link_pro::mcp_transport::read_frame;
use std::{
    io::{BufReader, Write},
    net::TcpListener,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[test]
fn stdio_session_connects_only_to_the_isolated_mock_workbench() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let root = std::env::temp_dir().join(format!(
        "soon-link-mcp-stdio-{}-{}",
        std::process::id(),
        listener.local_addr().unwrap().port()
    ));
    let profile = root.join("SoonLinkPro");
    std::fs::create_dir_all(&profile).unwrap();
    let token = "ab".repeat(32);
    std::fs::write(
        profile.join("mcp-endpoint.json"),
        serde_json::to_vec(&json!({"port":listener.local_addr().unwrap().port(),"token":token}))
            .unwrap(),
    )
    .unwrap();
    let mock = thread::spawn(move || {
        let mut received = Vec::new();
        let limit = Instant::now() + Duration::from_secs(10);
        while received.len() < 2 && Instant::now() < limit {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let request: Value = serde_json::from_slice(
                        &read_frame(&mut BufReader::new(&mut stream), None)
                            .unwrap()
                            .unwrap(),
                    )
                    .unwrap();
                    assert_eq!(request["token"], token);
                    let value = if request["tool"] == "get_routing_state" {
                        json!({"revision":"mock-1","inputs":[],"outputs":[],"routes":[]})
                    } else {
                        assert_eq!(request["arguments"]["expected_revision"], "mock-1");
                        assert_eq!(request["arguments"]["dry_run"], true);
                        json!({"dry_run":true,"applied":false,"revision":"mock-1"})
                    };
                    writeln!(stream, "{}", json!({"Ok":value})).unwrap();
                    received.push(request);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("Mock workbench: {e}"),
            }
        }
        assert_eq!(received.len(), 2);
        received
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_SoonLinkProMcp"))
        .env("LOCALAPPDATA", &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","clientInfo":{"name":"integration-test","version":"1"},"capabilities":{}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_routing_state","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"patch_routing","arguments":{"expected_revision":"mock-1","dry_run":true,"all_muted":true}}}),
    ];
    {
        let mut stdin = child.stdin.take().unwrap();
        for message in messages {
            writeln!(stdin, "{message}").unwrap();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
        panic!("MCP stdio process did not exit after EOF");
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let replies = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 4); // Initialized notification has no response.
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 2);
    assert_eq!(
        replies[2]["result"]["structuredContent"]["revision"],
        "mock-1"
    );
    assert_eq!(replies[3]["result"]["isError"], false);
    assert_eq!(replies[3]["result"]["structuredContent"]["applied"], false);
    mock.join().unwrap();
    std::fs::remove_file(profile.join("mcp-endpoint.json")).unwrap();
    std::fs::remove_dir(profile).unwrap();
    std::fs::remove_dir(root).unwrap();
}
