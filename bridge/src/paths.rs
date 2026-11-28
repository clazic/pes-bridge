//! 폴더 · 실행 파일 찾기. Node 브리지(worker/worker.mjs)와 같은 자리를 쓴다.
use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};

pub struct Paths {
    /// ~/.config/pes-bridge — 계정 · 설정 (PES_BRIDGE_CONF 로 바꿀 수 있다)
    pub conf: PathBuf,
    /// ~/.config/pes-bridge/accounts
    pub accounts: PathBuf,
    /// ~/.pes/docs — 받은 원문 파일과 변환한 Markdown
    pub docs: PathBuf,
    /// ~/.pes/work — AI 작업 폴더(계정마다 하나)
    pub work: PathBuf,
    pub home: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Self> {
        let home =
            home_dir().ok_or_else(|| anyhow!("홈 폴더를 찾지 못했어요(HOME 이 비어 있어요)"))?;
        let conf = std::env::var_os("PES_BRIDGE_CONF")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config").join("pes-bridge"));
        let pes = home.join(".pes");
        Ok(Self {
            accounts: conf.join("accounts"),
            docs: pes.join("docs"),
            work: pes.join("work"),
            conf,
            home,
        })
    }

    /// 조사 도구 사용법(스킬) 폴더 — 있는 것만. AI 는 읽기만 한다.
    pub fn skill_dirs(&self) -> Vec<PathBuf> {
        ["kosis", "vworld", "sgis"]
            .iter()
            .map(|n| self.home.join(".claude").join("skills").join(n))
            .filter_map(|d| std::fs::canonicalize(&d).ok())
            .filter(|d| d.join("SKILL.md").is_file())
            .collect()
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// PATH 다음에 볼 곳 — 설치 스크립트가 받은 것 · 사용자 설치 · Homebrew (launchd 처럼 PATH 가 짧을 때)
pub fn extra_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = home_dir() {
        let rt = h.join(".pes").join("runtime");
        v.push(rt.join("bin"));
        v.push(rt.join("node").join("bin"));
        v.push(h.join(".local").join("bin"));
        if let Some(a) = std::env::var_os("APPDATA").filter(|_| cfg!(windows)) {
            v.push(PathBuf::from(a).join("npm"));
        }
    }
    if !cfg!(windows) {
        v.push(PathBuf::from("/opt/homebrew/bin"));
        v.push(PathBuf::from("/usr/local/bin"));
    }
    v
}

/// 자식 프로세스에 줄 PATH — 지금 PATH 뒤에 extra_dirs 를 붙인다
pub fn child_path() -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for d in extra_dirs() {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// 실행 파일 찾기 — PATH 먼저, 그다음 extra_dirs
pub fn which(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    let p = child_path();
    for d in std::env::split_paths(&p) {
        for x in exts {
            let f = d.join(format!("{name}{x}"));
            if f.is_file() {
                return Some(f);
            }
        }
    }
    None
}

/// 남이 읽지 못하게 쓴다(유닉스 0600)
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// 프롬프트 · 설정에 넣는 경로 글(/ 로)
pub fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}
