//! 프로세스 묶음 끝내기 — AI · 조사 CLI 를 새 프로세스 묶음(유닉스 process group)으로 띄우고, 끝낼 때 묶음째 끝낸다
//! (AI 가 띄운 셸 · 도구 프로세스가 남아 출력 줄을 붙잡지 않게). unsafe 없이: 유닉스는 nix 의 killpg, Windows 는 taskkill /T.

/// 띄우기 전에 부른다 — 유닉스: 새 프로세스 묶음(묶음 번호 = 그 프로세스 번호)
pub fn own_group(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        let _ = cmd;
    }
}

/// 묶음 전체를 끝낸다. 실패하면 까닭을 돌려준다(부른 쪽이 로그에 남긴다)
pub fn kill_tree(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, killpg};
        use nix::unistd::Pid;
        let p = i32::try_from(pid).map_err(|_| format!("프로세스 번호가 맞지 않아요: {pid}"))?;
        killpg(Pid::from_raw(p), Signal::SIGKILL)
            .map_err(|e| format!("프로세스 묶음 {pid} 을 끝내지 못했어요: {e}"))
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .map_err(|e| format!("taskkill 을 돌리지 못했어요: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "taskkill 이 실패했어요: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}
