//! 조사 CLI 인자 검사 · 이 컴퓨터 키 확인 · 출력에서 키 지우기.
//! - 하위 명령은 조회 계열만 허용한다(각 CLI --help 로 정함, 2026-11-28 판). 설정 · 업데이트 · 배치 · 지도 · 차트 · 대화형 등은 거부.
//! - 파일을 읽거나 쓰는 플래그(-o --output --input --from --config --db --shp --outputPath …)는 거부한다 — AI 가 브리지를 통해 파일을 읽고 쓰지 못하게.
//! - 키 중계(티켓 23) 전까지 조사 CLI 는 이 컴퓨터 CLI 의 키로 돈다. 키가 없으면 그 도구는 분명한 까닭으로 실패한다.
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::Value;
use std::path::Path;

struct Rule {
    bin: &'static str,
    subs: &'static [&'static str],
    /// 하위 명령 이름 앞부분으로 허용(korean-law 도구 이름)
    prefixes: &'static [&'static str],
    deny: &'static [&'static str],
    key_hint: &'static str,
}

const RULES: [(&str, Rule); 4] = [
    (
        "kosis",
        Rule {
            bin: "kosis",
            subs: &[
                "search",
                "s",
                "meta",
                "m",
                "data",
                "d",
                "list",
                "ls",
                "explain",
                "ex",
                "indicator",
                "ind",
            ],
            prefixes: &[],
            deny: &[
                "-o",
                "--output",
                "--chart",
                "--chart-format",
                "--template",
                "--config",
            ],
            key_hint: "kosis config set-key <키>",
        },
    ),
    (
        "vworld",
        Rule {
            bin: "vworld",
            subs: &[
                "geocode", "geocoder", "search", "data", "ned", "wms", "wfs", "catalog",
            ],
            prefixes: &[],
            deny: &[
                "-o",
                "--output",
                "--config",
                "--input",
                "--from",
                "--hjd-shp",
                "--hjd-db",
                "--shp",
                "--db",
                "--out-dir",
            ],
            key_hint: "vworld config add-key <키>",
        },
    ),
    (
        "sgis",
        Rule {
            bin: "sgis",
            subs: &["data", "boundary", "geocode", "search", "code"],
            prefixes: &[],
            deny: &["-o", "--output", "--config"],
            key_hint: "sgis config set-credential <서비스 ID> <보안 Key>",
        },
    ),
    (
        "korean_law",
        Rule {
            bin: "korean-law",
            subs: &[
                "query",
                "q",
                "explain",
                "list",
                "ls",
                "advanced_search",
                "ordinance_radar",
                "parse_article_links",
                "summarize_precedent",
                "extract_precedent_keywords",
                "find_similar_precedents",
            ],
            prefixes: &["search_", "get_", "compare_", "suggest_"],
            deny: &["--outputPath", "--output", "-o"],
            key_hint: "LAW_OC(법령 API 값)를 설치 스크립트로 넣기",
        },
    ),
];

fn rule(tool: &str) -> Option<&'static Rule> {
    RULES.iter().find(|(t, _)| *t == tool).map(|(_, r)| r)
}

pub fn bin_of(tool: &str) -> Option<&'static str> {
    rule(tool).map(|r| r.bin)
}

/// 인자 검사 — 첫 인자는 허용한 하위 명령, 파일 플래그 없음
pub fn check_args(tool: &str, args: &[String]) -> Result<(), String> {
    let r = rule(tool).ok_or_else(|| format!("없는 조사 도구예요: {tool}"))?;
    let sub = args.first().map(String::as_str).unwrap_or_default();
    let sub_ok = r.subs.contains(&sub) || r.prefixes.iter().any(|p| sub.starts_with(p));
    if !sub_ok {
        return Err(format!(
            "{} 의 하위 명령 「{sub}」은 쓸 수 없어요(조회 명령만: {}{}). 첫 인자에 하위 명령을 주세요.",
            r.bin,
            r.subs.join(" · "),
            if r.prefixes.is_empty() {
                String::new()
            } else {
                format!(" · {}…", r.prefixes.join("… · "))
            }
        ));
    }
    for a in args {
        let bad = r.deny.iter().any(|f| {
            a == f
                || a.starts_with(&format!("{f}="))
                || (f.len() == 2
                    && !a.starts_with("--")
                    && a.starts_with(f)
                    && a.len() > 2
                    && a.chars().nth(2).is_some_and(|c| !c.is_ascii_digit()))
        });
        if bad {
            return Err(format!(
                "{} 에서 파일을 읽거나 쓰는 옵션({a})은 쓸 수 없어요 — 결과는 답으로 받아요.",
                r.bin
            ));
        }
        if tool == "korean_law" && a.contains("outputPath") {
            return Err("korean-law 의 outputPath(파일 저장)는 쓸 수 없어요.".to_string());
        }
    }
    Ok(())
}

/// 설정 파일 줄에서 키 같은 값을 뽑는다: "- 값"(목록) · "이름: 값" · "이름 = \"값\"" 에서 이름에 key · secret · token · oc 가 든 것
fn values_from_text(text: &str, list_ok: bool) -> Vec<String> {
    let mut out = Vec::new();
    for l in text.lines() {
        let t = l.trim();
        let v = if let Some(x) = t.strip_prefix("- ").filter(|_| list_ok) {
            Some(x)
        } else {
            t.split_once([':', '=']).and_then(|(k, v)| {
                let k = k.trim().to_ascii_lowercase();
                (k.contains("key")
                    || k.contains("secret")
                    || k.contains("token")
                    || k == "oc"
                    || k.ends_with("_oc"))
                .then_some(v)
            })
        };
        if let Some(v) = v {
            let v = v
                .trim()
                .trim_matches(|c| c == '"' || c == '\'' || c == ',')
                .trim();
            if v.len() >= 6 && !v.contains(' ') {
                out.push(v.to_string());
            }
        }
    }
    out
}

fn json_values(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                let k = k.to_ascii_lowercase();
                match x {
                    Value::String(s)
                        if (k.contains("token") || k.contains("key") || k.contains("secret"))
                            && s.len() >= 6 =>
                    {
                        out.push(s.to_string())
                    }
                    _ => json_values(x, out),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| json_values(x, out)),
        _ => {}
    }
}

/// 이 컴퓨터에 있는 그 도구의 키(값) — 키 확인과 출력 지우기에 쓴다. 값은 로그 · 기록에 남기지 않는다
pub fn local_keys(tool: &str, home: &Path) -> Vec<String> {
    let read = |p: &str| std::fs::read_to_string(home.join(p)).unwrap_or_default();
    let env = |names: &[&str]| -> Vec<String> {
        names
            .iter()
            .filter_map(|n| std::env::var(n).ok())
            .flat_map(|v| {
                v.split(',')
                    .map(|x| x.trim().to_string())
                    .collect::<Vec<_>>()
            })
            .filter(|v| v.len() >= 4)
            .collect()
    };
    let mut v = match tool {
        "kosis" => {
            let mut v = values_from_text(&read(".kosis/config.yaml"), true);
            v.extend(env(&["KOSIS_API_KEY", "KOSIS_API_KEYS"]));
            v
        }
        "vworld" => {
            let mut v = values_from_text(&read(".vworld/config.toml"), false);
            v.extend(env(&["VWORLD_API_KEYS"]));
            v
        }
        "sgis" => {
            let mut v = values_from_text(&read(".sgis/config.yaml"), false);
            for f in std::fs::read_dir(home.join(".sgis"))
                .into_iter()
                .flatten()
                .flatten()
            {
                if f.file_name().to_string_lossy().ends_with(".json")
                    && let Ok(j) = serde_json::from_str::<Value>(
                        &std::fs::read_to_string(f.path()).unwrap_or_default(),
                    )
                {
                    json_values(&j, &mut v);
                }
            }
            v.extend(env(&[
                "SGIS_CONSUMER_KEY",
                "SGIS_CONSUMER_SECRET",
                "SGIS_API_KEYS",
            ]));
            v
        }
        "korean_law" => {
            let mut v = env(&["LAW_OC", "KOREAN_LAW_API_KEY"]);
            if let Ok(j) = serde_json::from_str::<Value>(&read(".claude/settings.json"))
                && let Some(oc) = j.pointer("/env/LAW_OC").and_then(Value::as_str)
            {
                v.push(oc.to_string());
            }
            v
        }
        _ => Vec::new(),
    };
    v.sort();
    v.dedup();
    v
}

/// 키가 없을 때의 까닭(있으면 None)
pub fn missing_key(tool: &str, home: &Path) -> Option<String> {
    let r = rule(tool)?;
    local_keys(tool, home).is_empty().then(|| {
        format!("이 컴퓨터의 {} 에 키가 없어 조회하지 못해요. PES 키 중계(티켓 23)가 붙기 전에는 이 컴퓨터 CLI 에 키를 넣어야 해요: {}", r.bin, r.key_hint)
    })
}

/// 지울 글 목록 — 키 그대로 · 주소 꼴(%XX) · base64 로 감싼 키의 속 값
pub fn scrub_list(keys: &[String]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for k in keys {
        v.push(k.to_string());
        let enc: String = k
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        if enc != *k {
            v.push(enc);
        }
        if let Ok(d) = STANDARD.decode(k.as_bytes())
            && let Ok(s) = String::from_utf8(d)
            && s.len() >= 6
            && s.chars().all(|c| c.is_ascii_graphic())
        {
            v.push(s);
        }
    }
    v.retain(|x| x.len() >= 4);
    v.sort_by_key(|x| std::cmp::Reverse(x.len())); // 긴 것부터(짧은 것이 긴 것 안에 들 때)
    v.dedup();
    v
}

pub fn scrub(text: &str, list: &[String]) -> String {
    let mut t = text.to_string();
    for k in list {
        if t.contains(k.as_str()) {
            t = t.replace(k.as_str(), "(지움)");
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn allowed_and_denied_args() {
        assert!(
            check_args(
                "kosis",
                &s(&["data", "101", "DT_1", "-c1", "11", "-l", "5"])
            )
            .is_ok()
        );
        assert!(check_args("kosis", &s(&["search", "인구"])).is_ok());
        for bad in [
            &["config", "set-key", "x"][..],
            &["update"],
            &["chart", "x"],
            &["bulk", "x"],
            &["data", "101", "DT", "-o", "/tmp/x.csv"],
            &["data", "--output=/tmp/x"],
            &["data", "-o/tmp/x"],
            &["--config", "x", "search"],
        ] {
            assert!(check_args("kosis", &s(bad)).is_err(), "{bad:?}");
        }
        assert!(check_args("vworld", &s(&["geocode", "세종대로 110"])).is_ok());
        for bad in [
            &["batch", "geocode", "--from", "a.txt"][..],
            &["map", "choropleth"],
            &["staticmap"],
            &["hjd-db", "build"],
            &["ned", "x", "--shp", "a.shp"],
            &["geocode", "--input", "/etc/passwd"],
            &["wfs", "-o", "x"],
        ] {
            assert!(check_args("vworld", &s(bad)).is_err(), "{bad:?}");
        }
        assert!(check_args("sgis", &s(&["data", "population", "--adm-cd", "11"])).is_ok());
        assert!(check_args("sgis", &s(&["data", "population", "-o", "x.xlsx"])).is_err());
        assert!(check_args("korean_law", &s(&["query", "민법", "제1조"])).is_ok());
        assert!(check_args("korean_law", &s(&["get_law_text", "--mst", "1"])).is_ok());
        for bad in [
            &["interactive"][..],
            &["amendment_table", "--outputPath", "a.hwpx"],
            &["get_law_text", "--json-input", "{\"outputPath\":\"x\"}"],
            &["help", "x"],
        ] {
            assert!(check_args("korean_law", &s(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn keys_found_and_scrubbed() -> Result<(), Box<dyn std::error::Error>> {
        let h = tempfile::tempdir()?;
        std::fs::create_dir_all(h.path().join(".kosis"))?;
        let inner = "abcdef0123456789secret";
        std::fs::write(
            h.path().join(".kosis/config.yaml"),
            format!(
                "api_keys:\n    - {}\ncache_ttl_hours: 24\n",
                STANDARD.encode(inner)
            ),
        )?;
        std::fs::create_dir_all(h.path().join(".vworld"))?;
        std::fs::write(
            h.path().join(".vworld/config.toml"),
            "[[keys]]\nkey = \"VWKEY-123456\"\nalias = \"main\"\n",
        )?;
        assert!(missing_key("kosis", h.path()).is_none());
        assert!(
            missing_key("sgis", h.path()).is_some_and(|m| m.contains("sgis config set-credential"))
        );
        let list = scrub_list(&local_keys("kosis", h.path()));
        let out = scrub(
            &format!(
                "url=https://kosis.kr/x?apiKey={inner}&b=1 raw={}",
                STANDARD.encode(inner)
            ),
            &list,
        );
        assert!(!out.contains(inner) && out.contains("(지움)"), "{out}");
        assert_eq!(
            local_keys("vworld", h.path()),
            vec!["VWKEY-123456".to_string()]
        );
        Ok(())
    }
}
