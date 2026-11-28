//! pes_local — 브리지가 127.0.0.1 에 여는 MCP 서버(AI 본체가 부르고, 셸 샌드박스 밖에서 브리지가 실행한다. 티켓 21 7절).
//! 도구: 조사 CLI(kosis · vworld · sgis · korean_law) · kordoc(받은 문서 변환) · fetch_url(PC 에서 원문 열기) · ego_browser(감싼 브라우저) · read_more(긴 결과 이어 읽기).
//! 도구마다 요청 · 응답 원본을 PES 근거 기록으로 올리고(POST /mcp/evidence) 받은 번호(evidence_id)를 답에 붙인다.
//! 올리지 못하면 그 도구 호출은 실패로 돌려준다 — 결과를 AI 에 주지 않는다(근거 기록 저장 실패 = 실패).
//! 실행(라운드)마다 새 토큰(Authorization: Bearer — 명령줄 · 주소에는 넣지 않는다)과 새 받은 원문 폴더(~/.pes/docs/<계정>/<실행>)를 쓴다. 닫으면 둘 다 없앤다.
use crate::cli;
use crate::guard::Guard;
use crate::html;
use crate::log::{Event, line};
use crate::paths::{child_path, which};
use crate::pes::Pes;
use crate::proc::{kill_tree, own_group};
use crate::time::now_iso;
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

/// 한 호출 원본 한도 — PES 근거 원본 한도(src/evidence.ts RAW_MAX)와 같다. 넘으면 그 조회는 실패
pub const RAW_MAX: usize = 25 * 1024 * 1024;
/// AI 에 한 번에 주는 글자 수 — 넘으면 next_from 으로 이어 읽게 한다(잘라 버리지 않는다)
pub const PAGE: usize = 40_000;
/// 이어 읽기용으로 한 실행이 들고 있는 글의 한도 — 넘으면 오래된 것부터 버린다
const CACHE_MAX: usize = 64 * 1024 * 1024;
const CLI_SECS: u64 = 600;
/// 이보다 큰 원본은 한 번에 보내지 않고 바이트 그대로 조각으로 나눠 올린다(사용자 결정 2026-11-28, PES-API.md 5절)
pub const UPLOAD_OVER: usize = 1024 * 1024;

/// 조사 CLI — (도구 이름, 안내)
const CLIS: [(&str, &str); 4] = [
    (
        "kosis",
        "KOSIS 국가통계 CLI(search · meta · data · list · explain · indicator)",
    ),
    (
        "vworld",
        "VWorld 공간정보 CLI(geocode · geocoder · search · data · ned · wms · wfs · catalog)",
    ),
    (
        "sgis",
        "SGIS 통계지리 CLI(data · boundary · geocode · search · code)",
    ),
    (
        "korean_law",
        "법령 CLI(query · search_* · get_* · compare_* 등 조회 도구)",
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

/// 이어 읽기용 글 — 크기 한도 안에서 오래된 것부터 버린다
#[derive(Default)]
struct Cache {
    /// (근거 번호, 프로젝트, AI 가 읽는 글)
    items: VecDeque<(String, String, String)>,
    bytes: usize,
}

impl Cache {
    fn put(&mut self, id: String, slug: &str, text: String) {
        self.bytes += text.len();
        self.items.push_back((id, slug.to_string(), text));
        while self.bytes > CACHE_MAX && self.items.len() > 1 {
            if let Some((_, _, t)) = self.items.pop_front() {
                self.bytes -= t.len();
            }
        }
    }
    fn get(&self, id: &str) -> Option<(&str, &str)> {
        self.items
            .iter()
            .find(|(i, _, _)| i == id)
            .map(|(_, s, t)| (s.as_str(), t.as_str()))
    }
}

/// 실행(라운드) 하나
pub struct Run {
    pub id: String,
    pub slugs: Vec<String>,
    pub engine: &'static str,
    /// 이 실행의 받은 원문 폴더
    pub docs: PathBuf,
    pub stats: Mutex<Stats>,
    cache: Mutex<Cache>,
    /// 받은 문서 파일 이름 → 근거 기록(kordoc 은 이것만 바꾼다)
    files: Mutex<HashMap<String, FileRec>>,
    seq: AtomicU32,
}

pub struct Opened {
    pub token: String,
    pub url: String,
    pub docs: PathBuf,
    pub id: String,
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
    /// 원본 바이트 — 작으면 record 가 b64 로 바꿔 싣고, 크면 나눠 올린다
    #[serde(skip)]
    bytes: &'a [u8],
    #[serde(skip_serializing_if = "Option::is_none")]
    b64: Option<String>,
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
    /// 나눠 올린 원본 {file_id, sha256, size, mime, url, at, status} — 있으면 원본은 그 파일, parts 는 덧붙은 작은 글만
    #[serde(skip_serializing_if = "Option::is_none")]
    upload: Option<Value>,
    /// AI 에게 준 첫 쪽 범위 {chars, ranges: [[0, n]], pages}
    #[serde(skip_serializing_if = "Option::is_none")]
    seen: Option<Value>,
    requery: Value,
}

impl<'a> Rec<'a> {
    fn fail(
        ctx: &'a Ctx<'a>,
        tool: &'a str,
        args: &'a Value,
        at: &'a str,
        why: &'a str,
        url: &'a str,
    ) -> Self {
        Rec {
            slug: ctx.slug,
            stage: ctx.stage,
            tool,
            kind: "original",
            args,
            asked_at: at,
            status: "fail",
            error: Some(why),
            title: url,
            url,
            via: "pc",
            route: "pc",
            engine: ctx.run.engine,
            run: &ctx.run.id,
            parts: vec![],
            binary: None,
            upload: None,
            seen: None,
            requery: json!({ "주소": url }),
        }
    }
}

pub struct Local {
    pub port: u16,
    runs: Mutex<HashMap<String, Arc<Run>>>,
    pes: Arc<Pes>,
    /// ~/.pes/docs/<계정>
    docs_root: PathBuf,
    home: PathBuf,
    guard: Guard,
    web: reqwest::Client,
    /// Ego Lite 의 「PES」 프로필(로그인 없는 전용). 없으면 ego_browser 는 끈다
    ego_profile: Option<String>,
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
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

fn with<const N: usize>(mut p: Value, kv: [(&str, Value); N]) -> Value {
    if let Some(o) = p.as_object_mut() {
        for (k, v) in kv {
            o.insert(k.to_string(), v);
        }
    }
    p
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

/// (읽은 것, 한도를 넘었나, 읽기 오류가 났나)
async fn read_cap<R: AsyncRead + Unpin>(r: Option<R>, cap: usize) -> (Vec<u8>, bool, bool) {
    let Some(mut r) = r else {
        return (Vec::new(), false, true);
    };
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match r.read(&mut buf).await {
            Ok(0) => return (out, false, false),
            Err(_) => return (out, false, true),
            Ok(n) => {
                out.extend_from_slice(buf.get(..n).unwrap_or_default());
                if out.len() > cap {
                    out.truncate(cap);
                    return (out, true, false);
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
    read_err: bool,
    spawn_err: Option<String>,
}

impl Capped {
    /// 실패 까닭(없으면 None)
    fn failure(&mut self, bin: &str) -> Option<String> {
        self.spawn_err
            .take()
            .map(|e| format!("{bin} 을 띄우지 못했어요: {e}"))
            .or_else(|| {
                self.timed_out
                    .then(|| format!("{CLI_SECS}초가 넘어 멈췄어요"))
            })
            .or_else(|| {
                self.over
                    .then(|| format!("원본이 너무 커요(한도 {}MB)", RAW_MAX >> 20))
            })
            .or_else(|| {
                self.read_err
                    .then(|| format!("{bin} 출력을 끝까지 읽지 못했어요"))
            })
            .or_else(|| {
                (self.code != Some(0)).then(|| {
                    format!(
                        "{bin} 이 {} 로 끝났어요",
                        self.code.map_or("신호".to_string(), |x| x.to_string())
                    )
                })
            })
    }
}

fn kill_logged(pid: Option<u32>, what: &str) {
    if let Some(p) = pid
        && let Err(e) = kill_tree(p)
    {
        line(&format!("{what}: {e}"));
    }
}

/// 명령을 셸 없이(인자 배열 그대로) 새 프로세스 묶음으로 돌린다 — 출력은 RAW_MAX 바이트까지, secs 초 안에. 넘으면 묶음째 끝낸다
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
    own_group(&mut cmd);
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
    let pid = child.id();
    let (so, se) = (child.stdout.take(), child.stderr.take());
    let all = async {
        let ((out, over, e1), (err, _, e2)) =
            tokio::join!(read_cap(so, RAW_MAX), read_cap(se, 1 << 20));
        if over {
            kill_logged(pid, "출력이 너무 큰 명령을 끝내지 못했어요");
        }
        let code = child.wait().await.ok().and_then(|s| s.code());
        Capped {
            code,
            stdout: out,
            stderr: err,
            over,
            read_err: e1 || e2,
            ..Capped::default()
        }
    };
    match tokio::time::timeout(Duration::from_secs(secs), all).await {
        Ok(c) => c,
        Err(_) => {
            kill_logged(pid, "멈춘 명령을 끝내지 못했어요");
            Capped {
                timed_out: true,
                ..Capped::default()
            }
        }
    }
}

/// 기록에 남길 인자 — 이 컴퓨터의 키 값(그대로 · 주소 꼴 · base64 속 값)을 지운 것
fn shown_args(args: &[String], hide: &[String]) -> Vec<String> {
    args.iter().map(|a| cli::scrub(a, hide)).collect()
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

/// Ego Lite 의 「PES」 프로필 id — 없거나 못 읽으면 None(그러면 ego_browser 를 끈다: 로그인한 프로필로 열지 않게)
async fn ego_profile(docs: &Path) -> Option<String> {
    let bin = which("ego-browser")?;
    let script = r#"try { const ps = await profiles(); const p = (ps || []).find((x) => /^pes$/i.test(String(x.name || "").trim())); console.log("PES_EGO_PROFILE " + JSON.stringify(p ? String(p.id) : null)) } catch (e) { console.log("PES_EGO_PROFILE null") }"#;
    for _ in 0..3 {
        let c = run_capped(
            &bin,
            &["nodejs".to_string(), "-e".to_string(), script.to_string()],
            docs,
            &[],
            30,
        )
        .await;
        let all = format!(
            "{}\n{}",
            String::from_utf8_lossy(&c.stdout),
            String::from_utf8_lossy(&c.stderr)
        );
        if let Some(j) = all
            .lines()
            .find_map(|l| l.trim().strip_prefix("PES_EGO_PROFILE "))
        {
            return serde_json::from_str::<Option<String>>(j).ok().flatten();
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    None
}

impl Local {
    pub async fn start(
        pes: Arc<Pes>,
        docs_root: PathBuf,
        home: PathBuf,
    ) -> anyhow::Result<Arc<Self>> {
        let off = std::env::var("PES_BRIDGE_ALLOW_LOOPBACK_FETCH").is_ok_and(|v| v == "1");
        if off {
            line(
                "시험 설정: 원문 열기의 내부 주소 막기를 껐어요(PES_BRIDGE_ALLOW_LOOPBACK_FETCH=1)",
            );
        }
        let guard = Guard::new(Arc::new(crate::guard::SystemLookup), Vec::new(), off);
        let web = guard.client("Mozilla/5.0 (compatible; pes-bridge)")?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        std::fs::create_dir_all(&docs_root)?;
        let ego = ego_profile(&docs_root).await;
        match (&ego, which("ego-browser")) {
            (Some(p), _) => line(&format!("ego-browser: 「PES」 프로필({p})로만 열어요")),
            (None, Some(_)) => line(
                "ego-browser: 「PES」 프로필이 없어 ego_browser 도구를 끕니다 — 로그인 없는 전용 프로필을 Ego Lite 에서 「PES」 이름으로 만들면 켜져요",
            ),
            (None, None) => {}
        }
        let local = Arc::new(Self {
            port,
            runs: Mutex::new(HashMap::new()),
            pes,
            docs_root,
            home,
            guard,
            web,
            ego_profile: ego,
        });
        // clone: 서버 작업(axum)과 브리지(실행 열기 · 닫기)가 같은 Local 을 함께 쓴다
        let app = Router::new()
            .route("/mcp", any(handle))
            .with_state(Arc::clone(&local));
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                line(&format!("pes_local 서버가 멈췄어요: {e}"));
            }
        });
        Ok(local)
    }

    /// 실행 하나를 연다 — 토큰 · 주소(토큰 없음) · 받은 원문 폴더
    pub fn open(&self, slugs: Vec<String>, engine: &'static str) -> anyhow::Result<Opened> {
        let token = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let id = hex::encode(rand::random::<[u8; 4]>());
        let docs = self.docs_root.join(&id);
        std::fs::create_dir_all(&docs)?;
        let run = Run {
            id,
            slugs,
            engine,
            docs,
            stats: Mutex::new(Stats::default()),
            cache: Mutex::new(Cache::default()),
            files: Mutex::new(HashMap::new()),
            seq: AtomicU32::new(1),
        };
        // clone: 이 값들은 표(실행)와 부른 쪽(AI 설정 · 닫기) 두 곳이 쓴다
        let opened = Opened {
            token: token.clone(),
            url: format!("http://127.0.0.1:{}/mcp", self.port),
            docs: run.docs.clone(),
            id: run.id.clone(),
        };
        lock(&self.runs).insert(token, Arc::new(run));
        Ok(opened)
    }

    /// 실행을 닫는다 → 그 실행(통계). 그 뒤 토큰은 401, 받은 원문 폴더는 지운다(원본은 PES 근거 기록에 있다)
    pub fn close(&self, token: &str) -> Option<Arc<Run>> {
        let run = lock(&self.runs).remove(token)?;
        if let Err(e) = std::fs::remove_dir_all(&run.docs) {
            line(&format!(
                "받은 원문 폴더를 지우지 못했어요({}): {e}",
                run.docs.display()
            ));
        }
        Some(run)
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
        for (name, about) in CLIS {
            let Some(bin) = cli::bin_of(name) else {
                continue;
            };
            if which(bin).is_some() {
                v.push(json!({ "name": name, "description": format!("{about}를 이 컴퓨터에서 돌린다(셸 없이). args 의 첫 값은 하위 명령, 그 뒤는 인자(예: [\"query\", \"민법 제1조\"]). 파일을 읽고 쓰는 옵션(-o · --output · --input · --config …)은 쓸 수 없다. 요청 · 응답 원본은 PES 근거 기록으로 남고 evidence_id 가 붙어 온다 — 그 결과로 쓴 문장에 [^evidence_id] 를 단다. 결과가 길면 read_more 로 끝까지 읽는다."),
                    "inputSchema": { "type": "object", "properties": base(json!({ "args": { "type": "array", "items": { "type": "string" }, "description": format!("{bin} CLI 인자") } })), "required": ["slug", "args"] } }));
            }
        }
        if which("kordoc").is_some() {
            v.push(json!({ "name": "kordoc", "description": "fetch_url 로 받은 문서 파일(HWP · HWPX · PDF · 오피스)을 Markdown 으로 바꿔 읽는다. file 은 그 도구가 돌려준 file 이름. 바꾼 글도 근거 기록으로 남는다.",
                "inputSchema": { "type": "object", "properties": base(json!({ "file": { "type": "string" }, "pages": { "type": "string", "description": "쪽 범위(예: 1-20)" }, "ocr": { "type": "boolean" } })), "required": ["slug", "file"] } }));
        }
        v.push(json!({ "name": "fetch_url", "description": "웹 원문을 이 컴퓨터에서 연다(내장 웹 가져오기 대신). 받은 원본은 PES 근거 기록으로 남고 evidence_id 가 붙어 온다. HTML 은 읽을 글(링크 포함)로, 문서 파일은 file 이름으로 돌려준다(kordoc 으로 읽는다). 이 컴퓨터 · 사설망 주소는 열지 않는다.",
            "inputSchema": { "type": "object", "properties": base(json!({ "url": { "type": "string", "description": "http(s) 주소" } })), "required": ["slug", "url"] } }));
        if self.ego_profile.is_some() && which("ego-browser").is_some() {
            v.push(json!({ "name": "ego_browser", "description": "fetch_url 로 내용이 안 나오는 화면(자바스크립트로 그리는 화면 · 통계 표 · 지도)을 브라우저(로그인 없는 「PES」 프로필)로 열어 읽는다. 공개 페이지 읽기만 한다. 연 페이지 원본은 PES 근거 기록으로 남는다.",
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
            return self.read_more(run, a).await;
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
        if CLIS.iter().any(|(n, _)| *n == name) {
            return self.cli(&ctx, name, a).await;
        }
        match name {
            "kordoc" => self.kordoc(&ctx, a).await,
            "fetch_url" => self.fetch(&ctx, a).await,
            "ego_browser" if self.ego_profile.is_some() => self.ego(&ctx, a).await,
            "ego_browser" => Err("ego_browser 를 쓸 수 없어요 — Ego Lite 에 「PES」 프로필(로그인 없는 전용)이 없어요.".to_string()),
            _ => Err(format!("없는 도구예요: {name}")),
        }
    }

    /// 근거 기록을 올리고 번호를 받는다. 못 받으면 Err — 그 호출은 실패로 돌려준다.
    /// view_chars: AI 에게 줄 글의 길이(첫 쪽 범위를 seen 으로 함께 보낸다). 원본이 UPLOAD_OVER 보다 크면 먼저 나눠 올린다
    async fn record(
        &self,
        ctx: &Ctx<'_>,
        mut rec: Rec<'_>,
        view_chars: Option<usize>,
    ) -> Result<String, String> {
        if let Some(n) = view_chars.filter(|n| *n > 0) {
            rec.seen = Some(json!({ "chars": n, "ranges": [[0, n.min(PAGE)]], "pages": 1 }));
        }
        let tool = rec.tool;
        let url = rec.url;
        match self.send_record(ctx, &mut rec).await {
            Ok(id) => {
                // clone: 번호는 실행 통계(작업 로그)와 도구 답 두 곳에 들어간다
                lock(&ctx.run.stats).recorded.push(id.clone());
                Ok(id)
            }
            Err(e) => {
                lock(&ctx.run.stats).upload_failed += 1;
                let why = format!("{e:#}");
                line(&format!(
                    "pes_local {tool}: 근거 기록을 PES 에 올리지 못했어요 — {why}"
                ));
                let ev = Event {
                    action: "bridge_evidence_fail", project: Some(ctx.slug), stage: ctx.stage, target: Some(tool), how: format!("pes_local {tool} → POST /mcp/evidence"),
                    why_kind: "rule", why: "PC 에서 한 조회는 원본을 PES 근거 기록으로 올려야 성공으로 친다(티켓 21 · 22)".to_string(), result: "fail",
                    detail: json!({ "error": why, "url": url, "run": ctx.run.id }),
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

    /// 원본 크기에 따라 한 번에(글 · base64) 또는 나눠 올린 뒤 근거 기록을 만든다
    async fn send_record(&self, ctx: &Ctx<'_>, rec: &mut Rec<'_>) -> anyhow::Result<String> {
        let size = rec.binary.as_ref().map_or_else(
            || rec.parts.iter().map(|p| p.body.len()).sum(),
            |b| b.bytes.len(),
        );
        let mut expect: Option<String> = None;
        if size <= UPLOAD_OVER {
            if let Some(b) = rec.binary.as_mut() {
                b.b64 = Some(STANDARD.encode(b.bytes));
            }
        } else {
            // 원본(바이트 그대로)을 먼저 나눠 올리고, 근거 기록은 그 파일 번호와 지문으로 만든다
            let (bytes, mime, url, at, status) = match rec.binary.take() {
                Some(b) => (b.bytes, b.mime, b.url, b.at, b.status),
                None if !rec.parts.is_empty() => {
                    let p = rec.parts.remove(0);
                    (p.body.as_bytes(), p.mime, p.url, p.at, p.status)
                }
                None => (
                    &[][..],
                    "application/octet-stream",
                    rec.url,
                    rec.asked_at,
                    None,
                ),
            };
            let name = safe_name(&format!("{}-{}.raw", rec.tool, ctx.run.id));
            let (file_id, sha) = self.pes.upload_raw(ctx.slug, &name, mime, bytes).await?;
            rec.upload = Some(
                json!({ "file_id": file_id, "sha256": sha, "size": bytes.len(), "mime": mime, "url": url, "at": at, "status": status }),
            );
            expect = Some(sha);
        }
        let v = serde_json::to_value(&*rec)?;
        self.pes.evidence(&v, expect.as_deref()).await
    }

    /// 이어 읽기 — 준 범위를 PES 에 더한다(POST /mcp/evidence-seen)
    async fn read_more(&self, run: &Run, a: &Value) -> Result<Value, String> {
        let id = str_arg(a, "evidence_id").ok_or("evidence_id 가 필요해요.")?;
        let from = a
            .get("from")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        let (slug, p, total) = {
            let cache = lock(&run.cache);
            let (slug, text) = cache.get(id).ok_or(
                "이번 실행에서 받은 결과가 아니거나, 오래돼 지웠어요(같은 도구로 다시 조회하세요).",
            )?;
            let total = text.chars().count();
            (
                slug.to_string(),
                with(page(text, from), [("evidence_id", json!(id))]),
                total,
            )
        };
        let end = (from + PAGE).min(total);
        if from < end {
            // 실패해도 이 도구 결과는 실패로 하지 않는다 — 글은 이미 이 컴퓨터 안에서 AI 에게 주는 중이고, 근거 원본 자체는 올라가 있다.
            // 그 범위는 PES 에 「본 범위」로 남지 않아, 그 부분을 근거로 다는 것은 PES 검사에서 걸린다(로그로 남긴다)
            if let Err(e) = self
                .pes
                .evidence_seen(&slug, id, total, &[[from, end]])
                .await
            {
                line(&format!(
                    "pes_local read_more: 본 범위를 PES 에 남기지 못했어요({id} {from}..{end}) — {e:#}"
                ));
            }
        }
        Ok(p)
    }

    async fn cli(&self, ctx: &Ctx<'_>, tool: &str, a: &Value) -> Result<Value, String> {
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
        cli::check_args(tool, &args)?;
        let bin = cli::bin_of(tool).ok_or("없는 조사 도구예요.")?;
        let path = which(bin).ok_or_else(|| format!("이 컴퓨터에 {bin} 이 없어요."))?;
        if let Some(why) = cli::missing_key(tool, &self.home) {
            return Err(why);
        }
        let hide = cli::scrub_list(&cli::local_keys(tool, &self.home));
        let envs: Vec<(String, String)> = law_oc(&self.home)
            .map(|v| vec![("LAW_OC".to_string(), v)])
            .unwrap_or_default();
        let at = now_iso();
        let mut c = run_capped(&path, &args, &ctx.run.docs, &envs, CLI_SECS).await;
        let fail = c.failure(bin);
        // 출력에 이 컴퓨터의 키가 섞여 오면 지운다(근거 기록 · AI 답 모두)
        let out = cli::scrub(&String::from_utf8_lossy(&c.stdout), &hide);
        let err = cli::scrub(&String::from_utf8_lossy(&c.stderr), &hide);
        let status = if fail.is_some() {
            "fail"
        } else if out.trim().is_empty() {
            "empty"
        } else {
            "ok"
        };
        let first = args.first().map(String::as_str).unwrap_or_default();
        let kind = if [
            "search",
            "s",
            "list",
            "ls",
            "catalog",
            "suggest_law_names",
            "search_all",
        ]
        .contains(&first)
            || first.starts_with("search_")
        {
            "search"
        } else {
            "lookup"
        };
        // 기록 · 제목 · 다시 조회할 조건에는 키를 지운 인자만 쓴다(CLI 에는 받은 그대로 넘긴다)
        let shown = shown_args(&args, &hide);
        let cmdline = format!("cli:{bin} {}", shown.join(" "));
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
        let argv = json!({ "cli": bin, "args": shown });
        let title = format!(
            "{bin} {}",
            shown.join(" ").chars().take(120).collect::<String>()
        );
        let tname = format!("pc_{tool}");
        let rec = Rec {
            slug: ctx.slug,
            stage: ctx.stage,
            tool: &tname,
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
            upload: None,
            seen: None,
            requery: json!({ "명령": bin, "인자": argv.get("args") }),
        };
        let view = fail.is_none().then(|| out.chars().count());
        let id = self.record(ctx, rec, view).await?;
        if let Some(f) = fail {
            return Err(format!(
                "{f}. 이 실패도 근거 기록 {id} 로 남았어요(인용하지 마세요). 출력: {}",
                format!("{err}{out}").chars().take(1500).collect::<String>()
            ));
        }
        let p = with(
            page(&out, 0),
            [("evidence_id", json!(id)), ("status", json!(status))],
        );
        lock(&ctx.run.cache).put(id, ctx.slug, out);
        Ok(p)
    }

    async fn fetch(&self, ctx: &Ctx<'_>, a: &Value) -> Result<Value, String> {
        let raw = str_arg(a, "url").ok_or("url 이 필요해요.")?;
        let u = reqwest::Url::parse(raw).map_err(|e| format!("주소가 맞지 않아요: {e}"))?;
        if !matches!(u.scheme(), "http" | "https") {
            return Err("http(s) 주소만 열어요.".to_string());
        }
        self.guard.check_url(&u).await?;
        let at = now_iso();
        let argv = json!({ "url": raw });
        let resp = match self.web.get(u.as_str()).send().await {
            Ok(r) => r,
            Err(e) => {
                let why = format!("열지 못했어요: {}", error_chain(&e));
                let id = self
                    .record(ctx, Rec::fail(ctx, "pc_fetch", &argv, &at, &why, raw), None)
                    .await?;
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
                    let id = self
                        .record(
                            ctx,
                            Rec::fail(ctx, "pc_fetch", &argv, &at, &why, &final_url),
                            None,
                        )
                        .await?;
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
        let title: String = if title.is_empty() {
            final_url.chars().take(200).collect()
        } else {
            title.chars().take(200).collect()
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
                    bytes: &bytes,
                    b64: None,
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
            upload: None,
            seen: None,
            requery: json!({ "주소": raw, "받은 주소": final_url }),
        };
        // AI 에게 줄 글(HTML 은 읽을 글로) — 첫 쪽 범위를 근거와 함께 보낸다
        let html_view = (is_text && mime.contains("html"))
            .then(|| html::to_text(&text, reqwest::Url::parse(&final_url).ok().as_ref()));
        let chars = (fail.is_none() && is_text).then(|| {
            html_view
                .as_ref()
                .map_or_else(|| text.chars().count(), |v| v.chars().count())
        });
        let id = self.record(ctx, rec, chars).await?;
        if let Some(f) = fail {
            return Err(format!(
                "{f} — 이 실패도 근거 기록 {id} 로 남았어요(인용하지 마세요)."
            ));
        }
        if !is_text {
            // 문서 파일 — 이 실행의 받은 원문 폴더에 두고 kordoc 으로 읽게 한다
            let last = reqwest::Url::parse(&final_url)
                .ok()
                .and_then(|x| {
                    x.path_segments()
                        .and_then(|mut s| s.next_back().map(str::to_string))
                })
                .unwrap_or_default();
            let name = safe_name(&format!("{id}-{last}"));
            std::fs::write(ctx.run.docs.join(&name), &bytes).map_err(|e| {
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
        let view = html_view.unwrap_or(text);
        let p = with(
            page(&view, 0),
            [
                ("evidence_id", json!(id)),
                ("url", json!(final_url)),
                ("title", json!(title)),
            ],
        );
        lock(&ctx.run.cache).put(id, ctx.slug, view);
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
        let n = ctx.run.seq.fetch_add(1, Ordering::Relaxed);
        let src = ctx.run.docs.join(file);
        let dst = ctx.run.docs.join(format!("{file}.{n}.md")); // 부를 때마다 다른 이름(앞 결과를 덮지 않게)
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
        let mut c = run_capped(&bin, &args, &ctx.run.docs, &[], CLI_SECS).await;
        let md = match std::fs::metadata(&dst) {
            Ok(m) if m.len() > RAW_MAX as u64 => {
                Err(format!("변환한 글이 너무 커요(한도 {}MB)", RAW_MAX >> 20))
            }
            Ok(_) => std::fs::read(&dst)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .map_err(|e| format!("변환한 글을 읽지 못했어요: {e}")),
            Err(_) => Err("변환한 글이 없어요".to_string()),
        };
        let fail = c
            .failure("kordoc")
            .map(|f| {
                format!(
                    "{f}: {}",
                    String::from_utf8_lossy(&c.stderr)
                        .chars()
                        .take(300)
                        .collect::<String>()
                )
            })
            .or_else(|| md.as_ref().err().map(String::to_string))
            .or_else(|| {
                md.as_ref()
                    .ok()
                    .filter(|m| m.trim().is_empty())
                    .map(|_| "변환한 글이 비어 있어요(스캔본이면 ocr: true)".to_string())
            });
        let md = md.unwrap_or_default();
        let status = if fail.is_some() { "fail" } else { "ok" };
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
            upload: None,
            seen: None,
            requery: json!({ "원본 근거": src_ev, "쪽": pages }),
        };
        let view = fail.is_none().then(|| md.chars().count());
        let id = self.record(ctx, rec, view).await?;
        if let Some(f) = fail {
            return Err(format!("{f} (근거 기록 {id} — 인용하지 마세요)"));
        }
        let p = with(
            page(&md, 0),
            [
                ("evidence_id", json!(id)),
                ("source_evidence", json!(src_ev)),
            ],
        );
        lock(&ctx.run.cache).put(id, ctx.slug, md);
        Ok(p)
    }

    async fn ego(&self, ctx: &Ctx<'_>, a: &Value) -> Result<Value, String> {
        let raw = str_arg(a, "url").ok_or("url 이 필요해요.")?;
        let u = reqwest::Url::parse(raw).map_err(|e| format!("주소가 맞지 않아요: {e}"))?;
        if !matches!(u.scheme(), "http" | "https") {
            return Err("http(s) 주소만 열어요.".to_string());
        }
        self.guard.check_url(&u).await?;
        let profile = self
            .ego_profile
            .as_deref()
            .ok_or("「PES」 프로필이 없어요.")?;
        let bin = which("ego-browser").ok_or("이 컴퓨터에 ego-browser 가 없어요.")?;
        let script = ego_script(u.as_str(), profile);
        let at = now_iso();
        let c = run_capped(
            &bin,
            &["nodejs".to_string(), "-e".to_string(), script],
            &ctx.run.docs,
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
            let why = c.spawn_err.unwrap_or_else(|| {
                format!(
                    "페이지를 읽지 못했어요: {}",
                    all.trim().chars().take(300).collect::<String>()
                )
            });
            let id = self
                .record(
                    ctx,
                    Rec::fail(ctx, "pc_ego_read", &argv, &at, &why, raw),
                    None,
                )
                .await?;
            return Err(format!("{why} (실패도 근거 기록 {id} 로 남았어요)"));
        };
        let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or_default();
        let final_url = if s("url").is_empty() { raw } else { s("url") };
        // 페이지가 넘긴 곳(최종 주소)도 다시 본다 — 내부 주소면 내용을 쓰지 않는다
        let inside = match reqwest::Url::parse(final_url) {
            Ok(fu) => self.guard.check_url(&fu).await.err(),
            Err(_) => Some("최종 주소를 읽지 못했어요".to_string()),
        };
        if let Some(why) = inside {
            let why = format!("브라우저가 넘어간 주소를 쓸 수 없어요: {why}");
            let id = self
                .record(
                    ctx,
                    Rec::fail(ctx, "pc_ego_read", &argv, &at, &why, raw),
                    None,
                )
                .await?;
            return Err(format!("{why} (근거 기록 {id})"));
        }
        let code = r.get("status").and_then(Value::as_i64);
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
        let fail = code.filter(|c| *c >= 400).map(|c| format!("HTTP {c}"));
        let status = if fail.is_some() {
            "fail"
        } else if view.trim().is_empty() {
            "empty"
        } else {
            "ok"
        };
        let parts = vec![Part {
            url: final_url,
            at: &at,
            status: code,
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
            error: fail.as_deref(),
            title: if title.is_empty() { final_url } else { &title },
            url: final_url,
            via: "pc",
            route: "pc",
            engine: ctx.run.engine,
            run: &ctx.run.id,
            parts,
            binary: None,
            upload: None,
            seen: None,
            requery: json!({ "주소": raw, "받은 주소": final_url }),
        };
        let chars = fail.is_none().then(|| view.chars().count());
        let id = self.record(ctx, rec, chars).await?;
        if let Some(f) = fail {
            return Err(format!(
                "{f} — 이 실패도 근거 기록 {id} 로 남았어요(인용하지 마세요)."
            ));
        }
        let p = with(
            page(&view, 0),
            [
                ("evidence_id", json!(id)),
                ("url", json!(final_url)),
                ("title", json!(title)),
                ("http_status", json!(code)),
            ],
        );
        lock(&ctx.run.cache).put(id, ctx.slug, view);
        Ok(p)
    }
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(x) = src {
        s.push_str(&format!(" — {x}"));
        src = x.source();
    }
    s
}

struct Ctx<'a> {
    run: &'a Run,
    slug: &'a str,
    stage: Option<&'a str>,
}

/// ego-browser 스크립트 — 브리지가 만든다(AI 가 준 JavaScript 는 돌리지 않는다). 「PES」 프로필로만 열고, 응답 코드를 받아 적고, 탭을 닫는다
fn ego_script(url: &str, profile: &str) -> String {
    let u = serde_json::to_string(url).unwrap_or_else(|_| "\"\"".to_string());
    let p = serde_json::to_string(profile).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"const want = {u};
const task = await taskSpace("PES 외부 AI", {{ profileId: {p} }});
const page = await task.newPage();
try {{
  await page.goto(want, {{ timeout: 60000 }});
  try {{ await page.waitForLoadState("networkidle", {{ timeout: 15000 }}) }} catch {{}}
  const r = await page.evaluate(() => {{ const nav = performance.getEntriesByType("navigation")[0]; return {{ url: location.href, status: nav && nav.responseStatus ? nav.responseStatus : null, title: document.title, html: document.documentElement.outerHTML, text: document.body ? document.body.innerText : "", links: [...document.querySelectorAll("a[href]")].slice(0, 2000).map((a) => [String(a.innerText || "").trim().slice(0, 200), a.href]) }} }});
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
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    let Some(run) = l.find(token) else {
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
    fn cache_drops_oldest_over_limit() {
        let mut c = Cache::default();
        c.put("a".into(), "p", "x".repeat(CACHE_MAX / 2 + 1));
        c.put("b".into(), "p", "y".repeat(CACHE_MAX / 2 + 1));
        assert!(c.get("a").is_none() && c.get("b").is_some());
        assert!(c.bytes <= CACHE_MAX);
    }

    #[test]
    fn euc_kr_decodes() {
        let (bytes, _, _) = encoding_rs::EUC_KR.encode("인구 통계");
        let (t, utf8) = decode(&bytes, "text/html; charset=euc-kr");
        assert_eq!(t, "인구 통계");
        assert!(!utf8, "UTF-8 이 아니면 원본은 바이트로 올린다");
    }

    #[test]
    fn recorded_args_have_no_keys() {
        let hide = cli::scrub_list(&["realkey0123456789".to_string()]);
        let args = vec![
            "data".to_string(),
            "--key=realkey0123456789".to_string(),
            "x realkey0123456789 y".to_string(),
        ];
        let shown = shown_args(&args, &hide);
        assert!(
            shown.iter().all(|a| !a.contains("realkey0123456789")),
            "{shown:?}"
        );
        assert_eq!(shown[0], "data");
        assert_eq!(shown[1], "--key=(지움)");
    }

    #[test]
    fn names_are_safe() {
        assert_eq!(safe_name("ev_x-../../etc/passwd"), "ev_x-.._.._etc_passwd");
        assert_eq!(safe_name(""), "file");
    }

    #[test]
    fn capped_failures() {
        let mut c = Capped {
            code: Some(0),
            read_err: true,
            ..Capped::default()
        };
        assert!(
            c.failure("kosis")
                .is_some_and(|f| f.contains("끝까지 읽지 못했어요"))
        );
        let mut ok = Capped {
            code: Some(0),
            ..Capped::default()
        };
        assert!(ok.failure("kosis").is_none());
    }

    #[tokio::test]
    async fn cli_timeout_kills_the_whole_group() -> Result<(), Box<dyn std::error::Error>> {
        let d = tempfile::tempdir()?;
        let pidf = d.path().join("child.pid");
        // 손자 프로세스가 출력 줄을 붙잡고 오래 잔다 — 시간이 넘으면 묶음째 끝나야 한다
        let script = format!("sleep 300 & echo $! > {}; wait", pidf.display());
        let c = run_capped(
            Path::new("/bin/sh"),
            &["-c".to_string(), script],
            d.path(),
            &[],
            1,
        )
        .await;
        assert!(c.timed_out);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let pid = std::fs::read_to_string(&pidf)?.trim().to_string();
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid])
            .status()?
            .success();
        assert!(!alive, "손자 프로세스 {pid} 가 살아 있어요");
        Ok(())
    }
}
