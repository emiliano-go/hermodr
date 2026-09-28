use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    time::Duration,
};

fn trace(value: &str) {
    use std::fs::OpenOptions;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("trace")
        .unwrap();
    writeln!(file, "{value}").unwrap();
}

fn main() {
    let mode = std::fs::read_to_string("mode").unwrap_or_default();
    trace("start");
    if std::env::vars().any(|(key, _)| key == "POSTAL_PLUGIN_TEST_SECRET") {
        trace("environment leak");
    }
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let value: Value = serde_json::from_str(&line.unwrap()).unwrap();
        match value["type"].as_str().unwrap() {
            "hello" => {
                assert_eq!(value["api_version"], 1);
                assert_eq!(value["capabilities"], json!(["events:read"]));
                trace("hello");
                if mode == "crash" {
                    std::process::exit(7);
                }
                if mode == "slow" {
                    std::thread::sleep(Duration::from_millis(250));
                }
                if mode == "malformed" {
                    println!("not JSON");
                    println!("{}", "x".repeat(1024 * 1024 + 16));
                    println!(
                        "{}",
                        json!({"type":"call", "id":7, "method":"message:send", "args":{}})
                    );
                    eprintln!("synthetic stderr");
                }
                println!("{}", json!({"type":"ready", "name":"Fixture"}));
            }
            "event" => {
                trace(&format!(
                    "event:{}:{}",
                    value["seq"], value["event"]["marker"]
                ));
                if mode == "event-crash" {
                    std::process::exit(9);
                }
                if mode == "no-ack" {
                    continue;
                }
                if mode == "slow-ack" {
                    println!("{}", json!({"type":"ack", "seq":99999}));
                    io::stdout().flush().unwrap();
                    std::thread::sleep(Duration::from_millis(250));
                }
                println!("{}", json!({"type":"ack", "seq":value["seq"]}));
                trace("ack");
            }
            "error" => {
                assert_eq!(value["id"], 7);
                trace("denied");
            }
            "shutdown" => {
                trace("shutdown");
                if mode == "ignore-shutdown" {
                    std::thread::sleep(Duration::from_secs(60));
                }
                break;
            }
            other => panic!("unexpected host message {other}"),
        }
        io::stdout().flush().unwrap();
    }
}
