//! Dev-loop bridge: connects the running app (outbound) to the hosted relay so
//! Claude can drive + observe Act mode during development.
//!
//! This is a DEVELOPMENT tool. It is compiled in, but only *starts* when both
//! (a) this is a debug build and (b) `FLOWRAD_DEVLOG_RELAY` + `FLOWRAD_DEVLOG_TOKEN`
//! are set — so it is inert in release and unless explicitly opted into.
//!
//! It connects out to `{FLOWRAD_DEVLOG_RELAY}/agent?token=…` (a WebSocket), sends
//! a `hello`, then:
//!   - forwards each `command {action:"act", text}` from the relay by running
//!     `text` through the live Conductor (as if spoken), streaming the resulting
//!     `ActEvent`s back up as `event`s (tagged with the command id) and a final
//!     `reply`;
//!   - the same events are emitted to the HUD, so a driven command looks normal.
//!
//! SECURITY: `send_input` on the relay drives the user's computer. The token is a
//! shared secret; the bridge is dev-only. The events are Act's PHI-free labels
//! (never raw values), same discipline as the rest of Act.

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::act::events::{ActEvent, ACT_EVENT};
use crate::commands::act::ActState;

/// Start the dev relay bridge if opted in (debug build + env vars). No-op
/// otherwise. Returns immediately; the connection runs on a background task with
/// reconnect.
pub fn start_if_enabled(app: &AppHandle) {
    if !cfg!(debug_assertions) {
        return;
    }
    let (Ok(base), Ok(token)) = (
        std::env::var("FLOWRAD_DEVLOG_RELAY"),
        std::env::var("FLOWRAD_DEVLOG_TOKEN"),
    ) else {
        return;
    };
    if base.trim().is_empty() || token.trim().is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tracing::info!("devlog relay: enabled; connecting to {base}");
        // Exponential backoff (capped) so a persistently down/flaky relay is not
        // hammered every 5s forever; resets to the floor after any clean session.
        let mut backoff = std::time::Duration::from_secs(2);
        let max_backoff = std::time::Duration::from_secs(60);
        loop {
            match run_once(&app, &base, &token).await {
                Ok(()) => backoff = std::time::Duration::from_secs(2),
                Err(e) => {
                    tracing::warn!("devlog relay: {e}; reconnecting in {:?}", backoff);
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
        }
    });
}

/// Percent-encode the token for use in the `?token=` query. Tokens are strong
/// shared secrets, but they may contain reserved characters (`&`, `?`, `#`, `+`,
/// `/`, `=`), which would otherwise corrupt the URL or split the query.
fn encode_token(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    for b in token.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn run_once(app: &AppHandle, base: &str, token: &str) -> Result<(), String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let url = format!(
        "{}/agent?token={}",
        base.trim_end_matches('/'),
        encode_token(token)
    );
    // Bound the handshake so a blackholed relay / stalled TLS cannot wedge the
    // reconnect loop forever — on timeout we return and the caller backs off.
    let connect = tokio_tungstenite::connect_async(&url);
    let (ws, _) = match tokio::time::timeout(std::time::Duration::from_secs(10), connect).await {
        Ok(Ok(pair)) => pair,
        Ok(Err(e)) => return Err(format!("connect failed: {e}")),
        Err(_) => return Err("connect timed out after 10s".to_string()),
    };
    let (mut write, mut read) = ws.split();

    let hello = json!({
        "type": "hello",
        "info": { "platform": std::env::consts::OS, "version": env!("CARGO_PKG_VERSION") }
    });
    write
        .send(Message::text(hello.to_string()))
        .await
        .map_err(|e| format!("hello send failed: {e}"))?;
    tracing::info!("devlog relay: connected");

    while let Some(msg) = read.next().await {
        let msg = msg.map_err(|e| format!("read error: {e}"))?;
        let Message::Text(txt) = msg else { continue };
        let Ok(val) = serde_json::from_str::<Value>(&txt) else {
            continue;
        };
        if val.get("type").and_then(Value::as_str) != Some("command") {
            continue;
        }
        let id = val
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let text = val
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        tracing::info!(id = %id, text = %text, "devlog relay: running command");

        let (events, result) = run_act(app, &text).await;
        for ev in &events {
            let _ = write
                .send(Message::text(event_message(&id, ev).to_string()))
                .await;
        }
        let reply = json!({ "type": "reply", "id": id, "result": result });
        write
            .send(Message::text(reply.to_string()))
            .await
            .map_err(|e| format!("reply send failed: {e}"))?;
    }
    Ok(())
}

/// Run one transcript through the live Conductor, mirror the events to the HUD,
/// and return them plus a coarse `{ok, summary}` result.
async fn run_act(app: &AppHandle, text: &str) -> (Vec<ActEvent>, Value) {
    let Some(state) = app.try_state::<ActState>() else {
        return (
            vec![],
            json!({ "ok": false, "summary": "Act state unavailable" }),
        );
    };
    // Take the Conductor out of the shared Option so we do NOT hold the session
    // mutex across the (potentially multi-second) `on_transcript` await. Holding
    // it would block — and, if Act ever re-entered `session.lock()` from within,
    // deadlock — the live hotkey path. We restore it afterwards.
    let mut conductor = {
        let mut guard = state.session.lock().await;
        match guard.take() {
            Some(c) => c,
            None => {
                return (
                    vec![],
                    json!({ "ok": false, "summary": "Act is not enabled" }),
                );
            }
        }
    };

    let outcome = match conductor.on_transcript(text.to_string()).await {
        Ok(events) => {
            for ev in &events {
                let _ = app.emit(ACT_EVENT, ev);
            }
            let (ok, summary) = summarize(&events);
            (events, json!({ "ok": ok, "summary": summary }))
        }
        Err(e) => (vec![], json!({ "ok": false, "summary": e.to_string() })),
    };

    // Restore the Conductor. If the slot was re-populated while we ran (e.g. the
    // user toggled Act off then on), don't clobber the newer session.
    {
        let mut guard = state.session.lock().await;
        if guard.is_none() {
            *guard = Some(conductor);
        }
    }

    outcome
}

/// Wrap an `ActEvent` as an `event` relay message, tagged with the command id so
/// the relay's `get_last_trace` can group it.
fn event_message(cmd: &str, ev: &ActEvent) -> Value {
    let mut inner = serde_json::to_value(ev).unwrap_or_else(|_| json!({}));
    if let Some(obj) = inner.as_object_mut() {
        obj.insert("cmd".to_string(), json!(cmd));
    }
    json!({ "type": "event", "event": inner })
}

/// Derive a coarse outcome from a command's events: the last explicit
/// Result/TaskResult wins; a lone Error is a failure; otherwise assume ok.
fn summarize(events: &[ActEvent]) -> (bool, String) {
    let mut outcome: Option<(bool, String)> = None;
    let mut error: Option<String> = None;
    for ev in events {
        match ev {
            ActEvent::Result { ok, summary } => outcome = Some((*ok, summary.clone())),
            ActEvent::TaskResult { ok, summary, .. } => outcome = Some((*ok, summary.clone())),
            ActEvent::Say { text } => outcome = Some((true, text.clone())),
            ActEvent::Error { message } => error = Some(message.clone()),
            _ => {}
        }
    }
    outcome
        .or_else(|| error.map(|m| (false, m)))
        .unwrap_or((true, "done".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_message_tags_cmd_and_preserves_kind() {
        let ev = ActEvent::Step {
            label: "launching Spotify".into(),
        };
        let msg = event_message("t0", &ev);
        assert_eq!(msg["type"], "event");
        assert_eq!(msg["event"]["cmd"], "t0");
        assert_eq!(msg["event"]["kind"], "step");
        assert_eq!(msg["event"]["label"], "launching Spotify");
    }

    #[test]
    fn summarize_prefers_last_result() {
        let events = vec![
            ActEvent::Step { label: "x".into() },
            ActEvent::Result {
                ok: true,
                summary: "Done: open_app".into(),
            },
        ];
        assert_eq!(summarize(&events), (true, "Done: open_app".to_string()));
    }

    #[test]
    fn summarize_surfaces_error_when_no_result() {
        let events = vec![ActEvent::Error {
            message: "boom".into(),
        }];
        assert_eq!(summarize(&events), (false, "boom".to_string()));
    }

    #[test]
    fn summarize_defaults_ok() {
        let events = vec![ActEvent::Step { label: "x".into() }];
        assert_eq!(summarize(&events), (true, "done".to_string()));
    }
}
