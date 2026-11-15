//! A stand-in obs-websocket v5 server for tests: it answers each request with
//! whatever a test's handler returns, so the order of requests `ObsClient`
//! sends (and what it does with OBS's answers) can be checked without OBS.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tungstenite::Message;

/// OBS's answer to one request, and the events it sends right after it.
pub struct Reply {
    ok: bool,
    comment: String,
    data: Value,
    events: Vec<Value>,
}

impl Reply {
    pub fn ok(data: Value) -> Self {
        Self {
            ok: true,
            comment: String::new(),
            data,
            events: Vec::new(),
        }
    }

    pub fn err(comment: &str) -> Self {
        Self {
            ok: false,
            comment: comment.to_string(),
            data: json!({}),
            events: Vec::new(),
        }
    }

    pub fn then_record_state(mut self, state: &str) -> Self {
        self.events.push(json!({
            "eventType": "RecordStateChanged",
            "eventIntent": 64,
            "eventData": { "outputActive": state.ends_with("STARTED"), "outputState": state },
        }));
        self
    }
}

/// A running fake: `url` for `ObsClient::connect`, and the request types it
/// was sent, in order.
pub struct FakeObs {
    pub url: String,
    pub requests: Arc<Mutex<Vec<String>>>,
}

impl FakeObs {
    pub fn sent(&self, request_type: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| *r == request_type)
            .count()
    }
}

/// Serves one connection on a free local port until the client hangs up.
pub fn serve(mut handler: impl FnMut(&str, &Value) -> Reply + Send + 'static) -> FakeObs {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&requests);
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let Ok(mut ws) = tungstenite::accept(stream) else {
            return;
        };
        let send = |ws: &mut tungstenite::WebSocket<_>, v: Value| {
            ws.send(Message::Text(v.to_string().into())).is_ok()
        };
        if !send(&mut ws, json!({ "op": 0, "d": { "rpcVersion": 1 } })) {
            return;
        }
        while let Ok(msg) = ws.read() {
            let Message::Text(text) = msg else {
                continue;
            };
            let Ok(m) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            match m["op"].as_u64() {
                Some(1) => {
                    send(
                        &mut ws,
                        json!({ "op": 2, "d": { "negotiatedRpcVersion": 1 } }),
                    );
                }
                Some(6) => {
                    let d = &m["d"];
                    let kind = d["requestType"].as_str().unwrap_or_default().to_string();
                    log.lock().unwrap().push(kind.clone());
                    let reply = handler(&kind, &d["requestData"]);
                    send(
                        &mut ws,
                        json!({ "op": 7, "d": {
                            "requestType": kind,
                            "requestId": d["requestId"],
                            "requestStatus": {
                                "result": reply.ok,
                                "code": if reply.ok { 100 } else { 500 },
                                "comment": reply.comment,
                            },
                            "responseData": reply.data,
                        }}),
                    );
                    for event in reply.events {
                        send(&mut ws, json!({ "op": 5, "d": event }));
                    }
                }
                _ => {}
            }
        }
    });
    FakeObs { url, requests }
}
