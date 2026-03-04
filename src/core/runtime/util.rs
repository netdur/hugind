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
