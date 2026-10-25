use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::engine::Engine;

fn private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v) => {
            v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_multicast()
                || matches!(v.octets(), [0, ..] | [100, 64..=127, ..] | [198, 18 | 19, ..] | [240..=255, ..])
        }
        std::net::IpAddr::V6(v) => {
            let s = v.segments();
            // An IPv4 address inside: IPv4-compatible ::a.b.c.d, NAT64 64:ff9b::a.b.c.d, and 6to4 2002:aabb:ccdd::.
            let v4 = |hi: u16, lo: u16| private_ip(std::net::Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo)).into());
            v.to_ipv4_mapped().is_some_and(|m| private_ip(m.into()))
                || (s[..6] == [0; 6] && v4(s[6], s[7]))
                || (s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] && v4(s[6], s[7]))
                || (s[0] == 0x2002 && v4(s[1], s[2]))
                || s[0] & 0xffc0 == 0xfec0
                || v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_unique_local()
                || v.is_unicast_link_local()
        }
    }
}

fn private_name(host: &str) -> bool {
    const RESERVED: [&str; 8] = [".local", ".localhost", ".internal", ".lan", ".home", ".home.arpa", ".intranet", ".corp"];
    !host.contains('.') || RESERVED.iter().any(|s| host.ends_with(s))
}

enum Lookup {
    Pending,
    Done(bool),
}

static LOOKUPS: std::sync::Mutex<Option<HashMap<String, Lookup>>> = std::sync::Mutex::new(None);

fn lookups() -> std::sync::MutexGuard<'static, Option<HashMap<String, Lookup>>> {
    LOOKUPS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn refused_hosts(hosts: Vec<String>) -> Vec<String> {
    const IN_FLIGHT: usize = 64;
    let (tx, rx) = mpsc::channel();
    let mut pending = 0;
    {
        let mut guard = lookups();
        let seen = guard.get_or_insert_with(HashMap::new);
        let mut in_flight = seen.values().filter(|l| matches!(l, Lookup::Pending)).count();
        for h in &hosts {
            if seen.contains_key(h) {
                continue;
            }
            if private_name(h) {
                seen.insert(h.clone(), Lookup::Done(true));
                continue;
            }
            if in_flight >= IN_FLIGHT {
                break;
            }
            let (tx, host) = (tx.clone(), h.clone());
            let lookup = std::thread::Builder::new().spawn(move || {
                use std::net::ToSocketAddrs;
                let refused = (host.as_str(), 443).to_socket_addrs().ok().map(|mut a| a.any(|a| private_ip(a.ip())));
                let mut guard = lookups();
                let seen = guard.get_or_insert_with(HashMap::new);
                match refused {
                    Some(r) => seen.insert(host, Lookup::Done(r)),
                    None => seen.remove(&host),
                };
                drop(guard);
                let _ = tx.send(());
            });
            if lookup.is_err() {
                break;
            }
            seen.insert(h.clone(), Lookup::Pending);
            in_flight += 1;
            pending += 1;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    for _ in 0..pending {
        if rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_err() {
            break;
        }
    }
    let mut guard = lookups();
    let seen = guard.get_or_insert_with(HashMap::new);
    hosts.into_iter().filter(|h| !matches!(seen.get(h), Some(Lookup::Done(false)))).collect()
}

pub fn load(engine: &Engine) -> bool {
    let hosts = engine.eval("mddog.remoteHosts()").ok().and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok()).unwrap_or_default();
    if engine.eval("String(document.querySelectorAll('img[data-mddog-src], video[data-mddog-poster], [data-mddog-media]').length)").is_ok_and(|n| n == "0") {
        return false;
    }
    let refused = serde_json::to_string(&refused_hosts(hosts)).unwrap_or_else(|_| "[]".into());
    let _ = engine.eval(&format!("mddog.loadRemote(1000, {refused})"));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_hosts_are_refused() {
        for ip in ["10.1.2.3", "127.0.0.1", "169.254.1.1", "100.100.1.1", "0.1.2.3", "::1", "fd00::1", "fe80::1", "::ffff:192.168.1.1", "::127.0.0.1", "64:ff9b::a00:1", "2002:c0a8:101::1", "fec0::1"]
        {
            assert!(private_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "140.82.112.3", "2606:4700::1111", "64:ff9b::808:808", "2002:808:808::1"] {
            assert!(!private_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(private_name("router") && private_name("nas.home.arpa") && private_name("wiki.internal"));
        assert!(!private_name("img.shields.io"));
        assert_eq!(refused_hosts(vec!["router".into(), "localtest.me".into()]).len(), 2);
        let many: Vec<String> = (0..5000).map(|i| format!("h{i}.invalid")).collect();
        let t = Instant::now();
        assert_eq!(refused_hosts(many).len(), 5000);
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    }
}
