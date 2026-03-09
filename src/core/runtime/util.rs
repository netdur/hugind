use std::net::IpAddr;
use std::time::Duration;

pub fn parse_memory_string(mem: &str) -> Option<usize> {
    let mem = mem.trim().to_uppercase();
    if let Some(stripped) = mem.strip_suffix("MB") {
        return stripped.parse::<usize>().ok().map(|v| v * 1024 * 1024);
    }
    if let Some(stripped) = mem.strip_suffix("GB") {
        return stripped
            .parse::<usize>()
            .ok()
            .map(|v| v * 1024 * 1024 * 1024);
    }
    mem.parse::<usize>().ok()
}

pub fn parse_duration_string(s: &str) -> Option<Duration> {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        return ms.parse::<u64>().ok().map(Duration::from_millis);
    }
    if let Some(sec) = s.strip_suffix("s") {
        return sec.parse::<u64>().ok().map(Duration::from_secs);
    }
    if let Some(min) = s.strip_suffix("m") {
        return min
            .parse::<u64>()
            .ok()
            .map(|min| Duration::from_secs(min * 60));
    }
    s.parse::<u64>().ok().map(Duration::from_secs)
}

pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(addr) => {
            let octets = addr.octets();
            if octets[0] == 10 {
                return true;
            }
            if octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31) {
                return true;
            }
            if octets[0] == 192 && octets[1] == 168 {
                return true;
            }
            if octets[0] == 127 {
                return true;
            }
            if octets[0] == 169 && octets[1] == 254 {
                return true;
            }
            if octets[0] == 100 && (octets[1] >= 64 && octets[1] <= 127) {
                return true;
            }
            if octets[0] == 0 {
                return true;
            }
            false
        }
        IpAddr::V6(addr) => {
            if addr.is_loopback() {
                return true;
            }
            let segments = addr.segments();
            if (segments[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            if (segments[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_private_ip, parse_duration_string, parse_memory_string};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::time::Duration;

    #[test]
    fn parse_memory_string_supports_mb_and_gb() {
        assert_eq!(parse_memory_string("1MB"), Some(1024 * 1024));
        assert_eq!(parse_memory_string("2gb"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_memory_string(" 42 "), Some(42));
    }

    #[test]
    fn parse_memory_string_rejects_invalid_values() {
        assert_eq!(parse_memory_string(""), None);
        assert_eq!(parse_memory_string("abc"), None);
        assert_eq!(parse_memory_string("8TB"), None);
    }

    #[test]
    fn parse_duration_string_supports_ms_seconds_and_minutes() {
        assert_eq!(
            parse_duration_string("250ms"),
            Some(Duration::from_millis(250))
        );
        assert_eq!(parse_duration_string("3s"), Some(Duration::from_secs(3)));
        assert_eq!(parse_duration_string("2m"), Some(Duration::from_secs(120)));
        assert_eq!(parse_duration_string("9"), Some(Duration::from_secs(9)));
    }

    #[test]
    fn parse_duration_string_rejects_invalid_values() {
        assert_eq!(parse_duration_string(""), None);
        assert_eq!(parse_duration_string("3h"), None);
        assert_eq!(parse_duration_string("oops"), None);
    }

    #[test]
    fn is_private_ip_detects_ipv4_ranges() {
        let private = [
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(172, 16, 0, 5)),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 2, 2)),
            IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(0, 1, 2, 3)),
        ];
        for ip in private {
            assert!(is_private_ip(&ip), "expected private ip: {ip}");
        }

        let public = [
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            IpAddr::V4(Ipv4Addr::new(172, 15, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(172, 32, 0, 1)),
        ];
        for ip in public {
            assert!(!is_private_ip(&ip), "expected public ip: {ip}");
        }
    }

    #[test]
    fn is_private_ip_detects_ipv6_ranges() {
        let private = [
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            IpAddr::V6("fc00::1".parse().expect("valid")),
            IpAddr::V6("fd12:3456:789a::1".parse().expect("valid")),
            IpAddr::V6("fe80::1".parse().expect("valid")),
        ];
        for ip in private {
            assert!(is_private_ip(&ip), "expected private ip: {ip}");
        }

        let public = [
            IpAddr::V6("2001:4860:4860::8888".parse().expect("valid")),
            IpAddr::V6("2606:4700:4700::1111".parse().expect("valid")),
        ];
        for ip in public {
            assert!(!is_private_ip(&ip), "expected public ip: {ip}");
        }
    }
}
