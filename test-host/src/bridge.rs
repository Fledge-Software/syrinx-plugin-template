// SPDX-License-Identifier: MPL-2.0
//! The checks Syrinx makes on every message from a plugin view before any WASM call.
//!
//! Order, as note 10 section 4 sets it: origin, generation, size, JSON shape,
//! method, then the pending-request bound.

use std::fmt;

use serde_json::Value;

use crate::state::MAX_MESSAGE_BYTES;

/// The envelope version this host speaks.
pub const BRIDGE_VERSION: u64 = 1;

/// The most requests one view may have waiting.
pub const MAX_PENDING_PER_VIEW: usize = 16;

/// A message from the interface that passed every check.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Request {
        id: u64,
        method: String,
        body: Value,
    },
    Cancel {
        id: u64,
    },
}

/// Why a message was refused. The plugin never sees a refused message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    WrongOrigin,
    StaleView,
    TooLarge,
    NotAnEnvelope(&'static str),
    UnsupportedVersion,
    UndeclaredMethod(String),
    DuplicateId(u64),
    QueueFull,
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongOrigin => write!(f, "the message came from another origin"),
            Self::StaleView => write!(f, "the message came from a closed or reloaded view"),
            Self::TooLarge => write!(f, "the message is over {MAX_MESSAGE_BYTES} bytes"),
            Self::NotAnEnvelope(why) => write!(f, "the message is not a bridge envelope: {why}"),
            Self::UnsupportedVersion => write!(f, "the bridge version is not {BRIDGE_VERSION}"),
            Self::UndeclaredMethod(method) => {
                write!(
                    f,
                    "method {method:?} is not declared for this view in the manifest"
                )
            }
            Self::DuplicateId(id) => write!(f, "request {id} is already pending"),
            Self::QueueFull => write!(f, "the view has too many requests waiting"),
        }
    }
}

impl std::error::Error for BridgeError {}

/// What the host knows about the sending view when a message arrives.
pub struct Sender<'a> {
    pub expected_origin: &'a str,
    pub origin: &'a str,
    pub current_generation: u64,
    pub generation: u64,
    pub declared_methods: &'a [String],
    pub pending: &'a [u64],
}

/// Run every check, in order, on one raw message.
pub fn check(sender: &Sender<'_>, raw: &str) -> Result<Incoming, BridgeError> {
    if sender.origin != sender.expected_origin {
        return Err(BridgeError::WrongOrigin);
    }
    if sender.generation != sender.current_generation {
        return Err(BridgeError::StaleView);
    }
    if raw.len() > MAX_MESSAGE_BYTES {
        return Err(BridgeError::TooLarge);
    }
    let envelope: Value =
        serde_json::from_str(raw).map_err(|_| BridgeError::NotAnEnvelope("not JSON"))?;
    if envelope.get("v").and_then(Value::as_u64) != Some(BRIDGE_VERSION) {
        return Err(BridgeError::UnsupportedVersion);
    }
    let id = envelope
        .get("id")
        .and_then(Value::as_u64)
        .ok_or(BridgeError::NotAnEnvelope("no numeric id"))?;
    match envelope.get("kind").and_then(Value::as_str) {
        Some("request") => check_request(sender, &envelope, id),
        Some("cancel") => Ok(Incoming::Cancel { id }),
        _ => Err(BridgeError::NotAnEnvelope("kind must be request or cancel")),
    }
}

fn check_request(sender: &Sender<'_>, envelope: &Value, id: u64) -> Result<Incoming, BridgeError> {
    let method = envelope
        .get("method")
        .and_then(Value::as_str)
        .ok_or(BridgeError::NotAnEnvelope("no method"))?;
    if !sender
        .declared_methods
        .iter()
        .any(|declared| declared == method)
    {
        return Err(BridgeError::UndeclaredMethod(method.to_owned()));
    }
    if sender.pending.contains(&id) {
        return Err(BridgeError::DuplicateId(id));
    }
    if sender.pending.len() >= MAX_PENDING_PER_VIEW {
        return Err(BridgeError::QueueFull);
    }
    let body = envelope
        .get("body")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));
    Ok(Incoming::Request {
        id,
        method: method.to_owned(),
        body,
    })
}

/// The response envelope for a reply or a plugin error.
pub fn response(id: u64, reply: Result<Value, (&str, String)>) -> String {
    let envelope = match reply {
        Ok(body) => {
            serde_json::json!({ "v": BRIDGE_VERSION, "kind": "response", "id": id, "ok": true, "body": body })
        }
        Err((kind, message)) => serde_json::json!({
            "v": BRIDGE_VERSION, "kind": "response", "id": id, "ok": false,
            "error": { "kind": kind, "message": message },
        }),
    };
    envelope.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://syrinx-plugin.hello.invalid";

    fn sender<'a>(methods: &'a [String], pending: &'a [u64]) -> Sender<'a> {
        Sender {
            expected_origin: ORIGIN,
            origin: ORIGIN,
            current_generation: 2,
            generation: 2,
            declared_methods: methods,
            pending,
        }
    }

    #[test]
    fn a_well_formed_request_passes() {
        let methods = vec!["list-greetings".to_owned()];
        let raw = r#"{"v":1,"kind":"request","id":4,"method":"list-greetings","body":{}}"#;
        let incoming = check(&sender(&methods, &[]), raw).expect("passes");
        assert!(matches!(incoming, Incoming::Request { id: 4, .. }));
    }

    #[test]
    fn each_check_refuses_its_case() {
        let methods = vec!["list-greetings".to_owned()];
        let ok = r#"{"v":1,"kind":"request","id":4,"method":"list-greetings"}"#;
        let mut other_origin = sender(&methods, &[]);
        other_origin.origin = "https://accounts.example.com";
        assert_eq!(check(&other_origin, ok), Err(BridgeError::WrongOrigin));
        let mut stale = sender(&methods, &[]);
        stale.generation = 1;
        assert_eq!(check(&stale, ok), Err(BridgeError::StaleView));
        let huge = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert_eq!(
            check(&sender(&methods, &[]), &huge),
            Err(BridgeError::TooLarge)
        );
        assert!(matches!(
            check(&sender(&methods, &[]), "{"),
            Err(BridgeError::NotAnEnvelope(_))
        ));
        let v2 = ok.replace("\"v\":1", "\"v\":2");
        assert_eq!(
            check(&sender(&methods, &[]), &v2),
            Err(BridgeError::UnsupportedVersion)
        );
        let undeclared = ok.replace("list-greetings", "read-files");
        assert!(matches!(
            check(&sender(&methods, &[]), &undeclared),
            Err(BridgeError::UndeclaredMethod(_))
        ));
        assert_eq!(
            check(&sender(&methods, &[4]), ok),
            Err(BridgeError::DuplicateId(4))
        );
        let full: Vec<u64> = (100..100 + MAX_PENDING_PER_VIEW as u64).collect();
        assert_eq!(
            check(&sender(&methods, &full), ok),
            Err(BridgeError::QueueFull)
        );
    }

    #[test]
    fn responses_carry_the_interface_id() {
        let ok = response(9, Ok(serde_json::json!({"n": 1})));
        assert_eq!(
            ok,
            r#"{"body":{"n":1},"id":9,"kind":"response","ok":true,"v":1}"#
        );
        let failed = response(9, Err(("not-found", "gone".into())));
        assert!(failed.contains(r#""ok":false"#) && failed.contains("not-found"));
    }
}
