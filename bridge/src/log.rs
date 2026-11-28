//! 로그 — 이 컴퓨터의 로그 줄(표준 출력, 서비스 관리자가 파일로 받는다)과 PES 서비스 로그(작업 로그)로 보낼 사건.
use crate::time::now_iso;
use serde_json::{Value, json};

/// 로그 한 줄(시각 + 글). 비밀값(키 · 토큰)은 넣지 않는다.
pub fn line(msg: &str) {
    println!("{} {msg}", now_iso());
}

/// PES 서비스 로그(작업 로그)의 한 줄 — 언제 · 어디서 · 누가 · 무엇을 · 어떻게 · 왜 · 결과(명세 「로그」).
/// 언제(at)는 브리지 시각, 누가는 서버가 연결 토큰으로 안다(브리지 = PC). 「왜」는 자동 규칙 이름이나 사람의 지시다.
pub struct Event<'a> {
    /// bridge_connect · bridge_round_start · bridge_round_end · bridge_round_refused · bridge_tool · bridge_evidence_fail
    pub action: &'static str,
    pub project: Option<&'a str>,
    pub stage: Option<&'a str>,
    pub target: Option<&'a str>,
    pub how: String,
    /// rule(자동 규칙) · human(사람의 지시 · 답)
    pub why_kind: &'static str,
    pub why: String,
    /// ok · fail · denied
    pub result: &'static str,
    pub detail: Value,
}

impl Event<'_> {
    pub fn to_json(&self) -> Value {
        json!({
            "at": now_iso(), "action": self.action, "project": self.project, "stage": self.stage, "target": self.target,
            "how": self.how, "why_kind": self.why_kind, "why": self.why, "result": self.result, "detail": self.detail,
        })
    }
}
