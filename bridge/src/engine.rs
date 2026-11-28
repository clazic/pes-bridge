//! 이 컴퓨터의 AI 세 가지 — 판 확인과 라운드 인자(티켓 21 확정).
//! - 셸은 네트워크 없이 보조 명령(jq · grep · cat)만. 조사 CLI · 원문 열기 · ego-browser 는 pes_local MCP 도구로만.
//! - 내장 웹 가져오기는 끈다. 내장 웹 검색은 켠다(결과 목록은 단서일 뿐, 근거가 아니다).
//! - Claude Code: OS 샌드박스(allowedDomains: []) · Codex: 권한 프로필 network.enabled=false · OpenCode: bash 허용 목록(글 규칙)
//!   + OPENCODE_DISABLE_PROJECT_CONFIG=1. Windows 는 OS 막기가 약해 글 규칙 + 감지 표시로 받아들인다(티켓 21 6절 4).
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Claude,
    Codex,
    OpenCode,
}

/// 셸에서 허용하는 보조 명령(이 컴퓨터 안의 글만 다룬다)
pub const SHELL_OK: [&str; 3] = ["jq", "grep", "cat"];

/// 막는 인자를 티켓 21 에서 실제로 시험한 판 — 이보다 낮으면 라운드를 받지 않는다
pub const MIN_CLAUDE: (u64, u64, u64) = (2, 1, 332);
pub const MIN_CODEX: (u64, u64, u64) = (0, 172, 0);
pub const MIN_OPENCODE: (u64, u64, u64) = (2, 4, 2);

impl Engine {
    pub fn parse(v: &str) -> Option<Self> {
        match v {
            "claude-code" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
    pub fn ko(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
        }
    }
    fn bin_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
    fn env_name(self) -> &'static str {
        match self {
            Self::Claude => "CLAUDE_BIN",
            Self::Codex => "CODEX_BIN",
            Self::OpenCode => "OPENCODE_BIN",
        }
    }
    fn min(self) -> (u64, u64, u64) {
        match self {
            Self::Claude => MIN_CLAUDE,
            Self::Codex => MIN_CODEX,
            Self::OpenCode => MIN_OPENCODE,
        }
    }
    /// 실행 파일 — 환경 변수(CLAUDE_BIN …)가 먼저, 없으면 PATH
    pub fn bin(self) -> Option<PathBuf> {
        std::env::var_os(self.env_name())
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| crate::paths::which(self.bin_name()))
    }
}

/// 글에서 처음 나오는 x.y.z
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find_map(|w| {
            let mut it = w.split('.').filter(|x| !x.is_empty());
            let a = it.next()?.parse().ok()?;
            let b = it.next()?.parse().ok()?;
            let c = it.next()?.parse().ok()?;
            Some((a, b, c))
        })
}

pub struct Support {
    pub ok: bool,
    pub version: String,
    /// 받지 않는 까닭(사람이 읽는 말)
    pub why: String,
}

async fn out_of(bin: &Path, args: &[&str]) -> String {
    let fut = tokio::process::Command::new(bin)
        .args(args)
        .env("PATH", crate::paths::child_path())
        .stdin(std::process::Stdio::null())
        .output();
    match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
        Ok(Ok(o)) => format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
        _ => String::new(),
    }
}

/// 이 AI 판에서 막는 인자가 듣는지 — 판 번호(시험한 판 이상)와 도움말의 인자 이름으로 본다
pub async fn support(engine: Engine, bin: &Path) -> Support {
    let vtext = out_of(bin, &["--version"]).await;
    let Some(v) = parse_version(&vtext) else {
        return Support {
            ok: false,
            version: String::new(),
            why: format!("{} 판을 읽지 못했어요(--version)", engine.ko()),
        };
    };
    let version = format!("{}.{}.{}", v.0, v.1, v.2);
    let (ma, mb, mc) = engine.min();
    if v < engine.min() {
        return Support {
            ok: false,
            version,
            why: format!(
                "{} {}.{}.{} 이상이어야 PES 밖 조사를 막는 인자가 들어요(지금 {}.{}.{}). AI 를 업데이트해 주세요",
                engine.ko(),
                ma,
                mb,
                mc,
                v.0,
                v.1,
                v.2
            ),
        };
    }
    let (help, need): (String, &[&str]) = match engine {
        Engine::Claude => (
            out_of(bin, &["--help"]).await,
            &[
                "--restricted",
                "--tools",
                "--settings",
                "--strict-mcp-config",
                "--permission-prompts",
            ],
        ),
        Engine::Codex if cfg!(windows) => (
            out_of(bin, &["exec", "--help"]).await,
            &["--ignore-user-config", "--json"],
        ),
        Engine::Codex => (
            format!(
                "{}{}",
                out_of(bin, &["exec", "--help"]).await,
                out_of(bin, &["sandbox", "--help"]).await
            ),
            &["--ignore-user-config", "--json", "--permission-profile"],
        ),
        Engine::OpenCode => (
            out_of(bin, &["run", "--help"]).await,
            &["--standalone", "--format"],
        ),
    };
    let missing: Vec<&str> = need.iter().copied().filter(|f| !help.contains(f)).collect();
    if !missing.is_empty() {
        let why = format!(
            "{} {version} 에 막는 인자({})가 없어요. AI 를 업데이트해 주세요",
            engine.ko(),
            missing.join(" · ")
        );
        return Support {
            ok: false,
            version,
            why,
        };
    }
    Support {
        ok: true,
        version,
        why: String::new(),
    }
}

/// 라운드 하나를 띄우는 데 필요한 것
pub struct Spec<'a> {
    pub base: &'a str,
    /// pes_local MCP 주소(이 실행만의 토큰이 들어 있다)
    pub local_url: &'a str,
    pub work: &'a Path,
    pub docs: &'a Path,
    pub skills: &'a [PathBuf],
    pub home: &'a Path,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub windows: bool,
}

/// Claude Code 의 MCP 설정(계정 폴더 mcp.json, 0600) — PES 는 머리글에 키, pes_local 은 주소에 실행 토큰
pub fn claude_mcp(s: &Spec, key: &str) -> Value {
    json!({ "mcpServers": {
        "pes": { "type": "http", "url": format!("{}/mcp", s.base), "headers": { "Authorization": format!("Bearer {key}") } },
        "pes_local": { "type": "http", "url": s.local_url },
    } })
}

/// Claude Code 샌드박스 — 셸 명령의 네트워크를 모두 막는다(티켓 21 C2: 바깥 · 127.0.0.1 모두 못 닿음)
pub fn claude_sandbox() -> Value {
    json!({ "sandbox": { "enabled": true, "allowUnsandboxedCommands": false, "failIfUnavailable": true, "network": { "allowedDomains": [] } } })
}

pub fn claude_args(s: &Spec, mcp_cfg: &Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--restricted",
        "--tools",
        "WebSearch",
        "Read",
        "Bash",
        "--add-dir",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    a.push(crate::paths::slash(s.docs));
    a.extend(s.skills.iter().map(|d| crate::paths::slash(d)));
    a.extend([
        "--mcp-config".to_string(),
        crate::paths::slash(mcp_cfg),
        "--strict-mcp-config".to_string(),
        "--allowedTools".to_string(),
    ]);
    a.extend(
        ["mcp__pes", "mcp__pes_local", "WebSearch", "Read"]
            .iter()
            .map(|x| x.to_string()),
    );
    a.extend(SHELL_OK.iter().map(|c| format!("Bash({c}:*)")));
    a.extend([
        "--permission-prompts".to_string(),
        "none".to_string(),
        "--no-session-persistence".to_string(),
    ]);
    if !s.windows {
        a.push("--settings".to_string());
        a.push(claude_sandbox().to_string());
    }
    if let Some(m) = s.model {
        a.extend(["--model".to_string(), m.to_string()]);
    }
    if let Some(e) = s.effort {
        a.extend(["--effort".to_string(), e.to_string()]);
    }
    a.extend(
        [
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
        ]
        .iter()
        .map(|x| x.to_string()),
    );
    a
}

fn toml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

pub fn codex_args(s: &Spec) -> Vec<String> {
    let mut a: Vec<String> = [
        "exec",
        "--json",
        "--skip-git-repo-check",
        "--ignore-user-config",
        "--ignore-rules",
        "--disable",
        "hooks",
        "-c",
        "approval_policy=\"never\"",
        // 웹 검색은 켠다(단서용). 앱 · 브라우저 · 컴퓨터 쓰기 기능은 끈다(티켓 21 X2)
        "-c",
        "web_search=\"cached\"",
        "--disable",
        "apps",
        "--disable",
        "browser_use",
        "--disable",
        "computer_use",
        "--disable",
        "in_app_browser",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    if let Some(m) = s.model {
        a.extend(["-m".to_string(), m.to_string()]);
    }
    if let Some(e) = s.effort {
        a.extend([
            "-c".to_string(),
            format!("model_reasoning_effort={}", toml_str(e)),
        ]);
    }
    if s.windows {
        // Windows: 권한 프로필이 없다 — workspace-write(네트워크 기본 꺼짐)
        a.extend(["-s".to_string(), "workspace-write".to_string()]);
    } else {
        let mut fs: Vec<(String, &str)> = vec![
            (":root".to_string(), "read"),
            (crate::paths::slash(s.home), "deny"),
            (":workspace_roots".to_string(), "write"),
            (":tmpdir".to_string(), "write"),
            (crate::paths::slash(s.docs), "read"),
        ];
        fs.extend(s.skills.iter().map(|d| (crate::paths::slash(d), "read")));
        let body = fs
            .iter()
            .map(|(k, v)| format!("{}={}", toml_str(k), toml_str(v)))
            .collect::<Vec<_>>()
            .join(",");
        a.extend([
            "-c".to_string(),
            "default_permissions=\"pes\"".to_string(),
            "-c".to_string(),
            format!("permissions.pes={{filesystem={{{body}}},network={{enabled=false}}}}"),
        ]);
    }
    a.extend([
        "-c".to_string(),
        format!(
            "mcp_servers.pes.url={}",
            toml_str(&format!("{}/mcp", s.base))
        ),
        "-c".to_string(),
        "mcp_servers.pes.bearer_token_env_var=\"PES_MCP_TOKEN\"".to_string(),
        "-c".to_string(),
        "shell_environment_policy.exclude=[\"PES_MCP_TOKEN\"]".to_string(),
        "-c".to_string(),
        "mcp_servers.pes.default_tools_approval_mode=\"approve\"".to_string(),
        "-c".to_string(),
        format!("mcp_servers.pes_local.url={}", toml_str(s.local_url)),
        "-c".to_string(),
        "mcp_servers.pes_local.default_tools_approval_mode=\"approve\"".to_string(),
        "-C".to_string(),
        crate::paths::slash(s.work),
        "-".to_string(),
    ]);
    a
}

/// OpenCode 설정(OPENCODE_CONFIG). carry: 내 opencode.json 에서 옮겨 오는 모델 · 제공자
pub fn opencode_config(s: &Spec, carry: Map<String, Value>) -> Value {
    let mut bash = Map::new();
    bash.insert("*".to_string(), json!("deny"));
    for c in SHELL_OK {
        bash.insert(c.to_string(), json!("allow"));
        bash.insert(format!("{c} *"), json!("allow"));
    }
    let mut outside = Map::new();
    outside.insert("*".to_string(), json!("deny"));
    let docs = crate::paths::slash(s.docs);
    for d in std::iter::once(docs).chain(s.skills.iter().map(|d| crate::paths::slash(d))) {
        outside.insert(format!("{d}/**"), json!("allow"));
        outside.insert(d, json!("allow"));
    }
    let mut cfg = carry;
    cfg.insert(
        "$schema".to_string(),
        json!("https://opencode.ai/config.json"),
    );
    if let Some(m) = s.model {
        cfg.insert("model".to_string(), json!(m));
    }
    cfg.insert(
        "mcp".to_string(),
        json!({
            "pes": { "type": "remote", "url": format!("{}/mcp", s.base), "headers": { "Authorization": "Bearer {env:PES_MCP_TOKEN}" }, "oauth": false, "enabled": true },
            "pes_local": { "type": "remote", "url": s.local_url, "oauth": false, "enabled": true },
        }),
    );
    // websearch 는 넣지 않는다(켬). webfetch · 하위 에이전트는 끈다 — 거부하면 도구가 목록에서 빠진다(티켓 21 O)
    cfg.insert(
        "permission".to_string(),
        json!({ "edit": "deny", "webfetch": "deny", "task": "deny", "external_directory": Value::Object(outside), "bash": Value::Object(bash) }),
    );
    Value::Object(cfg)
}

pub fn opencode_args(model_variant: Option<&str>) -> Vec<String> {
    let mut a: Vec<String> = ["run", "--standalone", "--format", "json", "--auto"]
        .iter()
        .map(|x| x.to_string())
        .collect();
    if let Some(m) = model_variant {
        a.extend(["--model".to_string(), m.to_string()]);
    }
    a
}

/// OpenCode 에 주는 환경 변수 — 위쪽 폴더 opencode.json 을 싣지 않게(티켓 21: 브리지 규칙을 덮는다)
pub fn opencode_env(cfg: &Path, xdg: &Path) -> Vec<(String, String)> {
    vec![
        ("OPENCODE_CONFIG".to_string(), crate::paths::slash(cfg)),
        ("XDG_CONFIG_HOME".to_string(), crate::paths::slash(xdg)),
        (
            "OPENCODE_DISABLE_PROJECT_CONFIG".to_string(),
            "1".to_string(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec<'a>(work: &'a Path, docs: &'a Path, home: &'a Path, windows: bool) -> Spec<'a> {
        Spec {
            base: "https://pes.example",
            local_url: "http://127.0.0.1:5/mcp/tok",
            work,
            docs,
            skills: &[],
            home,
            model: None,
            effort: None,
            windows,
        }
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("2.1.332 (Claude Code)"), Some((2, 1, 332)));
        assert_eq!(parse_version("codex-cli 0.172.0"), Some((0, 172, 0)));
        assert_eq!(parse_version("opencode v2.4.2\n"), Some((2, 4, 2)));
        assert_eq!(parse_version("no version"), None);
        assert!((2, 1, 331) < MIN_CLAUDE && (2, 2, 0) > MIN_CLAUDE);
    }

    #[test]
    fn claude_blocks_fetch_and_shell_network() {
        let (w, d, h) = (Path::new("/w"), Path::new("/d"), Path::new("/h"));
        let a = claude_args(&spec(w, d, h, false), Path::new("/a/mcp.json"));
        let tools = a
            .iter()
            .position(|x| x == "--tools")
            .map(|i| a[i + 1..i + 4].to_vec())
            .unwrap_or_default();
        assert_eq!(
            tools,
            ["WebSearch", "Read", "Bash"],
            "WebFetch 는 도구 목록에 없다"
        );
        assert!(!a.iter().any(|x| x.contains("WebFetch")));
        assert!(
            a.contains(&"--restricted".to_string())
                && a.contains(&"--strict-mcp-config".to_string())
        );
        assert!(
            a.contains(&"Bash(jq:*)".to_string()) && !a.iter().any(|x| x.starts_with("Bash(kosis"))
        );
        assert!(a.contains(&"mcp__pes_local".to_string()));
        let set = a
            .iter()
            .position(|x| x == "--settings")
            .and_then(|i| a.get(i + 1))
            .map(String::as_str)
            .unwrap_or("");
        let v: Value = serde_json::from_str(set).unwrap_or(Value::Null);
        assert_eq!(
            v.pointer("/sandbox/network/allowedDomains"),
            Some(&json!([]))
        );
        assert_eq!(
            v.pointer("/sandbox/allowUnsandboxedCommands"),
            Some(&json!(false))
        );
        // Windows: 샌드박스 없음(글 규칙 + 감지)
        let wa = claude_args(&spec(w, d, h, true), Path::new("/a/mcp.json"));
        assert!(!wa.contains(&"--settings".to_string()));
    }

    #[test]
    fn codex_network_off_and_web_search_on() {
        let (w, d, h) = (Path::new("/w"), Path::new("/d"), Path::new("/h"));
        let a = codex_args(&spec(w, d, h, false)).join(" ");
        assert!(a.contains("network={enabled=false}"));
        assert!(a.contains("web_search=\"cached\""));
        for f in [
            "--disable apps",
            "--disable browser_use",
            "--disable computer_use",
            "--disable in_app_browser",
            "--ignore-user-config",
            "--disable hooks",
        ] {
            assert!(a.contains(f), "{f}");
        }
        assert!(a.contains("\"/h\"=\"deny\""), "홈 폴더는 못 읽는다");
        assert!(a.contains("mcp_servers.pes_local.url=\"http://127.0.0.1:5/mcp/tok\""));
        assert!(a.ends_with("-C /w -"));
        assert!(!a.contains("network={enabled=true}"));
    }

    #[test]
    fn opencode_denies_webfetch_and_shell() {
        let (w, d, h) = (Path::new("/w"), Path::new("/d"), Path::new("/h"));
        let mut carry = Map::new();
        carry.insert("model".to_string(), json!("p/m"));
        let c = opencode_config(&spec(w, d, h, false), carry);
        assert_eq!(c.pointer("/permission/webfetch"), Some(&json!("deny")));
        assert_eq!(c.pointer("/permission/task"), Some(&json!("deny")));
        assert_eq!(c.pointer("/permission/websearch"), None, "웹 검색은 켠다");
        assert_eq!(c.pointer("/permission/bash/*"), Some(&json!("deny")));
        assert_eq!(c.pointer("/permission/bash/jq *"), Some(&json!("allow")));
        assert_eq!(c.pointer("/permission/bash/kosis"), None);
        assert_eq!(
            c.pointer("/mcp/pes_local/url"),
            Some(&json!("http://127.0.0.1:5/mcp/tok"))
        );
        assert_eq!(c.get("model"), Some(&json!("p/m")));
        let env = opencode_env(Path::new("/c.json"), Path::new("/x"));
        assert!(env.contains(&(
            "OPENCODE_DISABLE_PROJECT_CONFIG".to_string(),
            "1".to_string()
        )));
    }
}
