//! PES 브리지(Rust) — 티켓 22: 계정 하나로 PES 에서 라운드 일을 받아 이 컴퓨터의 AI 로 돌리고,
//! 조사(조사 CLI · 원문 열기 · ego-browser)는 pes_local MCP 도구로 PC 에서 하고 원본을 PES 근거 기록으로 올린다.
//! 여러 계정 · 키 중계 · 채팅 · 다듬기 · 설치는 티켓 23, 업데이터는 티켓 24.
//!
//! 실행: pes-bridge [--account aN] [--once]   · pes-bridge --version · pes-bridge --check
mod account;
mod cli;
mod device;
mod engine;
mod events;
mod guard;
mod html;
mod live;
mod local;
mod log;
mod paths;
mod pes;
mod proc;
mod prompt;
mod round;
mod runner;
mod time;

use anyhow::{Context, Result};
use log::line;
use serde_json::json;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

fn arg(name: &str) -> Option<String> {
    let mut it = std::env::args().skip_while(|a| a != name);
    it.next()?;
    it.next()
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

async fn check() -> ExitCode {
    for e in [
        engine::Engine::Claude,
        engine::Engine::Codex,
        engine::Engine::OpenCode,
    ] {
        match e.bin() {
            None => println!("{}: 없음", e.ko()),
            Some(b) => {
                let s = engine::support(e, &b).await;
                println!(
                    "{}: {} {}{}",
                    e.ko(),
                    b.display(),
                    s.version,
                    if s.ok {
                        " — 라운드 받음".to_string()
                    } else {
                        format!(" — 받지 않음: {}", s.why)
                    }
                );
            }
        }
    }
    for t in [
        "kosis",
        "vworld",
        "sgis",
        "korean-law",
        "kordoc",
        "ego-browser",
    ] {
        println!(
            "{t}: {}",
            paths::which(t).map_or("없음".to_string(), |p| p.display().to_string())
        );
    }
    ExitCode::SUCCESS
}

async fn real_main() -> Result<ExitCode> {
    if flag("--version") {
        println!("{}", pes::VERSION);
        return Ok(ExitCode::SUCCESS);
    }
    if flag("--check") {
        return Ok(check().await);
    }
    let base = std::env::var("PES_BASE").unwrap_or_else(|_| "https://pes.seliq.kr".to_string());
    let paths = paths::Paths::new()?;
    let acc = account::pick(&paths, arg("--account").as_deref())?;
    line(&format!(
        "PES 브리지(Rust {}) — 계정 {} · {base}",
        pes::VERSION,
        acc.id
    ));
    let pes = Arc::new(pes::Pes::new(base, acc)?);
    // clone: Arc — pes_local 서버와 라운드가 같은 PES 연결을 함께 쓴다
    // clone: 경로 — pes_local 서버는 따로 도는 작업이라 자기 몫의 경로를 갖는다(Paths 는 라운드가 쓴다)
    let local = local::Local::start(
        Arc::clone(&pes),
        paths.docs.join(&pes.account.id),
        paths.home.clone(),
    )
    .await
    .context("pes_local 서버를 열지 못했어요")?;
    let machine = device::machine_id(&paths.conf).await;
    let engines: Vec<&str> = [
        engine::Engine::Claude,
        engine::Engine::Codex,
        engine::Engine::OpenCode,
    ]
    .iter()
    .filter(|e| e.bin().is_some())
    .map(|e| e.id())
    .collect();
    let info = json!({ "machine": machine, "hostname": std::env::var("HOSTNAME").unwrap_or_default(), "os_user": std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default(),
        "macs": [], "primary_mac": "", "platform": std::env::consts::OS, "os_version": "", "arch": std::env::consts::ARCH, "bridge_ver": pes::VERSION, "node_ver": "", "engines": engines, "account": pes.account.id });
    let b = round::Bridge {
        pes,
        local,
        paths,
        limits: runner::Limits::from_env(),
    };

    // 이 컴퓨터로 PES 를 써도 되나 — --once 도 이 확인을 거친다. 안 되면 10분마다 다시 묻는다(--once 는 끝낸다)
    let once = flag("--once");
    loop {
        match b.pes.computer(&info).await {
            Ok((true, _, _)) => break,
            Ok((false, why, _)) => line(&format!(
                "이 컴퓨터는 지금 PES 를 쓸 수 없어요 — {why} (10분마다 다시 확인해요)"
            )),
            Err(e) => line(&format!("컴퓨터 확인 오류 — {e:#} (10분 뒤 다시)")),
        }
        if once {
            return Ok(ExitCode::from(2));
        }
        tokio::time::sleep(Duration::from_secs(600)).await;
    }
    if once {
        // 알림 없이 한 번만 밀린 일을 보고 끝낸다(시험 · 손으로 돌릴 때)
        return Ok(match round::handle(&b, "한 번 확인").await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                line(&format!("오류 — {e:#}"));
                ExitCode::from(1)
            }
        });
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
    // clone: Arc — 알림 작업과 라운드 작업이 같은 PES 연결을 함께 쓴다
    let pes2 = Arc::clone(&b.pes);
    tokio::spawn(async move { live::run(&pes2, &machine, tx).await });
    // 라운드는 한 번에 하나 — 알림이 몰리면 2초 모아 한 번 확인한다. 실패해도 다음 알림을 받는다
    while let Some(why) = rx.recv().await {
        tokio::time::sleep(Duration::from_secs(2)).await;
        while rx.try_recv().is_ok() {}
        if let Err(e) = round::handle(&b, &why).await {
            line(&format!(
                "[{}] 라운드 일 확인 오류 — {e:#}",
                b.pes.account.id
            ));
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[tokio::main]
async fn main() -> ExitCode {
    match real_main().await {
        Ok(c) => c,
        Err(e) => {
            line(&format!("PES 브리지를 시작하지 못했어요 — {e:#}"));
            ExitCode::from(1)
        }
    }
}
