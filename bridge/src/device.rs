//! 이 컴퓨터 — 기기 ID(하드웨어 UUID 의 지문)와 계정 폴더의 기기 키(ECDSA P-256, device.json).
//! Node 브리지(worker.mjs deviceInfo · relay.mjs Vault)와 같은 꼴이라 같은 파일을 함께 쓴다.
//! 기기 키는 브리지가 PES 에 올리는 근거 기록 · 작업 로그에 서명한다 — 연결 토큰은 AI 프로세스도 갖고 있어서(MCP 연결)
//! 토큰만으로는 근거 기록을 올리지 못하게 하려는 것이다.
use crate::paths::write_private;
use anyhow::{Context, Result, anyhow};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct DeviceKey {
    key: SigningKey,
    /// 공개 키(압축 안 한 65바이트)의 base64 — PES 에 묶을 때 보낸다
    pub public: String,
}

impl DeviceKey {
    /// 계정 폴더의 device.json — 없으면 만든다(개인 키는 이 파일에만, 0600)
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let file = dir.join("device.json");
        if let Ok(raw) = std::fs::read_to_string(&file) {
            let j: Value = serde_json::from_str(&raw).context("device.json 이 JSON 이 아니에요")?;
            let d = j
                .pointer("/jwk/d")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("device.json 에 개인 키가 없어요"))?;
            let bytes = URL_SAFE_NO_PAD
                .decode(d)
                .context("device.json 개인 키를 읽지 못했어요")?;
            let key = SigningKey::from_slice(&bytes)
                .map_err(|e| anyhow!("device.json 개인 키가 맞지 않아요: {e}"))?;
            let public = STANDARD.encode(key.verifying_key().to_encoded_point(false).as_bytes());
            return Ok(Self { key, public });
        }
        let key = SigningKey::random(&mut rand::rngs::OsRng);
        let point = key.verifying_key().to_encoded_point(false);
        let raw = point.as_bytes();
        let (x, y) = (
            raw.get(1..33).unwrap_or_default(),
            raw.get(33..65).unwrap_or_default(),
        );
        let public = STANDARD.encode(raw);
        // WebCrypto exportKey('jwk') 꼴 — Node 브리지가 그대로 읽는다
        let j = json!({
            "jwk": { "kty": "EC", "crv": "P-256", "x": URL_SAFE_NO_PAD.encode(x), "y": URL_SAFE_NO_PAD.encode(y),
                     "d": URL_SAFE_NO_PAD.encode(key.to_bytes()), "ext": true, "key_ops": ["sign"] },
            "pub": public,
        });
        write_private(&file, serde_json::to_string(&j)?.as_bytes())?;
        Ok(Self { key, public })
    }

    /// 글에 서명 → base64(r‖s 64바이트, WebCrypto ECDSA 와 같은 꼴)
    pub fn sign(&self, msg: &str) -> String {
        let sig: Signature = self.key.sign(msg.as_bytes());
        STANDARD.encode(sig.to_bytes())
    }
}

async fn run1(cmd: &str, args: &[&str]) -> String {
    let fut = tokio::process::Command::new(cmd).args(args).output();
    match tokio::time::timeout(std::time::Duration::from_secs(5), fut).await {
        Ok(Ok(o)) => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => String::new(),
    }
}

/// 기기 ID — 하드웨어 UUID 의 sha256 앞 32자(UUID 자체는 보내지 않는다). 못 읽으면 conf/machine-id 에 만든 무작위 값
pub async fn machine_id(conf: &Path) -> String {
    let mut hw = String::new();
    if cfg!(target_os = "macos") {
        let o = run1("/usr/sbin/ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"]).await;
        if let Some(rest) = o.split("\"IOPlatformUUID\" = \"").nth(1) {
            hw = rest.split('"').next().unwrap_or_default().to_string();
        }
    } else if cfg!(windows) {
        let o = run1(
            "reg",
            &[
                "query",
                "HKLM\\SOFTWARE\\Microsoft\\Cryptography",
                "/v",
                "MachineGuid",
            ],
        )
        .await;
        hw = o
            .split_whitespace()
            .skip_while(|w| *w != "REG_SZ")
            .nth(1)
            .unwrap_or_default()
            .to_string();
    } else {
        for f in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
            if let Ok(s) = std::fs::read_to_string(f) {
                hw = s.trim().to_string();
                if !hw.is_empty() {
                    break;
                }
            }
        }
    }
    if hw.trim().is_empty() {
        let f = conf.join("machine-id");
        let saved = std::fs::read_to_string(&f)
            .unwrap_or_default()
            .trim()
            .to_string();
        let ok = saved.len() >= 32 && saved.chars().all(|c| c.is_ascii_hexdigit());
        let v = if ok {
            saved
        } else {
            let bytes: [u8; 32] = rand::random();
            let v = hex::encode(bytes);
            if let Err(e) = write_private(&f, format!("{v}\n").as_bytes()) {
                crate::log::line(&format!(
                    "기기 ID 를 저장하지 못했어요(이번만 쓰는 ID 로 해요): {e}"
                ));
            }
            v
        };
        hw = format!("saved:{v}");
    }
    let h = Sha256::digest(format!("pes-bridge:{hw}").as_bytes());
    hex::encode(h).chars().take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;

    #[test]
    fn create_load_sign_verify() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let a = DeviceKey::load_or_create(dir.path())?;
        let b = DeviceKey::load_or_create(dir.path())?;
        assert_eq!(a.public, b.public, "다시 읽어도 같은 키");
        let sig = STANDARD.decode(b.sign("pes-evidence\n1\nn\nr"))?;
        let vk = VerifyingKey::from_sec1_bytes(&STANDARD.decode(&a.public)?)?;
        let s = Signature::from_slice(&sig)?;
        vk.verify(b"pes-evidence\n1\nn\nr", &s)?;
        Ok(())
    }
}
