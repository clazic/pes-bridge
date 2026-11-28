//! 라운드 한 바퀴 — pending_work 로 일을 보고, 맡을 AI 별로 묶어, 판을 확인한 뒤 막는 인자로 띄우고, 끝나면 알린다.
use crate::engine::{self, Engine, Spec};
use crate::local::{Local, Opened, Run};
use crate::log::{Event, line};
use crate::paths::{Paths, write_private};
use crate::pes::Pes;
use crate::runner::{self, Job, Limits, Outcome};
use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use std::sync::Arc;

/// 라운드 일 갈래(pending_work). chats · polish · rag 는 티켓 23 에서 옮긴다
pub const KINDS: [&str; 4] = ["start", "new", "open", "review"];
/// AI 프로세스에 넘기지 않는 이 컴퓨터의 조사 도구 키 — 조사 CLI 는 브리지가 돌린다
const LOCAL_KEY_ENV: [&str; 9] = [
    "LAW_OC",
    "KOREAN_LAW_API_KEY",
    "KOSIS_API_KEY",
    "KOSIS_API_KEYS",
    "SGIS_CONSUMER_KEY",
    "SGIS_CONSUMER_SECRET",
    "SGIS_API_KEYS",
    "VWORLD_API_KEYS",
    "VWORLD_DOMAIN",
];

pub struct Bridge {
    pub pes: Arc<Pes>,
    pub local: Arc<Local>,
    pub paths: Paths,
    pub limits: Limits,
}

pub struct Group {
    pub engine: Engine,
    pub model: String,
    pub effort: String,
    pub items: Vec<Value>,
}

fn ok_model(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 80
        && v.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:/@[]-".contains(c))
}
fn ok_effort(v: &str) -> bool {
    (2..=12).contains(&v.len()) && v.chars().all(|c| c.is_ascii_lowercase())
}

/// 할 일을 맡을 AI · 모델 · effort 별로 묶는다(Node 브리지 byEngine 과 같다)
pub fn group(out: &mut Value, def: Engine) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for k in KINDS {
        let Some(list) = out.get_mut(k).and_then(Value::as_array_mut) else {
            continue;
        };
        for mut x in list.drain(..) {
            let engine = x
                .get("engine")
                .and_then(Value::as_str)
                .and_then(Engine::parse)
                .unwrap_or(def);
            let model = x
                .get("model")
                .and_then(Value::as_str)
                .filter(|m| ok_model(m))
                .unwrap_or_default()
                .to_string();
            let effort = x
                .get("effort")
                .and_then(Value::as_str)
                .filter(|m| ok_effort(m))
                .unwrap_or_default()
                .to_string();
            if let Some(o) = x.as_object_mut() {
                o.insert("work".to_string(), json!(k));
            }
            match groups
                .iter_mut()
                .find(|g| g.engine == engine && g.model == model && g.effort == effort)
            {
                Some(g) => g.items.push(x),
                None => groups.push(Group {
                    engine,
                    model,
                    effort,
                    items: vec![x],
                }),
            }
        }
    }
    groups
}

fn slugs_of(items: &[Value]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for s in items
        .iter()
        .filter_map(|x| x.get("slug").and_then(Value::as_str))
    {
        if !v.iter().any(|y| y == s) {
            v.push(s.to_string());
        }
    }
    v
}

/// pending_work 의 라운드 일 지문 — 단계별 모델에서 진척이 있었는지 본다(Node 브리지 sig 와 같다)
fn sig(out: &Value) -> String {
    let mut v: Vec<String> = KINDS
        .iter()
        .flat_map(|k| {
            out.get(*k)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(move |x| {
                    let s = |f: &str| {
                        x.get(f)
                            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
                            .unwrap_or_default()
                    };
                    format!("{k}:{}:{}:{}", s("slug"), s("stage"), s("round"))
                })
        })
        .collect();
    v.sort();
    v.join("|")
}

/// 밀린 일을 보고 라운드를 돌린다. 단계별 모델(per_stage)이면 AI 가 단계 끝에서 멈추므로, 남은 일(다음 단계)이 바뀌었을 때만 그 모델로 이어 띄운다
pub async fn handle(b: &Bridge, why: &str) -> Result<()> {
    let mut why = why.to_string();
    for _ in 0..8 {
        let mut out = b.pes.tool("pending_work", json!({})).await?;
        let n: usize = KINDS
            .iter()
            .map(|k| out.get(*k).and_then(Value::as_array).map_or(0, Vec::len))
            .sum();
        if n == 0 {
            line(&format!("[{}] ({why}) 라운드 일 없음", b.pes.account.id));
            return Ok(());
        }
        let before = sig(&out);
        let per_stage = KINDS.iter().any(|k| {
            out.get(*k)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|x| x.get("per_stage").and_then(Value::as_bool) == Some(true))
        });
        let def = b
            .pes
            .me()
            .await
            .ok()
            .and_then(|m| {
                m.get("engine")
                    .and_then(Value::as_str)
                    .and_then(Engine::parse)
            })
            .unwrap_or(Engine::Claude);
        for g in group(&mut out, def) {
            if let Err(e) = run_group(b, &g, &why).await {
                line(&format!(
                    "[{}] {} 라운드 오류 — {e:#}",
                    b.pes.account.id,
                    g.engine.ko()
                ));
            }
        }
        if !per_stage {
            return Ok(());
        }
        let after = sig(&b.pes.tool("pending_work", json!({})).await?);
        if after.is_empty() {
            line(&format!(
                "[{}] 단계별 모델: 남은 라운드 일이 없어요",
                b.pes.account.id
            ));
            return Ok(());
        }
        if after == before {
            line(&format!(
                "[{}] 단계별 모델: 남은 일이 그대로예요(진척 없음) — 다시 띄우지 않아요",
                b.pes.account.id
            ));
            return Ok(());
        }
        line(&format!(
            "[{}] 단계별 모델: 진척이 있어 남은 일을 이어 띄워요 ({before} → {after})",
            b.pes.account.id
        ));
        why = "단계별 모델 · 다음 단계".to_string();
    }
    Ok(())
}

async fn log_event(b: &Bridge, ev: &Event<'_>) {
    let j = ev.to_json();
    if let Err(e) = b.pes.bridge_log(&j).await {
        line(&format!(
            "작업 로그를 PES 에 올리지 못해 이 컴퓨터에만 남겨요: {e:#} — {j}"
        ));
    }
}

async fn refuse(b: &Bridge, g: &Group, slugs: &[String], why: &str, version: &str) {
    line(&format!(
        "[{}] {} 라운드를 받지 않아요 — {why}",
        b.pes.account.id,
        g.engine.ko()
    ));
    b.pes.beat(&json!({ "slugs": slugs, "kind": "round", "phase": "error", "error": why.chars().take(200).collect::<String>() })).await;
    for s in slugs {
        log_event(
            b,
            &Event {
                action: "bridge_round_refused",
                project: Some(s),
                stage: None,
                target: Some(g.engine.id()),
                how: format!("{} {version} 판 확인", g.engine.ko()),
                why_kind: "rule",
                why: "막는 인자가 듣지 않는 옛 판 AI 에는 라운드를 주지 않는다(티켓 21 6절 5)"
                    .to_string(),
                result: "denied",
                detail: json!({ "reason": why, "version": version }),
            },
        )
        .await;
    }
}

fn read_json(p: &std::path::Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// 고르지 않았을 때의 모델 — Claude: ~/.claude/settings.json 의 model · Codex: ~/.codex/config.toml 의 model(--restricted · --ignore-user-config 라 직접 넘긴다)
fn default_model(engine: Engine, home: &std::path::Path) -> Option<String> {
    match engine {
        Engine::Claude => read_json(&home.join(".claude").join("settings.json"))
            .and_then(|v| v.get("model").and_then(Value::as_str).map(str::to_string)),
        Engine::Codex => std::fs::read_to_string(home.join(".codex").join("config.toml"))
            .ok()
            .and_then(|t| {
                t.lines().find_map(|l| {
                    l.trim()
                        .strip_prefix("model")
                        .map(str::trim)
                        .and_then(|r| r.strip_prefix('='))
                        .map(|r| r.trim().trim_matches('"').to_string())
                })
            }),
        Engine::OpenCode => None,
    }
    .filter(|m| ok_model(m))
}

async fn run_group(b: &Bridge, g: &Group, why: &str) -> Result<()> {
    let slugs = slugs_of(&g.items);
    let Some(bin) = g.engine.bin() else {
        refuse(
            b,
            g,
            &slugs,
            &format!(
                "이 컴퓨터에 {} 가 없어요 — 설치 스크립트를 다시 실행해 주세요",
                g.engine.ko()
            ),
            "",
        )
        .await;
        return Ok(());
    };
    let sup = engine::support(g.engine, &bin).await;
    if !sup.ok {
        refuse(b, g, &slugs, &sup.why, &sup.version).await;
        return Ok(());
    }
    // clone: 맡은 프로젝트 목록은 pes_local 실행(도구가 slug 를 검사)과 여기(신호 · 로그) 두 곳이 쓴다
    let opened = match b.local.open(slugs.clone(), g.engine.id()) {
        Ok(o) => o,
        Err(e) => {
            let why = format!("받은 원문 폴더를 만들지 못해 라운드를 띄우지 못했어요: {e:#}");
            finish(b, g, &slugs, &sup.version, None, failed_outcome(why)).await;
            return Ok(());
        }
    };
    let Opened {
        token,
        url,
        docs,
        id,
    } = opened;
    let guard = RunGuard {
        local: &b.local,
        token,
        done: false,
    };
    let l = Launch {
        bin: &bin,
        version: &sup.version,
        slugs: &slugs,
        url: &url,
        docs: &docs,
        token: &guard.token,
        run_id: &id,
    };
    let outcome = launch(b, g, why, &l)
        .await
        .unwrap_or_else(|e| failed_outcome(format!("라운드를 띄우지 못했어요: {e:#}")));
    let run = guard.finish();
    finish(b, g, &slugs, &sup.version, run, outcome).await;
    Ok(())
}

/// 실행 토큰 · 받은 원문 폴더를 닫는 가드 — 중간에 오류로 빠져나가도 닫힌다
struct RunGuard<'a> {
    local: &'a Local,
    token: String,
    done: bool,
}

impl RunGuard<'_> {
    fn finish(mut self) -> Option<Arc<Run>> {
        self.done = true;
        self.local.close(&self.token)
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.local.close(&self.token);
        }
    }
}

struct Launch<'a> {
    bin: &'a std::path::Path,
    version: &'a str,
    slugs: &'a [String],
    url: &'a str,
    docs: &'a std::path::Path,
    token: &'a str,
    run_id: &'a str,
}

fn failed_outcome(why: String) -> Outcome {
    Outcome {
        error: Some(why),
        last: String::new(),
        outside: Vec::new(),
        tool_uses: 0,
    }
}

async fn launch(b: &Bridge, g: &Group, why: &str, l: &Launch<'_>) -> Result<Outcome> {
    let acc = &b.pes.account.id;
    let work = b.paths.work.join(acc);
    std::fs::create_dir_all(&work).context("작업 폴더를 만들지 못했어요")?;
    let skills = b.paths.skill_dirs();
    let deny = engine::deny_read_paths(&b.paths.home, &b.paths.conf, &b.paths.docs);
    let key = b.pes.key().await?;
    let picked = (!g.model.is_empty()).then_some(g.model.as_str());
    let fallback = if picked.is_none() {
        default_model(g.engine, &b.paths.home)
    } else {
        None
    };
    let model = picked.or(fallback.as_deref());
    let effort = (!g.effort.is_empty()).then_some(g.effort.as_str());
    let windows = cfg!(windows);
    let spec = Spec {
        base: &b.pes.base,
        local_url: l.url,
        deny_read: &deny,
        work: &work,
        docs: l.docs,
        skills: &skills,
        home: &b.paths.home,
        model,
        effort,
        windows,
    };
    let stop = g
        .items
        .iter()
        .any(|x| x.get("per_stage").and_then(Value::as_bool) == Some(true))
        .then(|| {
            g.items
                .iter()
                .map(|x| {
                    format!(
                        "{} {}",
                        x.get("slug").and_then(Value::as_str).unwrap_or_default(),
                        x.get("stage_name")
                            .or_else(|| x.get("stage"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ")
        });
    let prompt = format!(
        "{}{}{}",
        crate::prompt::ROUND,
        crate::prompt::skill_rule(&skills),
        crate::prompt::only(l.slugs, stop.as_deref())
    );
    let mut env: Vec<(String, String)> = Vec::new();
    let mut variant: Option<String> = None;
    let args = match g.engine {
        Engine::Claude => {
            let cfg = b.pes.account.dir.join("mcp.json");
            write_private(
                &cfg,
                engine::claude_mcp(&spec, &key, l.token)
                    .to_string()
                    .as_bytes(),
            )?;
            engine::claude_args(&spec, &cfg)
        }
        Engine::Codex => {
            // 토큰은 명령줄이 아니라 환경 변수로(셸 환경에서는 뺀다 — shell_environment_policy.exclude)
            env.push(("PES_MCP_TOKEN".to_string(), key));
            env.push(("PES_LOCAL_TOKEN".to_string(), l.token.to_string()));
            engine::codex_args(&spec)
        }
        Engine::OpenCode => {
            // 내 설정 폴더는 싣지 않고, 내 설정에서 모델 · 제공자만 옮겨 온다(로그인 정보는 다른 폴더라 그대로 쓴다)
            let xdg_mine = std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| b.paths.home.join(".config"));
            let mine = match read_json(&xdg_mine.join("opencode").join("opencode.json")) {
                Some(Value::Object(m)) => m,
                _ => Map::new(),
            };
            let carry: Map<String, Value> = mine
                .into_iter()
                .filter(|(k, _)| {
                    [
                        "model",
                        "small_model",
                        "provider",
                        "enabled_providers",
                        "disabled_providers",
                    ]
                    .contains(&k.as_str())
                })
                .collect();
            // 모델 이름은 carry 를 설정에 넘기기(옮김) 전에 따로 적어 둔다
            let base_model = model.map(str::to_string).or_else(|| {
                carry
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            let cfg = b.pes.account.dir.join("opencode-round.json");
            write_private(
                &cfg,
                engine::opencode_config(&spec, carry).to_string().as_bytes(),
            )?;
            let xdg = b.paths.conf.join("opencode-home");
            std::fs::create_dir_all(&xdg)?;
            env.push(("PES_MCP_TOKEN".to_string(), key));
            env.push(("PES_LOCAL_TOKEN".to_string(), l.token.to_string()));
            env.extend(engine::opencode_env(&cfg, &xdg));
            variant = match (effort, base_model) {
                (Some(e), Some(m)) => Some(format!("{m}#{e}")),
                _ => None,
            };
            engine::opencode_args(variant.as_deref())
        }
    };
    let label = format!("[{acc}] {}", g.engine.ko());
    line(&format!(
        "{label} ({why}) 할 일 {}건 · {} {} · 모델 {} 시작",
        g.items.len(),
        g.engine.ko(),
        l.version,
        model.unwrap_or("기본")
    ));
    for s in l.slugs {
        log_event(b, &Event {
            action: "bridge_round_start", project: Some(s), stage: None, target: Some(g.engine.id()), how: format!("{} {} 를 PC 에서 띄움", g.engine.ko(), l.version),
            why_kind: "rule", why: format!("PES 알림({why}) 뒤 pending_work 에 일이 있음"), result: "ok",
            detail: json!({ "engine": g.engine.id(), "version": l.version, "model": model, "run": l.run_id,
                            "blocked": { "web_fetch": true, "shell_network": !windows, "shell_rule": "jq · grep · cat 만", "read_deny": deny.len(), "web_search": "켬(단서용)", "research": "pes_local 도구로만" } }),
        }).await;
    }
    let remove: Vec<&str> = LOCAL_KEY_ENV.to_vec();
    let job = Job {
        engine: g.engine,
        bin: l.bin,
        args: &args,
        env: &env,
        remove: &remove,
        prompt: &prompt,
        cwd: &work,
        label: &label,
    };
    // clone: Arc — 신호를 보내는 작업들이 같은 PES 연결을 함께 쓴다
    let mut outcome = runner::run(&job, Arc::clone(&b.pes), l.slugs, &b.limits).await;
    if g.engine == Engine::OpenCode
        && variant.is_some()
        && outcome
            .error
            .as_deref()
            .is_some_and(|e| e.to_ascii_lowercase().contains("variant unavailable"))
    {
        line(&format!(
            "{label} 이 모델은 effort {} 를 받지 않아요 — effort 없이 다시 돌려요",
            g.effort
        ));
        let args2 = engine::opencode_args(None);
        let job2 = Job {
            args: &args2,
            ..job
        };
        outcome = runner::run(&job2, Arc::clone(&b.pes), l.slugs, &b.limits).await;
    }
    Ok(outcome)
}

/// 라운드 끝 — 실패해도 늘 끝 신호와 작업 로그를 보낸다
async fn finish(
    b: &Bridge,
    g: &Group,
    slugs: &[String],
    version: &str,
    run: Option<Arc<Run>>,
    outcome: Outcome,
) {
    let label = format!("[{}] {}", b.pes.account.id, g.engine.ko());
    let (recorded, failed, calls) = run.as_ref().map_or((Vec::new(), 0, 0), |r| {
        let mut st = r
            .stats
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // 실행은 곧 버리므로 번호 목록을 꺼내 로그로 옮긴다
        (std::mem::take(&mut st.recorded), st.upload_failed, st.calls)
    });
    let phase = if outcome.error.is_some() {
        "error"
    } else {
        "done"
    };
    // outside: PES 밖 조사 흔적 수 — 라운드 결과에 「PES 밖 조사 있음」을 표시할 자리(서버 쪽 표시는 PES-API.md)
    b.pes.beat(&json!({ "slugs": slugs, "kind": "round", "phase": phase, "error": outcome.error, "outside": outcome.outside.len() })).await;
    line(&format!(
        "{label} 끝 — 도구 {}회 · pes_local {calls}회 · 근거 기록 {}건 · 올리지 못함 {failed}건 · PES 밖 조사 흔적 {}건{}",
        outcome.tool_uses,
        recorded.len(),
        outcome.outside.len(),
        outcome
            .error
            .as_deref()
            .map(|e| format!(" — {e}"))
            .unwrap_or_default()
    ));
    for s in slugs {
        log_event(b, &Event {
            action: "bridge_round_end", project: Some(s), stage: None, target: Some(g.engine.id()), how: format!("{} {version}", g.engine.ko()),
            why_kind: "rule", why: "라운드가 끝나면 결과 · 근거 기록 · PES 밖 조사 흔적을 남긴다".to_string(), result: if outcome.error.is_some() { "fail" } else { "ok" },
            detail: json!({ "error": outcome.error, "evidence": recorded, "evidence_failed": failed, "local_calls": calls, "tool_uses": outcome.tool_uses,
                            "outside_research": { "found": !outcome.outside.is_empty(), "items": outcome.outside } }),
        }).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_engine_and_model() {
        let mut out = json!({
            "new": [{ "slug": "a", "stage": "plan", "engine": "codex" }, { "slug": "b", "stage": "plan" }],
            "open": [{ "slug": "c", "stage": "research", "engine": "codex", "model": "gpt-x" }],
            "chats": [{ "slug": "z" }],
        });
        let g = group(&mut out, Engine::Claude);
        assert_eq!(g.len(), 3);
        assert_eq!(
            g.iter()
                .map(|x| (x.engine, x.items.len()))
                .collect::<Vec<_>>(),
            vec![(Engine::Codex, 1), (Engine::Claude, 1), (Engine::Codex, 1)]
        );
        assert_eq!(g[2].model, "gpt-x");
        assert_eq!(slugs_of(&g[0].items), vec!["a".to_string()]);
    }
}
