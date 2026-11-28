//! HTML → 읽을 글. 링크는 「글 (주소)」로 남긴다(명세: 검색 결과의 링크도 빠지지 않아야 한다).
//! 원본은 근거 기록에 HTML 그대로 올리고, 이것은 AI 가 읽는 글이다.

fn entity(e: &str) -> Option<String> {
    let s = match e {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" | "#39" => "'",
        "nbsp" => " ",
        "middot" => "·",
        _ => {
            let n = e.strip_prefix('#')?;
            let code = match n.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => n.parse().ok()?,
            };
            return char::from_u32(code).map(String::from);
        }
    };
    Some(s.to_string())
}

pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        match tail
            .find(';')
            .filter(|&j| j <= 10)
            .and_then(|j| entity(&tail[..j]).map(|v| (j, v)))
        {
            Some((j, v)) => {
                out.push_str(&v);
                rest = &tail[j + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(p) = lower[from..].find(name) {
        let at = from + p;
        let before_ok = at == 0
            || lower
                .as_bytes()
                .get(at - 1)
                .is_some_and(|b| b.is_ascii_whitespace());
        let after = lower[at + name.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let off = tag.len() - after.len() + 1;
            let v = tag[off..].trim_start();
            let val = match v.chars().next() {
                Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or_default(),
                _ => v
                    .split(|c: char| c.is_ascii_whitespace() || c == '>')
                    .next()
                    .unwrap_or_default(),
            };
            return Some(decode_entities(val));
        }
        from = at + name.len();
    }
    None
}

const BLOCK: [&str; 22] = [
    "p", "div", "br", "li", "tr", "h1", "h2", "h3", "h4", "h5", "h6", "table", "section",
    "article", "header", "footer", "ul", "ol", "dt", "dd", "hr", "caption",
];

/// base: 상대 주소를 풀 기준(받은 페이지의 최종 주소)
pub fn to_text(html: &str, base: Option<&reqwest::Url>) -> String {
    let mut out = String::new();
    let mut rest = html;
    let mut href: Vec<Option<String>> = Vec::new();
    while let Some(i) = rest.find('<') {
        out.push_str(&decode_entities(&rest[..i]));
        let after = &rest[i + 1..];
        let Some(end) = after.find('>') else {
            rest = "";
            break;
        };
        let tag = &after[..end];
        rest = &after[end + 1..];
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        let closing = tag.starts_with('/');
        if !closing
            && (name == "script" || name == "style" || name == "noscript" || name == "template")
        {
            let close = format!("</{name}");
            match rest.to_ascii_lowercase().find(&close) {
                Some(j) => rest = rest[j..].split_once('>').map(|x| x.1).unwrap_or(""),
                None => rest = "",
            }
            continue;
        }
        if tag.starts_with("!--") {
            if let Some(j) = after.find("-->") {
                rest = &after[j + 3..];
            }
            continue;
        }
        if name == "a" {
            if closing {
                if let Some(Some(h)) = href.pop() {
                    out.push_str(&format!(" ({h})"));
                }
            } else {
                let h = attr(tag, "href")
                    .filter(|h| {
                        !h.starts_with('#') && !h.to_ascii_lowercase().starts_with("javascript:")
                    })
                    .map(|h| match base.and_then(|b| b.join(&h).ok()) {
                        Some(u) => u.to_string(),
                        None => h,
                    });
                href.push(h);
            }
        } else if BLOCK.contains(&name.as_str()) {
            out.push('\n');
        } else if name == "td" || name == "th" {
            out.push_str(" | ");
        }
    }
    out.push_str(&decode_entities(rest));
    // 빈칸 정리: 줄 안의 겹친 빈칸은 하나로, 빈 줄은 하나까지
    let mut clean = String::with_capacity(out.len());
    let mut blank = 0;
    for l in out.lines() {
        let t = l.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() {
            blank += 1;
            if blank == 1 && !clean.is_empty() {
                clean.push('\n');
            }
        } else {
            blank = 0;
            clean.push_str(&t);
            clean.push('\n');
        }
    }
    clean.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_links() -> Result<(), Box<dyn std::error::Error>> {
        let base = reqwest::Url::parse("https://www.example.go.kr/a/b.html")?;
        let h = "<html><head><style>x{}</style><script>alert(1)</script></head><body><h1>제목 &amp; 부제</h1><p>인구는 <b>51,000,000</b>명</p><a href=\"/c.pdf\">첨부</a><!-- 주석 --><table><tr><td>1</td><td>2</td></tr></table></body></html>";
        let t = to_text(h, Some(&base));
        assert!(t.contains("제목 & 부제"));
        assert!(t.contains("인구는 51,000,000명"));
        assert!(t.contains("첨부 (https://www.example.go.kr/c.pdf)"));
        assert!(!t.contains("alert") && !t.contains("x{}") && !t.contains("주석"));
        assert_eq!(
            decode_entities("&#54620;&#xAE00; &lt;b&gt; &zz;"),
            "한글 <b> &zz;"
        );
        Ok(())
    }
}
