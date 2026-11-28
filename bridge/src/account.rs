//! 계정 하나 — ~/.config/pes-bridge/accounts/<aN>/ 의 key(개인 키) 또는 token.json(OAuth 토큰).
//! Node 브리지와 같은 파일을 읽는다. 여러 계정 붙이기 · 떼기는 티켓 23.
use crate::paths::{Paths, write_private};
use crate::time::now_ms;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use std::path::PathBuf;

pub struct Account {
    pub id: String,
    pub dir: PathBuf,
}

fn has_cred(d: &std::path::Path) -> bool {
    d.join("token.json").is_file() || d.join("key").is_file()
}

/// 쓸 계정 — want(번호, 예: a2)를 주면 그것, 없으면 번호가 가장 작은 계정
pub fn pick(paths: &Paths, want: Option<&str>) -> Result<Account> {
    if let Some(id) = want {
        let dir = paths.accounts.join(id);
        if !has_cred(&dir) {
            bail!(
                "계정 {id} 에 연결 정보가 없어요({}) — Node 브리지로 먼저 붙여 주세요: node worker/worker.mjs --add",
                dir.display()
            );
        }
        return Ok(Account {
            id: id.to_string(),
            dir,
        });
    }
    let mut ids: Vec<(u64, String)> = std::fs::read_dir(&paths.accounts)
        .with_context(|| format!("계정 폴더를 읽지 못했어요: {}", paths.accounts.display()))?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| {
            n.strip_prefix('a')
                .and_then(|x| x.parse::<u64>().ok())
                .map(|k| (k, n))
        })
        .filter(|(_, n)| has_cred(&paths.accounts.join(n)))
        .collect();
    ids.sort();
    let (_, id) = ids.into_iter().next().ok_or_else(|| {
        anyhow!("붙은 계정이 없어요 — Node 브리지로 먼저 붙여 주세요: node worker/worker.mjs --add")
    })?;
    let dir = paths.accounts.join(&id);
    Ok(Account { id, dir })
}

fn valid_key(k: &str) -> bool {
    k.len() >= 24
        && k.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("pes_"))
        && k.chars().skip(4).all(|c| c.is_ascii_alphanumeric())
}

impl Account {
    fn file_key(&self) -> Option<String> {
        let k = std::fs::read_to_string(self.dir.join("key")).ok()?;
        let k = k.trim();
        valid_key(k).then(|| k.to_string())
    }

    /// 쓸 키: 직접 넣은 개인 키 → 저장된 토큰(만료 하루 전이면 갱신). 못 받으면 오류(다시 붙여야 한다)
    pub async fn key(&self, http: &reqwest::Client, base: &str) -> Result<String> {
        if let Some(k) = self.file_key() {
            return Ok(k);
        }
        let file = self.dir.join("token.json");
        let raw = std::fs::read_to_string(&file)
            .with_context(|| format!("연결 토큰을 읽지 못했어요: {}", file.display()))?;
        let tok: Value = serde_json::from_str(&raw).context("연결 토큰 파일이 JSON 이 아니에요")?;
        let access = tok
            .get("access")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let exp = tok.get("exp").and_then(Value::as_f64).unwrap_or(0.0);
        let now = now_ms() as f64;
        if exp - now > 86_400_000.0 && !access.is_empty() {
            return Ok(access.to_string());
        }
        match self.refresh(http, base, &tok).await {
            Ok(k) => Ok(k),
            Err(e) if exp > now && !access.is_empty() => {
                crate::log::line(&format!(
                    "[{}] 토큰을 갱신하지 못했어요(아직 쓸 수 있어 그대로 써요): {e}",
                    self.id
                ));
                Ok(access.to_string())
            }
            Err(e) => Err(e.context(format!(
                "계정 {} 은 다시 붙여야 해요: node worker/worker.mjs --remove {} 뒤 --add",
                self.id, self.id
            ))),
        }
    }

    async fn refresh(&self, http: &reqwest::Client, base: &str, tok: &Value) -> Result<String> {
        let refresh = tok
            .get("refresh")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("갱신 토큰이 없어요"))?;
        let client = tok
            .get("client_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("client_id 가 없어요"))?;
        let r = http
            .post(format!("{base}/oauth/token"))
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
                ("client_id", client),
            ])
            .send()
            .await
            .context("토큰 갱신 요청이 가지 않았어요")?;
        let t: Value = r.json().await.context("토큰 갱신 답이 JSON 이 아니에요")?;
        let access = t
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("토큰 갱신이 거절됐어요"))?;
        let next_refresh = t
            .get("refresh_token")
            .and_then(Value::as_str)
            .unwrap_or(refresh);
        let exp = now_ms() + t.get("expires_in").and_then(Value::as_i64).unwrap_or(0) * 1000;
        let next =
            json!({ "client_id": client, "access": access, "refresh": next_refresh, "exp": exp });
        write_private(
            &self.dir.join("token.json"),
            serde_json::to_string_pretty(&next)?.as_bytes(),
        )?;
        crate::log::line(&format!("[{}] 토큰을 갱신했어요", self.id));
        Ok(access.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::valid_key;

    #[test]
    fn key_shape() {
        assert!(valid_key("pes_abcdefghijklmnopqrstuv12"));
        assert!(!valid_key("pes_short"));
        assert!(!valid_key("xyz_abcdefghijklmnopqrstuv12"));
        assert!(!valid_key("pes_abcdefghijklmnopqrstu-12"));
    }
}
