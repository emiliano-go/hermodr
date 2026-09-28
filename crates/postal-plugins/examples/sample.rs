use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let message: Value = serde_json::from_str(&line?)?;
        match message["type"].as_str() {
            Some("hello") if message["api_version"] == 1 => {
                println!("{}", json!({"type":"ready", "name":"Sample Plugin"}));
            }
            Some("event") => {
                println!(
                    "{}",
                    json!({"type":"log", "level":"info", "message":format!("Received {}", message["event"]["kind"])})
                );
                println!("{}", json!({"type":"ack", "seq":message["seq"]}));
            }
            Some("shutdown") => break,
            _ => {}
        }
        io::stdout().flush()?;
    }
    Ok(())
}
