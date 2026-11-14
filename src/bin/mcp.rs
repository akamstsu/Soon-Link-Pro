//! Standard MCP stdio entry point. Never opens an audio device or starts the GUI.
use soon_link_pro::{mcp_protocol::Session, mcp_transport};
use std::io::{self, BufReader, Write};

fn run() -> io::Result<()> {
    let mut session = Session::default();
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = io::stdout().lock();
    while let Some(line) = mcp_transport::read_frame(&mut input, None)? {
        let reply = match serde_json::from_slice(&line) {
            Ok(message) => session.handle(message, mcp_transport::call),
            Err(_) => Some(serde_json::json!({"jsonrpc":"2.0","id":null,
                "error":{"code":-32700,"message":"Parse error"}})),
        };
        if let Some(reply) = reply {
            serde_json::to_writer(&mut output, &reply)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Soon Link Pro MCP: {error}");
        std::process::exit(1);
    }
}
