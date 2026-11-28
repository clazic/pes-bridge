//! pes_local — 브리지가 127.0.0.1 에 여는 MCP 서버(AI 본체가 부르고, 셸 샌드박스 밖에서 브리지가 실행한다. 티켓 21 7절).
//! 도구: 조사 CLI(kosis · vworld · sgis · korean_law) · kordoc(받은 문서 변환) · fetch_url(PC 에서 원문 열기) · ego_browser(감싼 브라우저) · read_more(긴 결과 이어 읽기).
//! 도구마다 요청 · 응답 원본을 PES 근거 기록으로 올리고(POST /mcp/evidence) 받은 번호(evidence_id)를 답에 붙인다.
//! 올리지 못하면 그 도구 호출은 실패로 돌려준다 — 결과를 AI 에 주지 않는다(근거 기록 저장 실패 = 실패).
//! 실행(라운드)마다 새 토큰을 주소에 넣는다(open → close). 실행이 끝나면 그 주소는 죽는다.
use crate::html;
use crate::log::{Event, line};
use crate::paths::{child_path, which};
use crate::pes::Pes;
use crate::time::now_iso;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

/// 한 호출 원본 한도 — PES 근거 원본 한도(src/evidence.ts RAW_MAX)와 같다. 넘으면 그 조회는 실패
pub const RAW_MAX: usize = 25 * 1024 * 1024;
/// AI 에 한 번에 주는 글자 수 — 넘으면 next_from 으로 이어 읽게 한다(잘라 버리지 않는다)
pub const PAGE: usize = 40_000;
const CLI_SECS: u64 = 600;

/// 조사 CLI — (도구 이름, 실행 파일 이름, 안내)
const CLIS: [(&str, &str, &str); 4] = [
    ("kosis", "kosis", "KOSIS 국가통계 CLI"),
    (
        "vworld",
        "vworld",
        "VWorld 공간정보 CLI(주소 · 좌표 · 용도지역 · 공간 데이터)",
    ),
    (
        "sgis",
        "sgis",
        "SGIS 통계지리 CLI(인구 · 가구 · 주택 · 사업체 · 행정경계)",
    ),
    (
        "korean_law",
        "korean-law",
        "법령 CLI(법령 · 조문 · 조례 · 행정규칙)",
    ),
];

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
pub struct Stats {
    pub calls: u32,
    /// 올린 근거 기록 번호
    pub recorded: Vec<String>,
    /// 근거 기록을 올리지 못해 실패로 돌려준 호출 수
    pub upload_failed: u32,
}

struct FileRec {
    evidence: String,
    url: String,
}

/// 실행(라운드) 하나
pub struct Run {
    pub id: String,
    pub slugs: Vec<String>,
    pub engine: &'static str,
    pub stats: Mutex<Stats>,
    /// 근거 번호 → AI 가 읽는 글(이어 읽기용, 이 실행 안에서만)
    cache: Mutex<HashMap<String, String>>,
    /// 받은 문서 파일 이름 → 근거 기록(kordoc 은 이것만 바꾼다)
    files: Mutex<HashMap<String, FileRec>>,
}

#[derive(Serialize)]
struct Part<'a> {
    url: &'a str,
    at: &'a str,
    status: Option<i64>,
    mime: &'a str,
    body: &'a str,
}

#[derive(Serialize)]
struct Binary<'a> {
    url: &'a str,
    at: &'a str,
    status: Option<i64>,
    mime: &'a str,
    b64: String,
}

/// 근거 기록 하나 — PES 에 올리는 꼴(bridge/PES-API.md)
#[derive(Serialize)]
struct Rec<'a> {
    slug: &'a str,
    stage: Option<&'a str>,
    tool: &'a str,
    /// lookup(직접 조회) · original(원문) · search(검색 결과 목록) — 브리지의 제안, 정하는 것은 서버
    kind: &'a str,
    args: &'a Value,
    asked_at: &'a str,
    /// ok · empty · fail
    status: &'a str,
    error: Option<&'a str>,
    title: &'a str,
    url: &'a str,
    /// 어디서: PC(브리지가 이 컴퓨터에서 열었다)
    via: &'static str,
    route: &'static str,
    engine: &'a str,
    run: &'a str,
    parts: Vec<Part<'a>>,
    binary: Option<Binary<'a>>,
    requery: Value,
}

pub struct Local {
    pub port: u16,
    runs: Mutex<HashMap<String, Arc<Run>>>,
    pes: Arc<Pes>,
    docs: PathBuf,
    home: PathBuf,
    web: reqwest::Client,
    /// 시험용: 127.0.0.1 등 내부 주소도 열게 한다(PES_BRIDGE_ALLOW_LOOPBACK_FETCH=1). 평소에는 막는다(로컬 서비스 · pes_local 자신을 열지 못하게)
    allow_loopback: bool,
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64
        }
        IpAddr::V6(v) => {
            v.is_loopback()
                || v.is_unspecified()
                || (v.segments()[0] & 0xfe00) == 0xfc00
                || (v.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn blocked_host(u: &reqwest::Url) -> bool {
    match u.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.to_ascii_lowercase();
            d == "localhost"
                || d.ends_with(".localhost")
                || d.ends_with(".local")
                || d.ends_with(".internal")
        }
        Some(url::Host::Ipv4(v)) => blocked_ip(IpAddr::V4(v)),
        Some(url::Host::Ipv6(v)) => blocked_ip(IpAddr::V6(v)),
        None => true,
    }
}

fn texty(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m.starts_with("text/")
        || m.contains("json")
        || m.contains("xml")
        || m.contains("javascript")
        || m.contains("html")
}

/// 받은 바이트 → 글. utf8: 원본이 그대로 UTF-8 이었나(아니면 원본은 바이트로 올린다)
fn decode(bytes: &[u8], mime: &str) -> (String, bool) {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_string(), true);
    }
    let label = mime
        .to_ascii_lowercase()
        .split("charset=")
        .nth(1)
        .map(|x| {
            x.trim_matches(|c: char| c == '"' || c == ';' || c.is_whitespace())
                .to_string()
        })
        .or_else(|| {
            let head =
                String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
            head.split("charset=").nth(1).map(|x| {
                x.trim_start_matches(['"', '\''])
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                    .collect()
            })
        });
    let enc = label
        .and_then(|l| encoding_rs::Encoding::for_label(l.as_bytes()))
        .unwrap_or(encoding_rs::EUC_KR);
    let (t, _, _) = enc.decode(bytes);
    (t.into_owned(), false)
}

/// 글의 from 번째 글자부터 PAGE 글자 — 끝이 아니면 next_from
fn page(text: &str, from: usize) -> Value {
    let total = text.chars().count();
    let s: String = text.chars().skip(from).take(PAGE).collect();
    let next = (from + PAGE < total).then_some(from + PAGE);
    json!({ "text": s, "from": from, "chars_total": total, "next_from": next,
            "read": if next.is_some() { "아직 끝이 아니에요 — read_more {evidence_id, from: next_from} 로 끝까지 이어 읽으세요" } else { "끝까지 받았어요" } })
}

fn str_arg<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// 파일 이름으로 쓸 글(경로 · 이상한 글자 없이)
fn safe_name(s: &str) -> String {
    let n: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let n = n.trim_matches('.').chars().take(80).collect::<String>();
    if n.is_empty() { "file".to_string() } else { n }
}

async fn read_cap<R: AsyncRead + Unpin>(r: Option<R>, cap: usize) -> (Vec<u8>, bool) {
    let Some(mut r) = r else {
        return (Vec::new(), false);
    };
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => return (out, false),
            Ok(n) => {
                out.extend_from_slice(buf.get(..n).unwrap_or_default());
                if out.len() > cap {
                    out.truncate(cap);
                    return (out, true);
                }
            }
        }
    }
}

#[derive(Default)]
struct Capped {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    over: bool,
    timed_out: bool,
    spawn_err: Option<String>,
}

/// 명령을 셸 없이(인자 배열 그대로) 돌린다 — 출력은 cap 바이트까지, secs 초 안에
async fn run_capped(
    bin: &Path,
    args: &[String],
    cwd: &Path,
    envs: &[(String, String)],
    secs: u64,
) -> Capped {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .current_dir(cwd)
        .env("PATH", child_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Capped {
                spawn_err: Some(e.to_string()),
                ..Capped::default()
            };
        }
    };
    let (so, se) = (child.stdout.take(), child.stderr.take());
    let all = async {
        let ((out, over), (err, _)) = tokio::join!(read_cap(so, RAW_MAX), read_cap(se, 1 << 20));
        if over {
            let _ = child.start_kill();
        }
        let code = child.wait().await.ok().and_then(|s| s.code());
        Capped {
            code,
            stdout: out,
            stderr: err,
            over,
            ..Capped::default()
        }
    };
    match tokio::time::timeout(Duration::from_secs(secs), all).await {
        Ok(c) => c,
        Err(_) => Capped {
            timed_out: true,
            ..Capped::default()
        },
    }
}

/// LAW_OC(법령 API 값) — 설치 때 ~/.claude/settings.json 의 env 에 넣는다(Node 브리지 loadLawOc 와 같다)
fn law_oc(home: &Path) -> Option<String> {
    if std::env::var_os("LAW_OC").is_some() {
        return None;
    }
    let raw = std::fs::read_to_string(home.join(".claude").join("settings.json")).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    v.pointer("/env/LAW_OC")
        .and_then(Value::as_str)
        .map(str::to_string)
}

impl Local {
    pub async fn start(pes: Arc<Pes>, docs: PathBuf, home: PathBuf) -> anyhow::Result<Arc<Self>> {
        let allow_loopback =
            std::env::var("PES_BRIDGE_ALLOW_LOOPBACK_FETCH").is_ok_and(|v| v == "1");
        let web = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; pes-bridge)")
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::custom(move |at| {
                if at.previous().len() >= 10 {
                    at.error("주소 넘김이 너무 많아요")
                } else if !allow_loopback && blocked_host(at.url()) {
                    at.error("내부 주소로 넘기는 것은 따라가지 않아요")
                } else {
                    at.follow()
                }
            }))
            .build()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        std::fs::create_dir_all(&docs)?;
        let local = Arc::new(Self {
            port,
            runs: Mutex::new(HashMap::new()),
            pes,
            docs,
            home,
            web,
            allow_loopback,
        });
        // clone: 서버 작업(axum)과 브리지(실행 열기 · 닫기)가 같은 Local 을 함께 쓴다
        let app = Router::new()
            .route("/mcp/{token}", any(handle))
            .with_state(Arc::clone(&local));
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                line(&format!("pes_local 서버가 멈췄어요: {e}"));
            }
        });
        Ok(local)
    }

    /// 실행 하나를 연다 → (토큰, 주소)
    pub fn open(&self, slugs: Vec<String>, engine: &'static str) -> (String, String) {
        let token = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let id = token.chars().take(6).collect::<String>();
        let run = Run {
            id,
            slugs,
            engine,
            stats: Mutex::new(Stats::default()),
            cache: Mutex::new(HashMap::new()),
            files: Mutex::new(HashMap::new()),
        };
        let url = format!("http://127.0.0.1:{}/mcp/{token}", self.port);
        lock(&self.runs).insert(token.clone(), Arc::new(run)); // clone: 토큰은 표의 열쇠이자 부른 쪽이 닫을 때 쓰는 값이다
        (token, url)
    }

    /// 실행을 닫는다 → 그 실행(통계). 그 뒤 주소는 401
    pub fn close(&self, token: &str) -> Option<Arc<Run>> {
        lock(&self.runs).remove(token)
    }

    fn find(&self, token: &str) -> Option<Arc<Run>> {
        let runs = lock(&self.runs);
        // clone: Arc — 표의 잠금을 풀고 도구가 도는 동안 그 실행을 쓴다
        runs.iter()
            .find(|(t, _)| same(t, token))
            .map(|(_, r)| Arc::clone(r))
    }

    fn tools_list(&self) -> Vec<Value> {
        let base = |extra: Value| -> Value {
            let mut p = json!({ "slug": { "type": "string", "description": "프로젝트 slug(이번 실행이 맡은 것)" }, "stage": { "type": "string", "description": "단계(plan · research · analyze · write · verify · publish)" } });
            if let (Some(o), Value::Object(e)) = (p.as_object_mut(), extra) {
                o.extend(e);
            }
            p
        };
        let mut v = Vec::new();
        for (name, bin, about) in CLIS {
            if which(bin).is_some() {
                v.push(json!({ "name": name, "description": format!("{about}를 이 컴퓨터에서 돌린다(셸 없이). args 에 그 CLI 의 인자를 배열로 준다(예: [\"query\", \"민법 제1조\"]). 요청 · 응답 원본은 PES 근거 기록으로 남고 evidence_id 가 붙어 온다 — 그 결과로 쓴 문장에 [^evidence_id] 를 단다. 결과가 길면 read_more 로 끝까지 읽는다."),
                    "inputSchema": { "type": "object", "properties": base(json!({ "args": { "type": "array", "items": { "type": "string" }, "description": format!("{bin} CLI 인자") } })), "required": ["slug", "args"] } }));
            }
        }
        if which("kordoc").is_some() {
            v.push(json!({ "name": "kordoc", "description": "fetch_url · ego_browser 로 받은 문서 파일(HWP · HWPX · PDF · 오피스)을 Markdown 으로 바꿔 읽는다. file 은 그 도구가 돌려준 file 이름. 바꾼 글도 근거 기록으로 남는다.",
                "inputSchema": { "type": "object", "properties": base(json!({ "file": { "type": "string" }, "pages": { "type": "string", "description": "쪽 범위(예: 1-20)" }, "ocr": { "type": "boolean" } })), "required": ["slug", "file"] } }));
        }
        v.push(json!({ "name": "fetch_url", "description": "웹 원문을 이 컴퓨터에서 연다(내장 웹 가져오기 대신). 받은 원본은 PES 근거 기록으로 남고 evidence_id 가 붙어 온다. HTML 은 읽을 글(링크 포함)로, 문서 파일은 file 이름으로 돌려준다(kordoc 으로 읽는다).",
            "inputSchema": { "type": "object", "properties": base(json!({ "url": { "type": "string", "description": "http(s) 주소" } })), "required": ["slug", "url"] } }));
        if which("ego-browser").is_some() {
            v.push(json!({ "name": "ego_browser", "description": "fetch_url 로 내용이 안 나오는 화면(자바스크립트로 그리는 화면 · 통계 표 · 지도)을 브라우저로 열어 읽는다. 공개 페이지 읽기만 한다. 연 페이지 원본은 PES 근거 기록으로 남는다.",
                "inputSchema": { "type": "object", "properties": base(json!({ "url": { "type": "string" } })), "required": ["slug", "url"] } }));
        }
        v.push(json!({ "name": "read_more", "description": "긴 결과를 이어 읽는다 — 앞 도구가 준 evidence_id 와 next_from.",
            "inputSchema": { "type": "object", "properties": { "evidence_id": { "type": "string" }, "from": { "type": "integer" } }, "required": ["evidence_id", "from"] } }));
        v
    }

    async fn rpc(&self, run: &Run, m: &Value) -> Option<Value> {
        let id = m.get("id")?;
        let method = m.get("method").and_then(Value::as_str).unwrap_or_default();
        let ok = |result: Value| json!({ "jsonrpc": "2.0", "id": id, "result": result });
        Some(match method {
            "initialize" => ok(
                json!({ "protocolVersion": m.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or("2025-03-26"), "capabilities": { "tools": {} },
                "serverInfo": { "name": "pes_local", "version": crate::pes::VERSION },
                "instructions": "PES 브리지의 로컬 조사 도구. 조사 CLI · 원문 열기 · 브라우저는 셸이 아니라 이 도구로만 부른다. 결과마다 evidence_id(PES 근거 기록 번호)가 붙는다." }),
            ),
            "ping" => ok(json!({})),
            "tools/list" => ok(json!({ "tools": self.tools_list() })),
            "tools/call" => {
                let name = m
                    .pointer("/params/name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let empty = json!({});
                let args = m.pointer("/params/arguments").unwrap_or(&empty);
                lock(&run.stats).calls += 1;
                match self.call(run, name, args).await {
                    Ok(v) => ok(
                        json!({ "content": [{ "type": "text", "text": v.to_string() }], "structuredContent": v }),
                    ),
                    Err(e) => {
                        ok(json!({ "isError": true, "content": [{ "type": "text", "text": e }] }))
                    }
                }
            }
            _ => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "지원하지 않는 메서드예요." } })
            }
        })
    }

    async fn call(&self, run: &Run, name: &str, a: &Value) -> Result<Value, String> {
        if name == "read_more" {
            return self.read_more(run, a);
        }
        let slug = str_arg(a, "slug").ok_or("slug 가 필요해요.")?;
        if !run.slugs.iter().any(|s| s == slug) {
            return Err(format!(
                "이번 실행이 맡은 프로젝트가 아니에요(맡은 것: {}).",
                run.slugs.join(" · ")
            ));
        }
        let stage = str_arg(a, "stage");
        let ctx = Ctx { run, slug, stage };
        if let Some((tool, bin, _)) = CLIS.iter().find(|(n, _, _)| *n == name) {
            return self.cli(&ctx, tool, bin, a).await;
        }
        match name {
            "kordoc" => self.kordoc(&ctx, a).await,
            "fetch_url" => self.fetch(&ctx, a).await,
            "ego_browser" => self.ego(&ctx, a).await,
            _ => Err(format!("없는 도구예요: {name}")),
        }
    }

    /// 근거 기록을 올리고 번호를 받는다. 못 받으면 Err — 그 호출은 실패로 돌려준다
    async fn record(&self, ctx: &Ctx<'_>, rec: &Rec<'_>) -> Result<String, String> {
        let v =
            serde_json::to_value(rec).map_err(|e| format!("근거 기록을 만들지 못했어요: {e}"))?;
        match self.pes.evidence(&v).await {
            Ok(id) => {
                // clone: 번호는 실행 통계(작업 로그)와 도구 답 두 곳에 들어간다
                lock(&ctx.run.stats).recorded.push(id.clone());
                Ok(id)
            }
            Err(e) => {
                lock(&ctx.run.stats).upload_failed += 1;
                let why = format!("{e:#}");
                line(&format!(
                    "pes_local {}: 근거 기록을 PES 에 올리지 못했어요 — {why}",
                    rec.tool
                ));
                let ev = Event {
                    action: "bridge_evidence_fail", project: Some(ctx.slug), stage: ctx.stage, target: Some(rec.tool), how: format!("pes_local {} → POST /mcp/evidence", rec.tool),
                    why_kind: "rule", why: "PC 에서 한 조회는 원본을 PES 근거 기록으로 올려야 성공으로 친다(티켓 21 · 22)".to_string(), result: "fail",
                    detail: json!({ "error": why, "url": rec.url, "run": ctx.run.id }),
                };
                if let Err(e2) = self.pes.bridge_log(&ev.to_json()).await {
                    line(&format!("작업 로그도 올리지 못했어요: {e2:#}"));
                }
                Err(format!(
                    "근거 기록을 PES 에 올리지 못해 이 조회는 실패로 돌려요(결과를 쓰지 마세요): {why}"
                ))
            }
        }
    }

    fn remember(&self, ctx: &Ctx<'_>, id: String, text: String) {
        lock(&ctx.run.cache).insert(id, text);
    }

    fn read_more(&self, run: &Run, a: &Value) -> Result<Value, String> {
        let id = str_arg(a, "evidence_id").ok_or("evidence_id 가 필요해요.")?;
        let from = a
            .get("from")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        let cache = lock(&run.cache);
        let text = cache
            .get(id)
            .ok_or("이번 실행에서 받은 결과가 아니에요(앞 도구가 준 evidence_id 를 쓰세요).")?;
        let mut p = page(text, from);
        if let Some(o) = p.as_object_mut() {
            o.insert("evidence_id".to_string(), json!(id));
        }
        Ok(p)
    }

    async fn cli(&self, ctx: &Ctx<'_>, tool: &str, bin: &str, a: &Value) -> Result<Value, String> {
        let args: Vec<String> = a
            .get("args")
            .and_then(Value::as_array)
            .map(|v| {
                v.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if args.is_empty() || args.len() > 64 || args.iter().any(|x| x.len() > 4000) {
            return Err("args 는 문자열 1~64개(각 4000자 안)여야 해요.".to_string());
        }
        let path = which(bin).ok_or_else(|| format!("이 컴퓨터에 {bin} 이 없어요."))?;
        let envs: Vec<(String, String)> = law_oc(&self.home)
            .map(|v| vec![("LAW_OC".to_string(), v)])
            .unwrap_or_default();
        let at = now_iso();
        let mut c = run_capped(&path, &args, &self.docs, &envs, CLI_SECS).await;
        let out = String::from_utf8_lossy(&c.stdout).into_owned();
        let err = String::from_utf8_lossy(&c.stderr).into_owned();
        let fail = c
            .spawn_err
            .take()
            .or_else(|| c.timed_out.then(|| format!("{CLI_SECS}초가 넘어 멈췄어요")))
            .or_else(|| {
                c.over
                    .then(|| format!("원본이 너무 커요(한도 {}MB)", RAW_MAX >> 20))
            })
            .or_else(|| {
                (c.code != Some(0)).then(|| {
                    format!(
                        "{bin} 이 {} 로 끝났어요",
                        c.code.map_or("신호".to_string(), |x| x.to_string())
                    )
                })
            });
        let status = if fail.is_some() {
            "fail"
        } else if out.trim().is_empty() {
            "empty"
        } else {
            "ok"
        };
        let first = args.first().map(String::as_str).unwrap_or_default();
        let kind = if ["search", "find", "list", "ls", "catalog"].contains(&first) {
            "search"
        } else {
            "lookup"
        };
        let cmdline = format!("cli:{bin} {}", args.join(" "));
        let mime = if out.trim_start().starts_with(['{', '[']) {
            "application/json"
        } else {
            "text/plain; charset=utf-8"
        };
        let code = c.code.map(i64::from);
        let mut parts = vec![Part {
            url: &cmdline,
            at: &at,
            status: code,
            mime,
            body: &out,
        }];
        if !err.trim().is_empty() {
            parts.push(Part {
                url: "cli:stderr",
                at: &at,
                status: code,
                mime: "text/plain; charset=utf-8",
                body: &err,
            });
        }
        let argv = json!({ "cli": bin, "args": args });
        let title = format!(
            "{bin} {}",
            args.join(" ").chars().take(120).collect::<String>()
        );
        let rec = Rec {
            slug: ctx.slug,
            stage: ctx.stage,
            tool: tool_name(tool),
            kind,
            args: &argv,
            asked_at: &at,
            status,
            error: fail.as_deref(),
            title: &title,
            url: &cmdline,
            via: "pc",
            route: "pc",
            engine: ctx.run.engine,
            run: &ctx.run.id,
            parts,
            binary: None,
            requery: json!({ "명령": bin, "인자": argv.get("args") }),
        };
        let id = self.record(ctx, &rec).await?;
        if let Some(f) = fail {
            return Err(format!(
                "{f}. 이 실패도 근거 기록 {id} 로 남았어요(인용하지 마세요). 출력: {}",
                format!("{err}{out}").chars().take(1500).collect::<String>()
            ));
        }
        let mut p = page(&out, 0);
        if let Some(o) = p.as_object_mut() {
            o.insert("evidence_id".to_string(), json!(id));
            o.insert("status".to_string(), json!(status));
        }
        self.remember(ctx, id, out);
        Ok(p)
    }

    async fn check_host(&self, u: &reqwest::Url) -> Result<(), String> {
        if self.allow_loopback {
            return Ok(());
        }
        if blocked_host(u) {
            return Err("내부 주소(이 컴퓨터 · 사설망)는 열지 않아요.".to_string());
        }
        if let Some(url::Host::Domain(d)) = u.host() {
            let port = u.port_or_known_default().unwrap_or(443);
            let addrs = tokio::net::lookup_host((d, port))
                .await
                .map_err(|e| format!("주소를 찾지 못했어요: {e}"))?;
            if addrs.into_iter().any(|a| blocked_ip(a.ip())) {
                return Err("내부 주소(이 컴퓨터 · 사설망)로 가는 이름은 열지 않아요.".to_string());
            }
        }
        Ok(())
    }

    async fn fetch(&self, ctx: &Ctx<'_>, a: &Value) -> Result<Value, String> {
        let raw = str_arg(a, "url").ok_or("url 이 필요해요.")?;
        let u = reqwest::Url::parse(raw).map_err(|e| format!("주소가 맞지 않아요: {e}"))?;
        if !matches!(u.scheme(), "http" | "https") {
            return Err("http(s) 주소만 열어요.".to_string());
        }
        self.check_host(&u).await?;
        let at = now_iso();
        let argv = json!({ "url": raw });
        let resp = match self.web.get(u.as_str()).send().await {
            Ok(r) => r,
            Err(e) => {
                let why = format!("열지 못했어요: {e}");
                let rec = Rec {
                    slug: ctx.slug,
                    stage: ctx.stage,
                    tool: "pc_fetch",
                    kind: "original",
                    args: &argv,
                    asked_at: &at,
                    status: "fail",
                    error: Some(&why),
                    title: raw,
                    url: raw,
                    via: "pc",
                    route: "pc",
                    engine: ctx.run.engine,
                    run: &ctx.run.id,
                    parts: vec![],
                    binary: None,
                    requery: json!({ "주소": raw }),
                };
                let id = self.record(ctx, &rec).await?;
                return Err(format!(
                    "{why} (실패도 근거 기록 {id} 로 남았어요 — 인용하지 마세요)"
                ));
            }
        };
        let final_url = resp.url().to_string();
        let code = i64::from(resp.status().as_u16());
        let mime = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let mut resp = resp;
        let mut bytes: Vec<u8> = Vec::new();
        let mut over = false;
        loop {
            match resp.chunk().await {
                Ok(Some(c)) => {
                    bytes.extend_from_slice(&c);
                    if bytes.len() > RAW_MAX {
                        over = true;
                        bytes.clear();
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let why = format!("받는 도중 끊겼어요: {e}");
                    let rec = Rec {
                        slug: ctx.slug,
                        stage: ctx.stage,
                        tool: "pc_fetch",
                        kind: "original",
                        args: &argv,
                        asked_at: &at,
                        status: "fail",
                        error: Some(&why),
                        title: raw,
                        url: &final_url,
                        via: "pc",
                        route: "pc",
                        engine: ctx.run.engine,
                        run: &ctx.run.id,
                        parts: vec![],
                        binary: None,
                        requery: json!({ "주소": raw }),
                    };
                    let id = self.record(ctx, &rec).await?;
                    return Err(format!("{why} (근거 기록 {id})"));
                }
            }
        }
        let fail = if over {
            Some(format!("원본이 너무 커요(한도 {}MB)", RAW_MAX >> 20))
        } else if code >= 400 {
            Some(format!("HTTP {code}"))
        } else {
            None
        };
        let status = if fail.is_some() {
            "fail"
        } else if bytes.is_empty() {
            "empty"
        } else {
            "ok"
        };
        let is_text = texty(&mime);
        let (text, utf8) = if is_text {
            decode(&bytes, &mime)
        } else {
            (String::new(), false)
        };
        let title = if mime.contains("html") {
            text.split_once("<title")
                .and_then(|x| x.1.split_once('>'))
                .and_then(|x| x.1.split_once("</"))
                .map(|x| html::decode_entities(x.0.trim()))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let title = if title.is_empty() {
            final_url.chars().take(200).collect()
        } else {
            title.chars().take(200).collect::<String>()
        };
        let (parts, binary) = if is_text && utf8 {
            (
                vec![Part {
                    url: &final_url,
                    at: &at,
                    status: Some(code),
                    mime: &mime,
                    body: &text,
                }],
                None,
            )
        } else {
            (
                vec![],
                Some(Binary {
                    url: &final_url,
                    at: &at,
                    status: Some(code),
                    mime: &mime,
                    b64: STANDARD.encode(&bytes),
                }),
            )
        };
        let rec = Rec {
            slug: ctx.slug,
            stage: ctx.stage,
            tool: "pc_fetch",
            kind: "original",
            args: &argv,
            asked_at: &at,
            status,
            error: fail.as_deref(),
            title: &title,
            url: &final_url,
            via: "pc",
            route: "pc",
            engine: ctx.run.engine,
            run: &ctx.run.id,
            parts,
            binary,
            requery: json!({ "주소": raw, "받은 주소": final_url }),
        };
        let id = self.record(ctx, &rec).await?;
        if let Some(f) = fail {
            return Err(format!(
                "{f} — 이 실패도 근거 기록 {id} 로 남았어요(인용하지 마세요)."
            ));
        }
        if !is_text {
            // 문서 파일 — 받은 원문 폴더에 두고 kordoc 으로 읽게 한다
            let last = reqwest::Url::parse(&final_url)
                .ok()
                .and_then(|x| {
                    x.path_segments()
                        .and_then(|mut s| s.next_back().map(str::to_string))
                })
                .unwrap_or_default();
            let name = safe_name(&format!("{id}-{last}"));
            std::fs::write(self.docs.join(&name), &bytes).map_err(|e| {
                format!("받은 파일을 저장하지 못했어요(근거 기록 {id} 은 남았어요): {e}")
            })?;
            let out = json!({ "evidence_id": id, "status": status, "file": name, "mime": mime, "bytes": bytes.len(), "url": final_url,
                              "read": "문서 파일이에요 — pes_local 의 kordoc {slug, stage, file} 로 바꿔 읽으세요" });
            lock(&ctx.run.files).insert(
                name,
                FileRec {
                    evidence: id,
                    url: final_url,
                },
            );
            return Ok(out);
        }
        let view = if mime.contains("html") {
            html::to_text(&text, reqwest::Url::parse(&final_url).ok().as_ref())
        } else {
            text
        };
        let mut p = page(&view, 0);
        if let Some(o) = p.as_object_mut() {
            o.insert("evidence_id".to_string(), json!(id));
            o.insert("url".to_string(), json!(final_url));
            o.insert("title".to_string(), json!(title));
        }
        self.remember(ctx, id, view);
        Ok(p)
    }

    async fn kordoc(&self, ctx: &Ctx<'_>, a: &Value) -> Result<Value, String> {
        let file = str_arg(a, "file").ok_or("file 이 필요해요.")?;
        let (src_ev, src_url) = {
            let files = lock(&ctx.run.files);
            let f = files.get(file).ok_or(
                "이번 실행에서 받아 PES 에 올린 파일만 바꿔요(fetch_url 이 준 file 이름을 쓰세요).",
            )?;
            // clone: 잠금을 풀고(명령이 도는 동안) 근거 기록에 쓴다
            (f.evidence.clone(), f.url.clone())
        };
        let bin = which("kordoc").ok_or("이 컴퓨터에 kordoc 이 없어요.")?;
        let out_name = format!("{file}.md");
        let src = self.docs.join(file);
        let dst = self.docs.join(&out_name);
        let mut args = vec![
            crate::paths::slash(&src),
            "-o".to_string(),
            crate::paths::slash(&dst),
            "--table-format".to_string(),
            "gfm".to_string(),
        ];
        let pages = str_arg(a, "pages");
        if let Some(p) = pages {
            args.extend(["--pages".to_string(), p.to_string()]);
        }
        if a.get("ocr").and_then(Value::as_bool) == Some(true) {
            args.push("--ocr".to_string());
        }
        let at = now_iso();
        let mut c = run_capped(&bin, &args, &self.docs, &[], CLI_SECS).await;
        let md = std::fs::read(&dst)
            .ok()
            .filter(|b| b.len() <= RAW_MAX)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        let fail = c
            .spawn_err
            .take()
            .or_else(|| {
                c.timed_out
                    .then(|| "바꾸기가 너무 오래 걸려 멈췄어요".to_string())
            })
            .or_else(|| {
                (c.code != Some(0)).then(|| {
                    format!(
                        "kordoc 이 실패했어요: {}",
                        String::from_utf8_lossy(&c.stderr)
                            .chars()
                            .take(300)
                            .collect::<String>()
                    )
                })
            });
        let status = if fail.is_some() {
            "fail"
        } else if md.trim().is_empty() {
            "empty"
        } else {
            "ok"
        };
        let argv = json!({ "file": file, "pages": pages, "from_evidence": src_ev });
        let title = format!("{file} (kordoc 변환)");
        let parts = vec![Part {
            url: &src_url,
            at: &at,
            status: c.code.map(i64::from),
            mime: "text/markdown; charset=utf-8",
            body: &md,
        }];
        let rec = Rec {
            slug: ctx.slug,
            stage: ctx.stage,
            tool: "pc_kordoc",
            kind: "original",
            args: &argv,
            asked_at: &at,
            status,
            error: fail.as_deref(),
            title: &title,
            url: &src_url,
            via: "pc",
            route: "pc",
            engine: ctx.run.engine,
            run: &ctx.run.id,
            parts,
            binary: None,
            requery: json!({ "원본 근거": src_ev, "쪽": pages }),
        };
        let id = self.record(ctx, &rec).await?;
        if let Some(f) = fail {
            return Err(format!("{f} (근거 기록 {id})"));
        }
        let mut p = page(&md, 0);
        if let Some(o) = p.as_object_mut() {
            o.insert("evidence_id".to_string(), json!(id));
            o.insert("source_evidence".to_string(), json!(src_ev));
        }
        self.remember(ctx, id, md);
        Ok(p)
    }

    async fn ego(&self, ctx: &Ctx<'_>, a: &Value) -> Result<Value, String> {
        let raw = str_arg(a, "url").ok_or("url 이 필요해요.")?;
        let u = reqwest::Url::parse(raw).map_err(|e| format!("주소가 맞지 않아요: {e}"))?;
        if !matches!(u.scheme(), "http" | "https") {
            return Err("http(s) 주소만 열어요.".to_string());
        }
        self.check_host(&u).await?;
        let bin = which("ego-browser").ok_or("이 컴퓨터에 ego-browser 가 없어요.")?;
        let script = ego_script(u.as_str());
        let at = now_iso();
        let mut c = run_capped(
            &bin,
            &["nodejs".to_string(), "-e".to_string(), script],
            &self.docs,
            &[],
            120,
        )
        .await;
        let all = format!(
            "{}\n{}",
            String::from_utf8_lossy(&c.stdout),
            String::from_utf8_lossy(&c.stderr)
        );
        let got: Option<Value> = all
            .lines()
            .find_map(|l| l.trim().strip_prefix("PES_EGO_RESULT "))
            .and_then(|j| serde_json::from_str(j).ok());
        let argv = json!({ "url": raw });
        let Some(r) = got else {
            let why = c.spawn_err.take().unwrap_or_else(|| {
                format!(
                    "페이지를 읽지 못했어요: {}",
                    all.trim().chars().take(300).collect::<String>()
                )
            });
            let rec = Rec {
                slug: ctx.slug,
                stage: ctx.stage,
                tool: "pc_ego_read",
                kind: "original",
                args: &argv,
                asked_at: &at,
                status: "fail",
                error: Some(&why),
                title: raw,
                url: raw,
                via: "pc",
                route: "pc",
                engine: ctx.run.engine,
                run: &ctx.run.id,
                parts: vec![],
                binary: None,
                requery: json!({ "주소": raw }),
            };
            let id = self.record(ctx, &rec).await?;
            return Err(format!("{why} (실패도 근거 기록 {id} 로 남았어요)"));
        };
        let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or_default();
        let final_url = if s("url").is_empty() { raw } else { s("url") };
        let title: String = s("title").chars().take(200).collect();
        let mut view = s("text").to_string();
        if let Some(links) = r.get("links").and_then(Value::as_array) {
            let l: Vec<String> = links
                .iter()
                .filter_map(|x| {
                    Some(format!(
                        "- {} ({})",
                        x.get(0)?.as_str()?,
                        x.get(1)?.as_str()?
                    ))
                })
                .collect();
            if !l.is_empty() {
                view.push_str("\n\n[링크]\n");
                view.push_str(&l.join("\n"));
            }
        }
        let status = if view.trim().is_empty() {
            "empty"
        } else {
            "ok"
        };
        let parts = vec![Part {
            url: final_url,
            at: &at,
            status: Some(200),
            mime: "text/html; charset=utf-8",
            body: s("html"),
        }];
        let rec = Rec {
            slug: ctx.slug,
            stage: ctx.stage,
            tool: "pc_ego_read",
            kind: "original",
            args: &argv,
            asked_at: &at,
            status,
            error: None,
            title: if title.is_empty() { final_url } else { &title },
            url: final_url,
            via: "pc",
            route: "pc",
            engine: ctx.run.engine,
            run: &ctx.run.id,
            parts,
            binary: None,
            requery: json!({ "주소": raw, "받은 주소": final_url }),
        };
        let id = self.record(ctx, &rec).await?;
        let mut p = page(&view, 0);
        if let Some(o) = p.as_object_mut() {
            o.insert("evidence_id".to_string(), json!(id));
            o.insert("url".to_string(), json!(final_url));
            o.insert("title".to_string(), json!(title));
        }
        self.remember(ctx, id, view);
        Ok(p)
    }
}

struct Ctx<'a> {
    run: &'a Run,
    slug: &'a str,
    stage: Option<&'a str>,
}

/// 근거 기록의 도구 이름 — PC 에서 연 것은 pc_ 를 붙여 서버 도구와 구별한다
fn tool_name(tool: &str) -> &'static str {
    match tool {
        "kosis" => "pc_kosis",
        "vworld" => "pc_vworld",
        "sgis" => "pc_sgis",
        "korean_law" => "pc_korean_law",
        _ => "pc_cli",
    }
}

/// ego-browser 스크립트 — 브리지가 만든다(AI 가 준 JavaScript 는 돌리지 않는다). 공개 페이지를 열어 읽고 탭을 닫는다
fn ego_script(url: &str) -> String {
    let u = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"const want = {u};
let pid = null;
try {{ const ps = await profiles(); pid = (ps || []).find((p) => /^pes$/i.test(String(p.name || "").trim()))?.id ?? null }} catch {{}}
const task = pid ? await taskSpace("PES 외부 AI", {{ profileId: pid }}) : await taskSpace("PES 외부 AI");
const page = await task.newPage();
try {{
  await page.goto(want, {{ timeout: 60000 }});
  try {{ await page.waitForLoadState("networkidle", {{ timeout: 15000 }}) }} catch {{}}
  const r = await page.evaluate(() => ({{ url: location.href, title: document.title, html: document.documentElement.outerHTML, text: document.body ? document.body.innerText : "", links: [...document.querySelectorAll("a[href]")].slice(0, 2000).map((a) => [String(a.innerText || "").trim().slice(0, 200), a.href]) }}));
  console.log("PES_EGO_RESULT " + JSON.stringify(r));
}} finally {{ try {{ await page.close() }} catch {{}} }}"#
    )
}

fn json_resp(status: StatusCode, v: &Value) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        v.to_string(),
    )
        .into_response()
}

async fn handle(
    State(l): State<Arc<Local>>,
    UrlPath(token): UrlPath<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // 이 컴퓨터의 AI 만 — 다른 이름으로 들어온 요청(이름 바꿔치기) · 브라우저의 다른 사이트 스크립트는 받지 않는다
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if host != format!("127.0.0.1:{}", l.port)
        || headers.contains_key("sec-fetch-site")
        || method == Method::OPTIONS
    {
        return json_resp(
            StatusCode::FORBIDDEN,
            &json!({ "error": "이 주소로는 받지 않아요." }),
        );
    }
    if method != Method::POST {
        return (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response();
    }
    let Some(run) = l.find(&token) else {
        return json_resp(
            StatusCode::UNAUTHORIZED,
            &json!({ "error": "실행 토큰이 맞지 않아요(실행이 끝났을 수 있어요)." }),
        );
    };
    if body.len() > 1 << 20 {
        return json_resp(
            StatusCode::PAYLOAD_TOO_LARGE,
            &json!({ "error": "요청이 너무 커요." }),
        );
    }
    let msg: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return json_resp(
                StatusCode::BAD_REQUEST,
                &json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "JSON 이 아니에요." } }),
            );
        }
    };
    if let Some(list) = msg.as_array() {
        let mut out = Vec::new();
        for m in list {
            if let Some(r) = l.rpc(&run, m).await {
                out.push(r);
            }
        }
        if out.is_empty() {
            return StatusCode::ACCEPTED.into_response();
        }
        return json_resp(StatusCode::OK, &Value::Array(out));
    }
    match l.rpc(&run, &msg).await {
        Some(r) => json_resp(StatusCode::OK, &r),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging_keeps_everything() {
        let text: String = "가".repeat(PAGE + 10);
        let p = page(&text, 0);
        assert_eq!(p.get("next_from"), Some(&json!(PAGE)));
        assert_eq!(p.get("chars_total"), Some(&json!(PAGE + 10)));
        let q = page(&text, PAGE);
        assert_eq!(q.get("next_from"), Some(&Value::Null));
        assert_eq!(
            q.get("text")
                .and_then(Value::as_str)
                .map(|s| s.chars().count()),
            Some(10)
        );
    }

    #[test]
    fn internal_hosts_blocked() -> Result<(), Box<dyn std::error::Error>> {
        for u in [
            "http://127.0.0.1:8765/",
            "http://localhost/",
            "http://10.1.2.3/",
            "http://192.168.0.1/",
            "http://[::1]/",
            "http://169.254.169.254/",
            "http://100.64.0.1/",
        ] {
            assert!(blocked_host(&reqwest::Url::parse(u)?), "{u}");
        }
        for u in ["https://kosis.kr/", "https://8.8.8.8/"] {
            assert!(!blocked_host(&reqwest::Url::parse(u)?), "{u}");
        }
        Ok(())
    }

    #[test]
    fn euc_kr_decodes() {
        let (bytes, _, _) = encoding_rs::EUC_KR.encode("인구 통계");
        let (t, utf8) = decode(&bytes, "text/html; charset=euc-kr");
        assert_eq!(t, "인구 통계");
        assert!(!utf8, "UTF-8 이 아니면 원본은 바이트로 올린다");
    }

    #[test]
    fn names_are_safe() {
        assert_eq!(safe_name("ev_x-../../etc/passwd"), "ev_x-.._.._etc_passwd");
        assert_eq!(safe_name(""), "file");
    }
}
