//! PES 알림 웹소켓(GET /mcp/live) — 붙어 있다가 「일 생김」 알림이 오면 라운드 일을 확인한다(Node 브리지와 같은 방식).
//! 시간마다 돌지 않는다. 붙을 때 한 번 밀린 일을 확인한다.
use crate::log::line;
use crate::pes::Pes;
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

/// 한 번 붙어 끊길 때까지. 돌려주는 값: 다음에 붙기 전 기다릴 시간
async fn once(pes: &Pes, machine: &str, tx: &Sender<String>) -> Result<Duration> {
    let key = pes.key().await?;
    let url = format!("{}/mcp/live", pes.base.replacen("http", "ws", 1));
    let mut req = url
        .into_client_request()
        .context("알림 주소가 맞지 않아요")?;
    let h = req.headers_mut();
    h.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {key}")).context("키 글자가 맞지 않아요")?,
    );
    if let Ok(v) = HeaderValue::from_str(machine) {
        h.insert("x-pes-device-id", v);
    }
    if let Ok(v) = HeaderValue::from_str(&pes.account.id) {
        h.insert("x-pes-account", v);
    }
    let (ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .context("PES 알림에 붙지 못했어요")?;
    line(&format!("[{}] PES 알림에 붙었어요", pes.account.id));
    let _ = tx.send("붙음 · 밀린 일 확인".to_string()).await;
    let (mut w, mut r) = ws.split();
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    loop {
        tokio::select! {
            m = r.next() => match m {
                Some(Ok(Message::Text(t))) => {
                    if t.as_str() == "pong" { continue }
                    let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else { continue };
                    if v.get("kind").and_then(Value::as_str) != Some("work") { continue }
                    let stage = v.get("stage").and_then(Value::as_str).unwrap_or_default();
                    line(&format!("[{}] 알림: {stage}", pes.account.id));
                    // 채팅 · 다듬기 · 내 자료 다듬기는 티켓 23 에서 옮긴다 — 지금은 라운드 일만
                    if stage.ends_with(" chat") || stage.ends_with(" polish") || stage.ends_with(" rag") { continue }
                    let _ = tx.send(stage.to_string()).await;
                }
                Some(Ok(Message::Close(f))) => {
                    let code = f.as_ref().map(|x| u16::from(x.code)).unwrap_or(0);
                    let reason = f.as_ref().map(|x| x.reason.as_str().to_string()).unwrap_or_default();
                    if code == 4003 {
                        line(&format!("[{}] PES 관리자가 이 컴퓨터를 막아 연결이 끊겼어요 {reason} — 10분 뒤 다시 확인해요", pes.account.id));
                        return Ok(Duration::from_secs(600));
                    }
                    line(&format!("[{}] 알림 연결 끊김({code})", pes.account.id));
                    return Ok(Duration::from_secs(1));
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => { line(&format!("[{}] 알림 연결 오류: {e}", pes.account.id)); return Ok(Duration::from_secs(1)) }
                None => return Ok(Duration::from_secs(1)),
            },
            _ = ping.tick() => {
                if w.send(Message::Text("ping".into())).await.is_err() { return Ok(Duration::from_secs(1)) }
            }
        }
    }
}

/// 끊기면 다시 붙는다(1초 ~ 60초, 막혔으면 10분)
pub async fn run(pes: &Pes, machine: &str, tx: Sender<String>) {
    let mut retry: u32 = 0;
    loop {
        let wait = match once(pes, machine, &tx).await {
            Ok(w) => {
                retry = 0;
                w
            }
            Err(e) => {
                let w = Duration::from_secs(60.min(1u64 << retry.min(6)));
                retry += 1;
                line(&format!(
                    "[{}] {e:#} — {}초 뒤 다시",
                    pes.account.id,
                    w.as_secs()
                ));
                w
            }
        };
        tokio::time::sleep(wait).await;
    }
}
