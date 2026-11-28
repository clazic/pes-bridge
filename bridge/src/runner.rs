//! AI 하나를 띄워 끝날 때까지 본다 — 출력(JSON 줄)마다 지금 하는 일을 PES 에 알리고(beat), 멈추면 끝낸다,
//! 도구 사용에서 PES 밖 조사 흔적을 센다(감지 — 라운드를 실패로 하지 않고 결과에 표시만 한다, 티켓 21 6절 6).
use crate::engine::Engine;
use crate::events::{Ev, outside, parse};
use crate::log::line;
use crate::pes::Pes;
use serde_json::{Value, json};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct Limits {
    /// 출력 없이 이만큼 지나면 끝낸다
    pub idle: Duration,
    /// 한 번 실행이 이만큼 넘으면 끝낸다
    pub max: Duration,
}

impl Limits {
    pub fn from_env() -> Self {
        let mins = |k: &str, d: u64| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(d)
        };
        Self {
            idle: Duration::from_secs(mins("PES_BRIDGE_IDLE_MIN", 10) * 60),
            max: Duration::from_secs(mins("PES_BRIDGE_ROUND_MAX_MIN", 120) * 60),
        }
    }
}

pub struct Outcome {
    pub error: Option<String>,
    pub last: String,
    /// PES 밖 조사 흔적 [{tool, command, why}]
    pub outside: Vec<Value>,
    pub tool_uses: u32,
}

/// 동작 신호 보내기 — 바뀔 때는 2초에 한 번까지, 10초마다 이어 보낸다(출력이 3분 안에 있었을 때만)
struct Beater {
    pes: Arc<Pes>,
    slugs: Value,
    sent: Instant,
    phase: &'static str,
    tool: Option<String>,
}

impl Beater {
    fn send(&mut self, force: bool) {
        if !force && self.sent.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.sent = Instant::now();
        let b =
            json!({ "slugs": self.slugs, "kind": "round", "phase": self.phase, "tool": self.tool });
        // clone: Arc — 신호는 따로 도는 작업이 보내고(기다리지 않는다), 브리지는 계속 출력을 읽는다
        let pes = Arc::clone(&self.pes);
        tokio::spawn(async move { pes.beat(&b).await });
    }
    fn set(&mut self, phase: &'static str, tool: Option<&str>) {
        if phase == self.phase && tool == self.tool.as_deref() {
            return;
        }
        self.phase = phase;
        self.tool = tool.map(str::to_string);
        self.send(false);
    }
}

pub struct Job<'a> {
    pub engine: Engine,
    pub bin: &'a Path,
    pub args: &'a [String],
    pub env: &'a [(String, String)],
    /// AI 프로세스에서 뺄 환경 변수(이 컴퓨터의 조사 도구 키 — AI 는 조사 CLI 를 직접 부르지 않는다)
    pub remove: &'a [&'a str],
    pub prompt: &'a str,
    pub cwd: &'a Path,
    pub label: &'a str,
}

pub async fn run(job: &Job<'_>, pes: Arc<Pes>, slugs: &[String], limits: &Limits) -> Outcome {
    let mut out = Outcome {
        error: None,
        last: String::new(),
        outside: Vec::new(),
        tool_uses: 0,
    };
    let mut cmd = tokio::process::Command::new(job.bin);
    cmd.args(job.args)
        .current_dir(job.cwd)
        .env("PATH", crate::paths::child_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for k in job.remove {
        cmd.env_remove(k);
    }
    for (k, v) in job.env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            out.error = Some(format!("{} 를 띄우지 못했어요: {e}", job.engine.ko()));
            return out;
        }
    };
    // 프롬프트는 표준 입력으로 — 긴 여러 줄 글을 인자로 넘기면 Windows 에서 깨진다
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(job.prompt.as_bytes()).await {
            line(&format!("{} 프롬프트를 넘기지 못했어요: {e}", job.label));
        }
        drop(stdin);
    }
    let (Some(so), Some(se)) = (child.stdout.take(), child.stderr.take()) else {
        out.error = Some("AI 출력을 읽지 못했어요".to_string());
        return out;
    };
    let mut lines = BufReader::new(so).lines();
    let mut errs = BufReader::new(se).lines();
    let mut beat = Beater {
        pes,
        slugs: json!(slugs),
        sent: Instant::now(),
        phase: "start",
        tool: None,
    };
    beat.send(true);
    let began = Instant::now();
    let mut seen = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    let (mut out_done, mut err_done) = (false, false);
    let mut err_tail = String::new();
    let mut killed: Option<String> = None;
    let code: Option<i32> = loop {
        tokio::select! {
            l = lines.next_line(), if !out_done => match l {
                Ok(Some(l)) => {
                    seen = Instant::now();
                    let Ok(m) = serde_json::from_str::<Value>(l.trim_end_matches('\r')) else { continue };
                    for ev in parse(job.engine, &m) {
                        match ev {
                            Ev::Phase(p, t) => beat.set(p, t.as_deref()),
                            Ev::Fail(e) => out.error = Some(e.chars().take(200).collect()),
                            Ev::Done(t) => out.last = t,
                            Ev::Tool(name, command) => {
                                out.tool_uses += 1;
                                beat.set("tool", Some(name.trim_start_matches("mcp__pes__").trim_start_matches("mcp__pes_local__")));
                                line(&format!("  {} 도구: {name}{}", job.label, command.as_deref().map(|c| format!(" {}", c.chars().take(80).collect::<String>())).unwrap_or_default()));
                                if let Some(o) = outside(job.engine, &name, command.as_deref()) {
                                    line(&format!("  {} PES 밖 조사 흔적: {o}", job.label));
                                    out.outside.push(o);
                                }
                            }
                        }
                    }
                }
                _ => out_done = true,
            },
            l = errs.next_line(), if !err_done => match l {
                Ok(Some(l)) => {
                    seen = Instant::now();
                    eprintln!("{l}");
                    err_tail.push_str(&l);
                    err_tail.push('\n');
                    if err_tail.len() > 2000 {
                        err_tail = err_tail.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
                    }
                }
                _ => err_done = true,
            },
            _ = tick.tick() => {
                if killed.is_none() {
                    let why = if seen.elapsed() > limits.idle {
                        Some(format!("{}분 동안 응답이 없어 멈췄어요", limits.idle.as_secs() / 60))
                    } else if began.elapsed() > limits.max {
                        Some(format!("{}분이 넘어 멈췄어요", limits.max.as_secs() / 60))
                    } else { None };
                    if let Some(w) = why {
                        line(&format!("{} {w} — 끝내요", job.label));
                        let _ = child.start_kill();
                        killed = Some(w);
                    } else if seen.elapsed() < Duration::from_secs(180) {
                        beat.send(true);
                    }
                }
            },
            s = child.wait(), if out_done && err_done => {
                break s.ok().and_then(|x| x.code());
            }
        }
    };
    if let Some(k) = killed {
        out.error = Some(k);
    }
    if out.error.is_none() && code != Some(0) {
        let last = err_tail
            .trim()
            .lines()
            .last()
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect::<String>();
        out.error = Some(if last.is_empty() {
            format!(
                "{} 가 {} 로 끝났어요",
                job.engine.ko(),
                code.map_or("신호".to_string(), |c| c.to_string())
            )
        } else {
            last
        });
    }
    if out.error.is_none() && !out.last.is_empty() {
        line(&format!(
            "  {} 결과: {}",
            job.label,
            out.last
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(160)
                .collect::<String>()
        ));
    }
    out
}
