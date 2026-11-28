//! 내부 주소 막기 — pes_local 의 원문 열기 · ego-browser 가 이 컴퓨터 · 사설망(로컬 서비스, pes_local 자신, 공유기 등)을 열지 못하게 한다.
//! - 주소에 IP 가 바로 적힌 것: blocked_host 로 본다(처음 주소 · 넘김 주소 모두, 넘김 정책에서).
//! - 이름: reqwest 의 이름 풀이(dns_resolver)를 Guard 로 바꿔, 풀린 주소 가운데 하나라도 내부면 연결하지 않는다.
//!   처음 · 넘김 · 다시 풀기 모두 이 길을 지난다(연결할 때마다 풀기 때문). 프록시는 쓰지 않는다(no_proxy — 프록시가 대신 풀면 못 본다).
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
#[cfg(test)]
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

fn blocked_v4(v: Ipv4Addr) -> bool {
    let [a, b, c, _] = v.octets();
    a == 0 // 0.0.0.0/8
        || a == 10
        || a == 127
        || (a == 100 && (b & 0xc0) == 64) // 100.64/10 공유 주소(CGNAT)
        || (a == 169 && b == 254)
        || (a == 172 && (b & 0xf0) == 16)
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && c == 0) // 192.0.0/24
        || (a == 192 && b == 0 && c == 2) // 문서용
        || (a == 198 && (b & 0xfe) == 18) // 198.18/15 시험망
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224 // 멀티캐스트 224/4 · 예약 240/4 · 255.255.255.255
}

fn embedded_v4(v: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = v.segments();
    let o = v.octets();
    if s[..5] == [0, 0, 0, 0, 0] && (s[5] == 0xffff || s[5] == 0) {
        // ::ffff:a.b.c.d(IPv4 대응) · ::a.b.c.d(옛 호환)
        return Some(Ipv4Addr::new(o[12], o[13], o[14], o[15]));
    }
    if s[0] == 0x64 && s[1] == 0xff9b {
        // 64:ff9b::/96 NAT64(64:ff9b:1::/48 도)
        return Some(Ipv4Addr::new(o[12], o[13], o[14], o[15]));
    }
    if s[0] == 0x2002 {
        // 6to4 2002:aabb:ccdd::/48
        return Some(Ipv4Addr::new(o[2], o[3], o[4], o[5]));
    }
    None
}

pub fn blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => blocked_v4(v),
        IpAddr::V6(v) => {
            let s0 = v.segments()[0];
            if let Some(x) = embedded_v4(v) {
                // 대응 · NAT64 · 6to4 는 안에 든 IPv4 로 본다. ::/96 의 0.0.0.0 · 0.0.0.1(:: · ::1)도 막힌다
                return blocked_v4(x) || v.is_unspecified() || v.is_loopback();
            }
            v.is_unspecified()
                || v.is_loopback()
                || (s0 & 0xfe00) == 0xfc00 // 고유 로컬
                || (s0 & 0xffc0) == 0xfe80 // 링크 로컬
                || (s0 & 0xffc0) == 0xfec0 // 옛 사이트 로컬
                || (s0 & 0xff00) == 0xff00 // 멀티캐스트
                || (s0 == 0x2001 && v.segments()[1] == 0) // Teredo 2001::/32
                || (s0 == 0x2001 && v.segments()[1] == 0x0db8) // 문서용
                || s0 == 0x0100 && v.segments()[1..4] == [0, 0, 0] // 버림 100::/64
        }
    }
}

/// 주소에 적힌 이름 · IP 만으로 막을 것(풀지 않는다)
pub fn blocked_host(u: &reqwest::Url) -> bool {
    match u.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost"
                || d.ends_with(".localhost")
                || d.ends_with(".local")
                || d.ends_with(".internal")
                || d.ends_with(".home.arpa")
        }
        Some(url::Host::Ipv4(v)) => blocked_v4(v),
        Some(url::Host::Ipv6(v)) => blocked_ip(IpAddr::V6(v)),
        None => true,
    }
}

type LookupFut = Pin<Box<dyn Future<Output = std::io::Result<Vec<IpAddr>>> + Send>>;

/// 이름 풀기 — 평소에는 운영체제, 시험에서는 정한 표
pub trait Lookup: Send + Sync {
    fn lookup(&self, name: String) -> LookupFut;
}

pub struct SystemLookup;

impl Lookup for SystemLookup {
    fn lookup(&self, name: String) -> LookupFut {
        Box::pin(async move {
            Ok(tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .map(|a| a.ip())
                .collect())
        })
    }
}

/// 시험용 — 이름 → 주소 표
#[cfg(test)]
pub struct TableLookup(pub HashMap<String, Vec<IpAddr>>);

#[cfg(test)]
impl Lookup for TableLookup {
    fn lookup(&self, name: String) -> LookupFut {
        let r = self.0.get(&name).map(|v| v.to_vec()).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, format!("{name}: 없는 이름"))
        });
        Box::pin(async move { r })
    }
}

#[derive(Clone)]
pub struct Guard {
    lookup: Arc<dyn Lookup>,
    /// 시험용: 내부 주소로 풀려도 여는 이름(평소에는 비어 있다)
    open: Arc<Vec<String>>,
    /// 시험용: 막기를 모두 끈다(PES_BRIDGE_ALLOW_LOOPBACK_FETCH=1)
    off: bool,
}

impl Guard {
    pub fn new(lookup: Arc<dyn Lookup>, open: Vec<String>, off: bool) -> Self {
        Self {
            lookup,
            open: Arc::new(open),
            off,
        }
    }

    /// 이름을 풀어 내부 주소가 하나라도 있으면 거절
    pub async fn resolve_ok(&self, name: &str) -> Result<Vec<IpAddr>, String> {
        let ips = self
            .lookup
            .lookup(name.to_string())
            .await
            .map_err(|e| format!("주소를 찾지 못했어요: {e}"))?;
        if ips.is_empty() {
            return Err(format!("{name}: 풀린 주소가 없어요"));
        }
        if !self.off && !self.open.iter().any(|n| n == name) && ips.iter().any(|ip| blocked_ip(*ip))
        {
            return Err(format!(
                "{name} 은 내부 주소(이 컴퓨터 · 사설망)로 풀려 열지 않아요"
            ));
        }
        Ok(ips)
    }

    /// 주소 하나를 연결 전에 본다(적힌 IP · 이름 모두)
    pub async fn check_url(&self, u: &reqwest::Url) -> Result<(), String> {
        if self.off {
            return Ok(());
        }
        if blocked_host(u) {
            return Err("내부 주소(이 컴퓨터 · 사설망)는 열지 않아요".to_string());
        }
        if let Some(url::Host::Domain(d)) = u.host() {
            self.resolve_ok(d).await?;
        }
        Ok(())
    }

    /// 원문 열기용 HTTP — 이름 풀이는 Guard, 넘김은 적힌 주소를 보고, 프록시는 쓰지 않는다
    pub fn client(&self, ua: &str) -> reqwest::Result<reqwest::Client> {
        let off = self.off;
        reqwest::Client::builder()
            .user_agent(ua)
            .timeout(Duration::from_secs(120))
            .no_proxy()
            // clone: Guard 는 Arc 묶음 — 클라이언트가 자기 몫을 갖는다
            .dns_resolver(Arc::new(self.clone()))
            .redirect(reqwest::redirect::Policy::custom(move |at| {
                if at.previous().len() >= 10 {
                    at.error("주소 넘김이 너무 많아요")
                } else if !off && blocked_host(at.url()) {
                    at.error("내부 주소로 넘기는 것은 따라가지 않아요")
                } else if !matches!(at.url().scheme(), "http" | "https") {
                    at.error("http(s) 가 아닌 주소로 넘기는 것은 따라가지 않아요")
                } else {
                    at.follow() // 넘긴 곳의 이름은 연결할 때 Guard 가 풀어 본다
                }
            }))
            .build()
    }
}

impl Resolve for Guard {
    fn resolve(&self, name: Name) -> Resolving {
        // clone: Guard 는 Arc 묶음 — 풀기 작업이 자기 몫을 갖고 따로 돈다
        let g = self.clone();
        Box::pin(async move {
            let ips = g
                .resolve_ok(name.as_str())
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            let addrs: Addrs = Box::new(ips.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
    }

    #[test]
    fn blocked_ranges() {
        for s in [
            "0.1.2.3",
            "10.0.0.1",
            "127.0.0.1",
            "127.9.9.9",
            "100.64.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.1",
            "224.0.0.1",
            "239.1.1.1",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b::a00:1",
            "2002:7f00:1::1",
            "2002:c0a8:101::",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "ff02::1",
            "2001::1",
            "2001:db8::1",
        ] {
            assert!(blocked_ip(ip(s)), "{s}");
        }
        for s in [
            "8.8.8.8",
            "211.110.1.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::1",
        ] {
            assert!(!blocked_ip(ip(s)), "{s}");
        }
    }

    #[test]
    fn literal_hosts() -> Result<(), Box<dyn std::error::Error>> {
        for u in [
            "http://[::ffff:127.0.0.1]:80/",
            "http://127.0.0.1/",
            "http://localhost./",
            "http://x.home.arpa/",
            "http://[64:ff9b::7f00:1]/",
            "http://0.0.0.0/",
        ] {
            assert!(blocked_host(&reqwest::Url::parse(u)?), "{u}");
        }
        assert!(!blocked_host(&reqwest::Url::parse("https://kosis.kr/")?));
        Ok(())
    }

    /// 로컬 가짜 리졸버로: 내부로 풀리는 이름 · 넘김 · 적힌 내부 IP 가 막히는지(막기를 끄지 않고)
    #[tokio::test]
    async fn client_blocks_internal_names_and_redirects() -> Result<(), Box<dyn std::error::Error>>
    {
        use axum::Router;
        use axum::http::{StatusCode, header};
        use axum::response::IntoResponse;
        use axum::routing::get;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = l.local_addr()?.port();
        let to = move |u: String| (StatusCode::FOUND, [(header::LOCATION, u)]).into_response();
        let app = Router::new()
            .route("/ok", get(|| async { "ok" }))
            .route(
                "/to-name",
                get(move || async move { to(format!("http://evil.test:{port}/ok")) }),
            )
            .route(
                "/to-ip",
                get(move || async move { to(format!("http://127.0.0.1:{port}/ok")) }),
            )
            .route(
                "/to-mapped",
                get(move || async move { to(format!("http://[::ffff:127.0.0.1]:{port}/ok")) }),
            );
        tokio::spawn(async move {
            let _ = axum::serve(l, app).await;
        });
        let mut t = HashMap::new();
        let lo = IpAddr::V4(Ipv4Addr::LOCALHOST);
        t.insert("pub.test".to_string(), vec![lo]); // 시험에서만 「공개」로 여는 이름
        t.insert("evil.test".to_string(), vec![lo]);
        t.insert("evil6.test".to_string(), vec![ip("::ffff:127.0.0.1")]);
        t.insert("mixed.test".to_string(), vec![ip("8.8.8.8"), lo]);
        let g = Guard::new(
            Arc::new(TableLookup(t)),
            vec!["pub.test".to_string()],
            false,
        );
        let c = g.client("t")?;
        let ok = c.get(format!("http://pub.test:{port}/ok")).send().await?;
        assert_eq!(ok.status(), 200, "열어 둔 이름은 열린다(시험 장치 확인)");
        for path in ["to-name", "to-ip", "to-mapped"] {
            assert!(
                c.get(format!("http://pub.test:{port}/{path}"))
                    .send()
                    .await
                    .is_err(),
                "넘김 {path} 은 막혀야 해요"
            );
        }
        assert!(
            c.get(format!("http://evil.test:{port}/ok"))
                .send()
                .await
                .is_err(),
            "127.0.0.1 로 풀리는 이름"
        );
        for u in [
            format!("http://evil.test:{port}/ok"),
            format!("http://evil6.test:{port}/ok"),
            format!("http://mixed.test:{port}/ok"),
            format!("http://[::ffff:127.0.0.1]:{port}/ok"),
            format!("http://127.0.0.1:{port}/ok"),
        ] {
            assert!(g.check_url(&reqwest::Url::parse(&u)?).await.is_err(), "{u}");
        }
        Ok(())
    }
}
