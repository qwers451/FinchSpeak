//! The webview must run under a strict Content-Security-Policy (AUD-032): it renders data that
//! other users wrote (chat messages, names), so an injected script must not be able to run or to
//! talk to the network. All network traffic goes through the Rust side via Tauri IPC.

use std::collections::HashMap;

use serde_json::Value;

fn tauri_conf() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("tauri.conf.json")).expect("valid JSON")
}

fn parse_policy(policy: &str) -> HashMap<String, Vec<String>> {
    policy
        .split(';')
        .filter_map(|directive| {
            let mut parts = directive.split_whitespace();
            let name = parts.next()?.to_string();
            Some((name, parts.map(str::to_string).collect()))
        })
        .collect()
}

fn csp(key: &str) -> HashMap<String, Vec<String>> {
    let conf = tauri_conf();
    let policy = conf["app"]["security"][key]
        .as_str()
        .unwrap_or_else(|| panic!("app.security.{key} must be a CSP string, not null"));
    parse_policy(policy)
}

#[test]
fn production_csp_is_strict() {
    let policy = csp("csp");

    assert_eq!(policy["default-src"], ["'self'"]);
    // No inline script, no eval: an injected <script> or javascript: URL cannot run.
    assert_eq!(policy["script-src"], ["'self'"]);
    assert_eq!(policy["object-src"], ["'none'"]);
    assert_eq!(policy["base-uri"], ["'none'"]);
    assert_eq!(policy["form-action"], ["'none'"]);
    assert_eq!(policy["frame-ancestors"], ["'none'"]);

    // Tauri IPC is the only way out of the webview.
    let connect = &policy["connect-src"];
    assert!(connect.contains(&"ipc:".to_string()), "Tauri IPC needs ipc:");
    assert!(connect.contains(&"http://ipc.localhost".to_string()), "Tauri IPC needs http://ipc.localhost on Windows");

    for (directive, sources) in &policy {
        for source in sources {
            assert!(
                !matches!(source.as_str(), "*" | "http:" | "https:" | "ws:" | "wss:" | "'unsafe-eval'"),
                "{directive} allows {source}"
            );
            if directive != "style-src" {
                assert_ne!(source, "'unsafe-inline'", "{directive} allows inline content");
            }
            assert!(
                !source.starts_with("https://") && !source.starts_with("wss://"),
                "{directive} allows an external origin: {source}"
            );
        }
    }
}

/// The dev server needs its HMR websocket and inline preamble; nothing else is relaxed.
#[test]
fn dev_csp_only_adds_the_dev_server() {
    let policy = csp("devCsp");
    assert_eq!(policy["object-src"], ["'none'"]);
    assert_eq!(policy["frame-ancestors"], ["'none'"]);
    let connect = &policy["connect-src"];
    assert!(connect.contains(&"ws://127.0.0.1:5173".to_string()));
    assert!(connect.contains(&"ipc:".to_string()));
    assert!(!connect.iter().any(|s| s == "*" || s == "https:" || s == "http:"));
}

/// A strict policy is pointless if the page itself pulls resources from the internet.
#[test]
fn frontend_page_loads_nothing_from_the_network() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/frontend/index.html");
    let html = std::fs::read_to_string(path).expect("index.html");
    for attribute in ["href=\"http", "src=\"http", "href='http", "src='http"] {
        assert!(!html.contains(attribute), "index.html references an external resource ({attribute})");
    }
}
