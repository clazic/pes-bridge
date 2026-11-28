//! PES 서버와 이야기하기 — MCP(JSON-RPC) 도구 부르기, 동작 신호(beat), 컴퓨터 확인, 기기 키 묶기,
//! 근거 기록 올리기(POST /mcp/evidence) · 작업 로그 올리기(POST /mcp/bridge-log).
//! 뒤 두 길은 지금 PES 서버에 없다 — 필요한 꼴은 bridge/PES-API.md (가짜 PES 로 시험한다).
use crate::account::Account;
use crate::device::DeviceKey;
use crate::time::now_ms;
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;

pub const VERSION: &str = concat!("rust-", env!("CARGO_PKG_VERSION"));

pub struct Pes {
    http: reqwest::Client,
    pub base: String,
    pub account: Account,
    device: DeviceKey,
    /// 기기 키를 묶은 연결 토큰 — 토큰이 바뀌면 다시 묶는다
    bound: Mutex<Option<String>>,
    rpc_id: AtomicU64,
}

/// 근거 기록 번호 꼴(PES src/evidence.ts EVIDENCE_ID): ev_ + base32 소문자 16자
pub fn evidence_id_ok(id: &str) -> bool {
    id.strip_prefix("ev_").is_some_and(|r| {
        r.len() == 16
            && r.chars()
                .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c))
    })
}

impl Pes {
    pub fn new(base: String, account: Account) -> Result<Self> {
        let device = DeviceKey::load_or_create(&account.dir)?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("pes-bridge/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .context("HTTP 클라이언트를 만들지 못했어요")?;
        Ok(Self {
            http,
            base,
            account,
            device,
            bound: Mutex::new(None),
            rpc_id: AtomicU64::new(1),
        })
    }

    pub async fn key(&self) -> Result<String> {
        self.account.key(&self.http, &self.base).await
    }

    async fn rpc(&self, key: &str, method: &str, params: Value) -> Result<Value> {
        let id = self.rpc_id.fetch_add(1, Ordering::Relaxed);
        let r = self
            .http
            .post(format!("{}/mcp", self.base))
            .bearer_auth(key)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .send()
            .await
            .with_context(|| format!("PES MCP({method})에 닿지 않아요"))?;
        let status = r.status();
        let mut v: Value = r
            .json()
            .await
            .with_context(|| format!("PES MCP({method}) 답이 JSON 이 아니에요(HTTP {status})"))?;
        if let Some(e) = v.get("error") {
            bail!(
                "PES MCP({method}) 오류: {}",
                e.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("알 수 없음")
            );
        }
        Ok(v.get_mut("result").map(Value::take).unwrap_or(Value::Null))
    }

    /// PES MCP 도구 하나 — 먼저 「pes-bridge」로 인사(initialize)해 둔다(Node 브리지와 같이: 고른 AI 와 달라도 막히지 않게)
    pub async fn tool(&self, name: &str, args: Value) -> Result<Value> {
        let key = self.key().await?;
        self.rpc(&key, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "pes-bridge", "version": VERSION } }))
            .await?;
        let mut res = self
            .rpc(
                &key,
                "tools/call",
                json!({ "name": name, "arguments": args }),
            )
            .await?;
        let text = res
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if res.get("isError").and_then(Value::as_bool) == Some(true) {
            bail!("{name}: {text}");
        }
        if let Some(sc) = res.get_mut("structuredContent") {
            return Ok(sc.take());
        }
        serde_json::from_str(&text).with_context(|| format!("{name} 답을 읽지 못했어요"))
    }

    /// 이 키가 누구인지(GET /mcp/me) — engine: 계정 기본 AI
    pub async fn me(&self) -> Result<Value> {
        let key = self.key().await?;
        let r = self
            .http
            .get(format!("{}/mcp/me", self.base))
            .bearer_auth(&key)
            .send()
            .await
            .context("PES 에 닿지 않아요(/mcp/me)")?;
        if !r.status().is_success() {
            bail!("PES 가 이 연결을 받지 않아요(HTTP {})", r.status());
        }
        r.json().await.context("/mcp/me 답이 JSON 이 아니에요")
    }

    /// 동작 신호 — 실패해도 일에는 지장 없다(화면 표시용)
    pub async fn beat(&self, b: &Value) {
        let Ok(key) = self.key().await else { return };
        let _ = self
            .http
            .post(format!("{}/mcp/beat", self.base))
            .bearer_auth(&key)
            .json(b)
            .send()
            .await;
    }

    /// 이 컴퓨터로 PES 를 써도 되나 → (허용, 까닭, 기기 ID). 서버에 길이 없으면(404) 허용
    pub async fn computer(&self, info: &Value) -> Result<(bool, String, String)> {
        let key = self.key().await?;
        let r = self
            .http
            .post(format!("{}/mcp/computer", self.base))
            .bearer_auth(&key)
            .json(info)
            .send()
            .await
            .context("PES 에 닿지 않아요(/mcp/computer)")?;
        let status = r.status().as_u16();
        let machine = info
            .get("machine")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if status == 404 {
            return Ok((true, String::new(), machine));
        }
        let j: Value = r.json().await.unwrap_or(Value::Null);
        if status == 401 {
            bail!("PES 가 이 연결을 받지 않아요(401) — 다시 붙여야 해요");
        }
        let allowed = j.get("allowed").and_then(Value::as_bool).unwrap_or(false);
        let reason = j
            .get("reason")
            .or_else(|| j.get("error"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("HTTP {status}"));
        let m = j
            .get("machine")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or(machine);
        Ok((allowed, reason, m))
    }

    /// 이 연결에 기기 키를 묶는다(POST /mcp/device, 연결 토큰마다 한 번)
    async fn bind(&self, key: &str) -> Result<()> {
        let mut bound = self.bound.lock().await;
        if bound.as_deref() == Some(key) {
            return Ok(());
        }
        let ts = now_ms();
        let sig = self
            .device
            .sign(&format!("pes-device\n{ts}\n{}", self.device.public));
        let r = self
            .http
            .post(format!("{}/mcp/device", self.base))
            .bearer_auth(key)
            .json(&json!({ "pub": self.device.public, "ts": ts, "sig": sig }))
            .send()
            .await
            .context("기기 키를 묶으러 PES 에 닿지 않아요")?;
        let status = r.status();
        if !status.is_success() {
            let j: Value = r.json().await.unwrap_or(Value::Null);
            bail!(
                "기기 키를 묶지 못했어요 — {}",
                j.get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("HTTP {status}"))
            );
        }
        *bound = Some(key.to_string());
        Ok(())
    }

    /// 기기 키로 서명한 POST — 몸통 {ts, nonce, r(JSON 글), sig}. 서명 글: "<purpose>\n<ts>\n<nonce>\n<r>"
    async fn signed(&self, path: &str, purpose: &str, r: String) -> Result<Value> {
        let key = self.key().await?;
        self.bind(&key).await?;
        let ts = now_ms();
        let nonce = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 24]>());
        let sig = self.device.sign(&format!("{purpose}\n{ts}\n{nonce}\n{r}"));
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&key)
            .json(&json!({ "ts": ts, "nonce": nonce, "r": r, "sig": sig }))
            .send()
            .await
            .with_context(|| format!("PES 에 닿지 않아요({path})"))?;
        let status = resp.status();
        let j: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            bail!(
                "{}",
                j.get("error")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("HTTP {status}"))
            );
        }
        Ok(j)
    }

    /// 근거 기록 하나를 올리고 번호를 받는다. 번호를 못 받으면 오류 — 부른 쪽은 그 조회를 실패로 돌려준다.
    /// expect_sha: 원본을 나눠 올렸으면(upload) 그 지문 — 서버가 답한 sha256 이 같아야 성공(아니면 오류)
    pub async fn evidence(&self, rec: &Value, expect_sha: Option<&str>) -> Result<String> {
        let r = serde_json::to_string(rec)?;
        let j = self.signed("/mcp/evidence", "pes-evidence", r).await?;
        let id = j
            .get("evidence_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("PES 가 근거 기록 번호를 주지 않았어요"))?;
        if !evidence_id_ok(id) {
            bail!("PES 가 준 근거 기록 번호가 꼴에 맞지 않아요: {id}");
        }
        if let Some(want) = expect_sha {
            let got = j.get("sha256").and_then(Value::as_str).unwrap_or_default();
            if !got.eq_ignore_ascii_case(want) {
                bail!(
                    "PES 가 받은 원본의 지문이 맞지 않아요(보낸 {want}, 받은 {got}) — 근거 기록 {id} 은 쓰지 않아요"
                );
            }
        }
        Ok(id.to_string())
    }

    /// 이어 읽기로 AI 에게 더 준 범위를 근거 기록에 더한다(POST /mcp/evidence-seen)
    pub async fn evidence_seen(
        &self,
        slug: &str,
        id: &str,
        chars: usize,
        ranges: &[[usize; 2]],
    ) -> Result<()> {
        let r = serde_json::to_string(
            &json!({ "slug": slug, "evidence_id": id, "chars": chars, "ranges": ranges }),
        )?;
        self.signed("/mcp/evidence-seen", "pes-evidence-seen", r)
            .await
            .map(|_| ())
    }

    /// 큰 원본을 바이트 그대로 조각으로 나눠 올린다 — 서버의 큰 파일 올리기(R2 멀티파트, src/uploads.ts)를 브리지 서명으로 쓴 꼴(PES-API.md 5절).
    /// 돌려주는 것: (파일 번호, sha256). 서버가 이어 붙여 계산한 sha256 이 보낸 것과 다르면 오류
    pub async fn upload_raw(
        &self,
        slug: &str,
        name: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Result<(String, String)> {
        let sha = hex::encode(Sha256::digest(bytes));
        let start = serde_json::to_string(
            &json!({ "slug": slug, "name": name, "mime": mime, "size": bytes.len(), "sha256": sha }),
        )?;
        let j = self
            .signed("/mcp/evidence-upload", "pes-evidence-upload", start)
            .await?;
        let id = j
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("PES 가 올리기 번호를 주지 않았어요"))?
            .to_string();
        let part = j
            .get("part_size")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| anyhow!("PES 가 조각 크기를 주지 않았어요"))?;
        let key = self.key().await?;
        for (i, chunk) in bytes.chunks(part).enumerate() {
            let n = i + 1;
            let csha = hex::encode(Sha256::digest(chunk));
            let mut last = None;
            for _ in 0..2 {
                // 조각마다 서명 — 연결 토큰만으로는 조각을 바꿔 넣지 못하게
                let ts = now_ms();
                let nonce = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 24]>());
                let sig = self.device.sign(&format!(
                    "pes-evidence-part\n{ts}\n{nonce}\n{id}\n{n}\n{csha}"
                ));
                let resp = self
                    .http
                    .put(format!("{}/mcp/evidence-upload/{id}/part/{n}", self.base))
                    .bearer_auth(&key)
                    .header("x-pes-ts", ts.to_string())
                    .header("x-pes-nonce", nonce)
                    .header("x-pes-sig", sig)
                    .header("x-pes-part-sha256", &csha)
                    .header("content-type", "application/octet-stream")
                    .body(chunk.to_vec()) // 조각 하나만 복사해 보낸다(전체가 아니라)
                    .send()
                    .await;
                match resp {
                    Ok(r) if r.status().is_success() => {
                        last = None;
                        break;
                    }
                    Ok(r) => {
                        let st = r.status();
                        let e: Value = r.json().await.unwrap_or(Value::Null);
                        last = Some(
                            e.get("error")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("HTTP {st}")),
                        );
                    }
                    Err(e) => last = Some(e.to_string()),
                }
            }
            if let Some(e) = last {
                bail!("원본 {n}번 조각을 올리지 못했어요: {e}");
            }
        }
        let done = self
            .signed(
                &format!("/mcp/evidence-upload/{id}/complete"),
                "pes-evidence-upload-complete",
                serde_json::to_string(&json!({ "id": id }))?,
            )
            .await?;
        let file_id = done
            .get("file_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("PES 가 파일 번호를 주지 않았어요"))?;
        let got = done
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !got.eq_ignore_ascii_case(&sha) {
            bail!("PES 가 이어 붙인 원본의 지문이 맞지 않아요(보낸 {sha}, 받은 {got})");
        }
        Ok((file_id.to_string(), sha))
    }

    /// 작업 로그 한 줄을 올린다(POST /mcp/bridge-log). 실패하면 부른 쪽이 이 컴퓨터의 로그에 남긴다
    pub async fn bridge_log(&self, ev: &Value) -> Result<()> {
        let r = serde_json::to_string(&json!({ "events": [ev] }))?;
        self.signed("/mcp/bridge-log", "pes-bridge-log", r)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::evidence_id_ok;

    #[test]
    fn evidence_id_shape() {
        assert!(evidence_id_ok("ev_abcdefgh234567qr"));
        assert!(!evidence_id_ok("ev_abcdefgh234567q1"));
        assert!(!evidence_id_ok("ev_ABCDEFGH234567QR"));
        assert!(!evidence_id_ok("ev_short"));
    }
}
