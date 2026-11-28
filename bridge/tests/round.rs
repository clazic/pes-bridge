//! 라운드 한 바퀴 시험 — 실제 브리지 실행 파일을 띄워 가짜 PES(로컬 HTTP · 웹소켓)에 붙이고, 가짜 AI(tests/fake/fake-ai.mjs)로 돌린다.
//! 확인: 일 받기(알림 → pending_work) · 막는 인자 · pes_local 도구 호출 · 원본 올리기 · 올리기 실패 시 실패 처리 · 옛 판 거부 · 감지 표시 · 작업 로그.
//! 실제 AI 는 부르지 않는다. 가짜 AI 는 Node 로 돈다(Codex · OpenCode 를 쓰는 PC 에는 Node 가 있다 — 명세).
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

type R<T> = Result<T, Box<dyn std::error::Error>>;
const KEY: &str = "pes_testkey0123456789abcdefgh";
const SLUG: &str = "t22-proj";

#[derive(Default)]
struct Fake {
    engine: String,
    evidence_fail: bool,
    device_pub: Option<String>,
    paths: Vec<String>,
    evidence: Vec<Value>,
    merges: Vec<Value>,
    submits: Vec<Value>,
    beats: Vec<Value>,
    logs: Vec<Value>,
    bad_sig: u32,
}

type St = Arc<Mutex<Fake>>;

fn lock(s: &St) -> MutexGuard<'_, Fake> {
    s.lock().unwrap_or_else(PoisonError::into_inner)
}

fn authed(h: &HeaderMap) -> bool {
    h.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {KEY}"))
}

fn jr(status: StatusCode, v: Value) -> Response {
    (
        status,
        [("content-type", "application/json")],
        v.to_string(),
    )
        .into_response()
}

fn ev_id(n: usize) -> String {
    const B32: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut s = String::from("ev_test");
    let mut x = n;
    for _ in 0..12 {
        s.push(char::from(B32.get(x % 32).copied().unwrap_or(b'a')));
        x /= 32;
    }
    s
}

fn verify(pubb64: &str, msg: &str, sig: &str) -> bool {
    let Ok(pk) = STANDARD.decode(pubb64) else {
        return false;
    };
    let Ok(vk) = VerifyingKey::from_sec1_bytes(&pk) else {
        return false;
    };
    let Ok(sb) = STANDARD.decode(sig) else {
        return false;
    };
    let Ok(s) = Signature::from_slice(&sb) else {
        return false;
    };
    vk.verify(msg.as_bytes(), &s).is_ok()
}

/// 기기 키로 서명한 몸통 {ts, nonce, r, sig} 을 확인하고 r 을 돌려준다
fn signed(st: &St, purpose: &str, body: &Value) -> Option<Value> {
    let r = body.get("r")?.as_str()?;
    let msg = format!(
        "{purpose}\n{}\n{}\n{r}",
        body.get("ts")?.as_i64()?,
        body.get("nonce")?.as_str()?
    );
    let mut f = lock(st);
    if !verify(f.device_pub.as_deref()?, &msg, body.get("sig")?.as_str()?) {
        f.bad_sig += 1;
        return None;
    }
    serde_json::from_str(r).ok()
}

async fn mcp(State(st): State<St>, h: HeaderMap, body: Bytes) -> Response {
    if !authed(&h) {
        return jr(StatusCode::UNAUTHORIZED, json!({ "error": "unauthorized" }));
    }
    let mut m: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let id = m.get_mut("id").map(Value::take).unwrap_or(Value::Null);
    let method = m.get("method").and_then(Value::as_str).unwrap_or_default();
    let ok = |v: Value| {
        jr(
            StatusCode::OK,
            json!({ "jsonrpc": "2.0", "id": id, "result": { "content": [{ "type": "text", "text": v.to_string() }], "structuredContent": v } }),
        )
    };
    if method == "initialize" {
        return jr(
            StatusCode::OK,
            json!({ "jsonrpc": "2.0", "id": id, "result": { "protocolVersion": "2025-06-18", "capabilities": {}, "serverInfo": { "name": "fake-pes" } } }),
        );
    }
    let args = m
        .pointer_mut("/params/arguments")
        .map(Value::take)
        .unwrap_or(Value::Null);
    let name = m
        .pointer("/params/name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut f = lock(&st);
    f.paths.push(format!("mcp:{name}"));
    match name {
        "pending_work" => {
            let new = if f.submits.is_empty() {
                json!([{ "slug": SLUG, "topic": "시험 주제", "engine": f.engine, "stage": "research", "stage_name": "조사", "kind": "stage_start", "next": "start_round" }])
            } else {
                json!([])
            };
            ok(
                json!({ "chats": [], "polish": [], "start": [], "review": [], "open": [], "new": new }),
            )
        }
        "start_round" => ok(
            json!({ "round": 1, "stage": "research", "instructions": "시험 지시", "context": "", "items": [] }),
        ),
        "merge_items" => {
            f.merges.push(args);
            ok(json!({ "ok": [1] }))
        }
        "submit_round" => {
            f.submits.push(args);
            ok(json!({ "ok": true, "round": 1, "questions": 0 }))
        }
        _ => jr(
            StatusCode::OK,
            json!({ "jsonrpc": "2.0", "id": id, "result": { "isError": true, "content": [{ "type": "text", "text": "없는 도구" }] } }),
        ),
    }
}

async fn plain(State(st): State<St>, uri: axum::http::Uri, h: HeaderMap, body: Bytes) -> Response {
    let path = uri.path();
    lock(&st).paths.push(path.to_string());
    if path == "/page.html" {
        // 길이가 한 쪽(PAGE)보다 길어 read_more 로 이어 읽어야 하는 원문
        let body = format!(
            "<html><head><title>시험 원문</title></head><body><h1>인구 현황</h1><p>{}</p><a href=\"/att.pdf\">첨부</a><p>끝 문장 표시</p></body></html>",
            "통계 ".repeat(15_000)
        );
        return (
            StatusCode::OK,
            [("content-type", "text/html; charset=utf-8")],
            body,
        )
            .into_response();
    }
    if path == "/att.pdf" {
        return (
            StatusCode::OK,
            [("content-type", "application/pdf")],
            b"%PDF-1.4 test".to_vec(),
        )
            .into_response();
    }
    if !authed(&h) {
        return jr(StatusCode::UNAUTHORIZED, json!({ "error": "unauthorized" }));
    }
    let b: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    match path {
        "/mcp/me" => jr(
            StatusCode::OK,
            json!({ "id": 7, "email": "t22@example.com", "engine": lock(&st).engine }),
        ),
        "/mcp/computer" => jr(
            StatusCode::OK,
            json!({ "allowed": true, "machine": b.get("machine") }),
        ),
        "/mcp/beat" => {
            lock(&st).beats.push(b);
            jr(StatusCode::OK, json!({ "ok": true }))
        }
        "/mcp/device" => {
            let (Some(p), Some(ts), Some(sig)) = (
                b.get("pub").and_then(Value::as_str),
                b.get("ts").and_then(Value::as_i64),
                b.get("sig").and_then(Value::as_str),
            ) else {
                return jr(StatusCode::BAD_REQUEST, json!({ "error": "pub" }));
            };
            if !verify(p, &format!("pes-device\n{ts}\n{p}"), sig) {
                return jr(
                    StatusCode::BAD_REQUEST,
                    json!({ "error": "서명이 맞지 않아요" }),
                );
            }
            lock(&st).device_pub = Some(p.to_string());
            jr(StatusCode::OK, json!({ "ok": true }))
        }
        "/mcp/evidence" => {
            if lock(&st).evidence_fail {
                return jr(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({ "error": "근거 저장소 오류(시험)" }),
                );
            }
            let Some(rec) = signed(&st, "pes-evidence", &b) else {
                return jr(
                    StatusCode::BAD_REQUEST,
                    json!({ "error": "서명이 맞지 않아요" }),
                );
            };
            if rec.get("slug").and_then(Value::as_str) != Some(SLUG) {
                return jr(
                    StatusCode::FORBIDDEN,
                    json!({ "error": "권한 없는 프로젝트" }),
                );
            }
            let mut f = lock(&st);
            let id = ev_id(f.evidence.len() + 1);
            f.evidence.push(json!({ "id": id, "rec": rec }));
            jr(StatusCode::OK, json!({ "evidence_id": id }))
        }
        "/mcp/bridge-log" => {
            let Some(mut r) = signed(&st, "pes-bridge-log", &b) else {
                return jr(
                    StatusCode::BAD_REQUEST,
                    json!({ "error": "서명이 맞지 않아요" }),
                );
            };
            if let Some(Value::Array(evs)) = r.get_mut("events").map(Value::take) {
                lock(&st).logs.extend(evs);
            }
            jr(StatusCode::OK, json!({ "ok": true }))
        }
        _ => jr(StatusCode::NOT_FOUND, json!({ "error": "없는 주소" })),
    }
}

async fn live(State(st): State<St>, h: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if !authed(&h) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    lock(&st).paths.push("/mcp/live".to_string());
    ws.on_upgrade(|mut s: WebSocket| async move {
        let _ = s
            .send(Message::Text(
                json!({ "kind": "work", "stage": format!("{SLUG} research") })
                    .to_string()
                    .into(),
            ))
            .await;
        while let Some(Ok(m)) = s.recv().await {
            if let Message::Text(t) = m
                && t.as_str() == "ping"
            {
                let _ = s.send(Message::Text("pong".into())).await;
            }
        }
    })
}

struct Env {
    st: St,
    base: String,
    home: tempfile::TempDir,
    log: PathBuf,
}

async fn fake_pes(engine: &str, evidence_fail: bool) -> R<Env> {
    let st: St = Arc::new(Mutex::new(Fake {
        engine: engine.to_string(),
        evidence_fail,
        ..Fake::default()
    }));
    let app = Router::new()
        .route("/mcp", post(mcp))
        .route("/mcp/live", get(live))
        .fallback(any(plain))
        .with_state(Arc::clone(&st));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", l.local_addr()?);
    tokio::spawn(async move {
        let _ = axum::serve(l, app).await;
    });
    let home = tempfile::tempdir()?;
    let acc = home.path().join(".config/pes-bridge/accounts/a1");
    std::fs::create_dir_all(&acc)?;
    std::fs::write(acc.join("key"), KEY)?;
    let log = home.path().join("fake-ai.jsonl");
    Ok(Env {
        st,
        base,
        home,
        log,
    })
}

fn fake_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fake")
}

#[cfg(unix)]
fn exec_bits(p: &Path) -> R<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn spawn_bridge(e: &Env, engine: &str, old: bool) -> R<tokio::process::Child> {
    let fake = fake_dir();
    let ai = fake.join("fake-ai.mjs");
    let bin = fake.join("bin");
    #[cfg(unix)]
    {
        exec_bits(&ai)?;
        for b in ["kosis", "kordoc", "ego-browser"] {
            exec_bits(&bin.join(b))?;
        }
    }
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))?;
    let var = match engine {
        "codex" => "CODEX_BIN",
        "opencode" => "OPENCODE_BIN",
        _ => "CLAUDE_BIN",
    };
    let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_pes-bridge"))
        .env("HOME", e.home.path())
        .env_remove("PES_BRIDGE_CONF")
        .env_remove("XDG_CONFIG_HOME")
        .env("PES_BASE", &e.base)
        .env("PATH", path)
        .env(var, &ai)
        .env("FAKE_ENGINE", engine)
        .env("FAKE_AI_OLD", if old { "1" } else { "0" })
        .env("FAKE_AI_LOG", &e.log)
        .env("FAKE_PAGE_URL", format!("{}/page.html", e.base))
        .env("PES_BRIDGE_ALLOW_LOOPBACK_FETCH", "1")
        .env("KOSIS_API_KEY", "local-secret-should-not-reach-ai")
        .kill_on_drop(true)
        .spawn()?;
    Ok(child)
}

async fn wait_for(st: &St, secs: u64, done: impl Fn(&Fake) -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if done(&lock(st)) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

fn ai_log(p: &Path) -> Vec<Value> {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn step<'a>(log: &'a [Value], name: &str) -> Option<&'a Value> {
    log.iter()
        .find(|x| x.get("step").and_then(Value::as_str) == Some(name))
}

fn has_log(f: &Fake, action: &str) -> bool {
    f.logs
        .iter()
        .any(|l| l.get("action").and_then(Value::as_str) == Some(action))
}

/// 일을 받아 라운드 한 바퀴 — 엔진마다
async fn full_round(engine: &str) -> R<()> {
    let e = fake_pes(engine, false).await?;
    let mut child = spawn_bridge(&e, engine, false)?;
    let ok = wait_for(&e.st, 60, |f| {
        !f.submits.is_empty() && has_log(f, "bridge_round_end")
    })
    .await;
    let _ = child.kill().await;
    let log = ai_log(&e.log);
    if let Some(err) = step(&log, "error") {
        return Err(format!("가짜 AI 오류: {err}").into());
    }
    assert!(ok, "라운드가 60초 안에 끝나지 않았어요");
    let f = lock(&e.st);
    let run = step(&log, "run").ok_or("가짜 AI 가 돌지 않았어요")?;
    let res = step(&log, "results").ok_or("가짜 AI 결과가 없어요")?;
    let argv: Vec<&str> = run
        .get("argv")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let joined = argv.join(" ");

    // 막는 인자(티켓 21 확정)
    match engine {
        "claude-code" => {
            assert!(argv.contains(&"--restricted") && argv.contains(&"--strict-mcp-config"));
            assert!(!joined.contains("WebFetch"), "내장 웹 가져오기는 꺼야 해요");
            assert!(
                joined.contains("\"allowedDomains\":[]"),
                "셸 네트워크를 막는 샌드박스"
            );
            assert!(
                !joined.contains("Bash(kosis"),
                "조사 CLI 는 셸로 주지 않아요"
            );
        }
        "codex" => {
            assert!(joined.contains("network={enabled=false}"));
            assert!(joined.contains("web_search=\"cached\""));
            assert!(joined.contains("--disable browser_use"));
        }
        _ => {
            assert_eq!(
                run.pointer("/env/OPENCODE_DISABLE_PROJECT_CONFIG"),
                Some(&json!("1"))
            );
            assert_eq!(
                run.pointer("/opencode_config/permission/webfetch"),
                Some(&json!("deny"))
            );
            assert_eq!(
                run.pointer("/opencode_config/permission/bash/*"),
                Some(&json!("deny"))
            );
        }
    }
    assert_eq!(
        run.pointer("/env/KOSIS_API_KEY"),
        Some(&Value::Null),
        "이 컴퓨터의 조사 도구 키는 AI 에 넘기지 않아요"
    );
    assert_eq!(run.get("prompt_has_pes_local"), Some(&json!(true)));

    // pes_local 도구 호출 → 원본이 근거 기록으로 올라감
    let tools = res.get("tools").and_then(Value::as_array).ok_or("tools")?;
    for t in ["kosis", "fetch_url", "read_more"] {
        assert!(tools.contains(&json!(t)), "pes_local 에 {t} 가 있어야 해요");
    }
    assert_eq!(res.pointer("/kosis/isError"), Some(&json!(false)));
    let kid = res
        .pointer("/kosis/data/evidence_id")
        .and_then(Value::as_str)
        .ok_or("kosis evidence_id")?;
    let krec = f
        .evidence
        .iter()
        .find(|x| x.get("id").and_then(Value::as_str) == Some(kid))
        .ok_or("kosis 근거 기록이 PES 에 없어요")?;
    assert_eq!(krec.pointer("/rec/tool"), Some(&json!("pc_kosis")));
    assert_eq!(krec.pointer("/rec/via"), Some(&json!("pc")));
    assert_eq!(krec.pointer("/rec/status"), Some(&json!("ok")));
    assert_eq!(krec.pointer("/rec/engine"), Some(&json!(engine)));
    assert_eq!(
        krec.pointer("/rec/args/args"),
        Some(&json!(["data", "--tbl", "DT_TEST"])),
        "요청(인자)이 남아요"
    );
    let kbody = krec
        .pointer("/rec/parts/0/body")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        kbody.contains("51751065") && kbody.contains("data --tbl DT_TEST"),
        "응답 원본이 그대로 남아요"
    );

    // 원문 열기: 한 쪽보다 길어 read_more 로 끝까지 읽음, 원본은 HTML 그대로
    assert!(
        res.get("fetch_pages").and_then(Value::as_u64).unwrap_or(0) >= 2,
        "긴 원문은 이어 읽어요"
    );
    assert!(
        res.get("fetch_tail")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("끝 문장 표시"),
        "끝까지 받아요"
    );
    let frec = f
        .evidence
        .iter()
        .find(|x| x.pointer("/rec/tool") == Some(&json!("pc_fetch")))
        .ok_or("원문 근거 기록이 없어요")?;
    assert!(
        frec.pointer("/rec/parts/0/body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("<h1>인구 현황</h1>")
    );
    assert_eq!(frec.pointer("/rec/title"), Some(&json!("시험 원문")));

    // 문서 파일: 바이트 그대로 올리고 받은 원문 폴더에 둔 뒤 kordoc 으로 읽음(변환 글도 근거 기록, 원본 근거를 가리킴)
    let pid = res
        .pointer("/pdf/data/evidence_id")
        .and_then(Value::as_str)
        .ok_or("pdf evidence_id")?;
    let prec = f
        .evidence
        .iter()
        .find(|x| x.get("id").and_then(Value::as_str) == Some(pid))
        .ok_or("pdf 근거 기록")?;
    assert_eq!(
        prec.pointer("/rec/binary/mime"),
        Some(&json!("application/pdf"))
    );
    assert_eq!(
        prec.pointer("/rec/binary/b64"),
        Some(&json!(STANDARD.encode(b"%PDF-1.4 test")))
    );
    assert_eq!(
        res.pointer("/doc/isError"),
        Some(&json!(false)),
        "{:?}",
        res.get("doc")
    );
    assert!(
        res.pointer("/doc/data/text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("13 바이트")
    );
    let did = res
        .pointer("/doc/data/evidence_id")
        .and_then(Value::as_str)
        .ok_or("kordoc evidence_id")?;
    let drec = f
        .evidence
        .iter()
        .find(|x| x.get("id").and_then(Value::as_str) == Some(did))
        .ok_or("kordoc 근거 기록")?;
    assert_eq!(drec.pointer("/rec/args/from_evidence"), Some(&json!(pid)));
    // ego-browser: 브리지가 만든 스크립트로 열고, 연 페이지 HTML 을 올림
    assert_eq!(
        res.pointer("/ego/isError"),
        Some(&json!(false)),
        "{:?}",
        res.get("ego")
    );
    let erec = f
        .evidence
        .iter()
        .find(|x| x.pointer("/rec/tool") == Some(&json!("pc_ego_read")))
        .ok_or("ego 근거 기록")?;
    assert!(
        erec.pointer("/rec/parts/0/body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("표 화면 값 42")
    );
    assert!(
        res.pointer("/ego/data/text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("[링크]")
    );

    // 맡지 않은 프로젝트는 거부
    assert_eq!(res.pointer("/foreign/isError"), Some(&json!(true)));

    // AI 가 근거 번호를 달아 넣음
    let details = f
        .merges
        .first()
        .and_then(|m| m.pointer("/items/0/details"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        details.contains(&format!("[^{kid}]")),
        "문장이 근거 번호를 가리켜요: {details}"
    );

    // 감지: 허용 밖 셸(curl)은 흔적으로, 허용한 jq 는 아님
    let end = f
        .logs
        .iter()
        .find(|l| l.get("action").and_then(Value::as_str) == Some("bridge_round_end"))
        .ok_or("round_end 로그")?;
    assert_eq!(
        end.pointer("/detail/outside_research/found"),
        Some(&json!(true))
    );
    let items = end
        .pointer("/detail/outside_research/items")
        .and_then(Value::as_array)
        .ok_or("outside items")?;
    assert_eq!(items.len(), 1, "curl 하나만 흔적: {items:?}");
    assert_eq!(end.pointer("/detail/evidence_failed"), Some(&json!(0)));
    assert!(has_log(&f, "bridge_round_start"));
    assert!(
        f.beats
            .iter()
            .any(|b| b.get("phase") == Some(&json!("done")) && b.get("outside") == Some(&json!(1)))
    );
    assert_eq!(f.bad_sig, 0);

    // 크레딧: 브리지는 크레딧 · 결제 길을 부르지 않는다(PC AI 작업은 차감하지 않음 — 서버 externalStart 도 차감하지 않는다)
    let allowed = [
        "/mcp/me",
        "/mcp/computer",
        "/mcp/live",
        "/mcp/beat",
        "/mcp/device",
        "/mcp/evidence",
        "/mcp/bridge-log",
        "/page.html",
        "/att.pdf",
    ];
    for p in &f.paths {
        assert!(
            p.starts_with("mcp:") || allowed.contains(&p.as_str()),
            "예상 밖 요청: {p}"
        );
    }
    assert!(
        !f.paths
            .iter()
            .any(|p| p.contains("credit") || p.contains("billing"))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn round_claude_code() -> R<()> {
    full_round("claude-code").await
}

#[tokio::test(flavor = "multi_thread")]
async fn round_codex() -> R<()> {
    full_round("codex").await
}

#[tokio::test(flavor = "multi_thread")]
async fn round_opencode() -> R<()> {
    full_round("opencode").await
}

/// 근거 기록을 못 올리면 그 도구 호출은 실패 — AI 는 결과를 받지 못한다
#[tokio::test(flavor = "multi_thread")]
async fn evidence_upload_failure_fails_the_tool() -> R<()> {
    let e = fake_pes("claude-code", true).await?;
    let mut child = spawn_bridge(&e, "claude-code", false)?;
    let ok = wait_for(&e.st, 60, |f| {
        !f.submits.is_empty() && has_log(f, "bridge_round_end")
    })
    .await;
    let _ = child.kill().await;
    assert!(ok, "라운드가 끝나지 않았어요");
    let log = ai_log(&e.log);
    let res = step(&log, "results").ok_or("가짜 AI 결과가 없어요")?;
    assert_eq!(res.pointer("/kosis/isError"), Some(&json!(true)));
    let text = res
        .pointer("/kosis/text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(text.contains("근거 기록을 PES 에 올리지 못해"), "{text}");
    assert!(
        !text.contains("51751065"),
        "실패한 조회의 결과는 AI 에 주지 않아요"
    );
    assert!(
        res.get("fetch_error")
            .and_then(Value::as_str)
            .is_some_and(|t| t.contains("근거 기록을 PES 에 올리지 못해"))
    );
    let f = lock(&e.st);
    assert!(f.evidence.is_empty());
    assert!(
        f.logs
            .iter()
            .filter(|l| l.get("action") == Some(&json!("bridge_evidence_fail")))
            .count()
            >= 2
    );
    let end = f
        .logs
        .iter()
        .find(|l| l.get("action") == Some(&json!("bridge_round_end")))
        .ok_or("round_end")?;
    assert!(
        end.pointer("/detail/evidence_failed")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 2
    );
    let details = f
        .merges
        .first()
        .and_then(|m| m.pointer("/items/0/details"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(details.contains("[^unverified]") && !details.contains("[^ev_"));
    Ok(())
}

/// 막는 인자가 듣지 않는 옛 판 AI 는 라운드를 받지 않고 까닭을 알린다
#[tokio::test(flavor = "multi_thread")]
async fn old_ai_is_refused() -> R<()> {
    for engine in ["claude-code", "codex", "opencode"] {
        let e = fake_pes(engine, false).await?;
        let mut child = spawn_bridge(&e, engine, true)?;
        let ok = wait_for(&e.st, 30, |f| has_log(f, "bridge_round_refused")).await;
        let _ = child.kill().await;
        assert!(ok, "{engine}: 거부 로그가 없어요");
        let f = lock(&e.st);
        assert!(
            f.submits.is_empty() && f.evidence.is_empty(),
            "{engine}: 라운드를 돌리면 안 돼요"
        );
        assert!(
            step(&ai_log(&e.log), "run").is_none(),
            "{engine}: AI 를 띄우면 안 돼요"
        );
        let beat = f
            .beats
            .iter()
            .find(|b| b.get("phase") == Some(&json!("error")))
            .ok_or("error beat")?;
        assert!(
            beat.get("error")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("업데이트"),
            "{engine}: {beat}"
        );
        let r = f
            .logs
            .iter()
            .find(|l| l.get("action") == Some(&json!("bridge_round_refused")))
            .ok_or("refused")?;
        assert_eq!(r.get("result"), Some(&json!("denied")));
    }
    Ok(())
}
