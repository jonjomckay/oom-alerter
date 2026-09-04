use std::{fs, io};

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub available: u64,
    pub total: u64,
    pub swap_total: u64,
    pub swap_free: u64,
    pub psi_some: Option<Psi>,
    pub psi_full: Option<Psi>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Psi {
    pub avg10: u64,
    pub avg60: u64,
    pub avg300: u64,
    pub total_us: u64,
}

fn parse_meminfo(text: &str) -> io::Result<(u64, u64, u64, u64)> {
    let mut v = [0; 4];
    let mut seen = [false; 4];
    for line in text.lines() {
        let mut p = line.split_whitespace();
        let key = p.next().unwrap_or("");
        let Some(index) = (match key {
            "MemAvailable:" => Some(0),
            "MemTotal:" => Some(1),
            "SwapTotal:" => Some(2),
            "SwapFree:" => Some(3),
            _ => None,
        }) else {
            continue;
        };
        let value = p
            .next()
            .and_then(|x| x.parse::<u64>().ok())
            .and_then(|x| x.checked_mul(1024));
        if seen[index] || value.is_none() || p.next() != Some("kB") || p.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed meminfo field",
            ));
        }
        v[index] = value.unwrap();
        seen[index] = true;
    }
    if seen.iter().any(|x| !x) || v[0] == 0 || v[1] == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing required memory fields",
        ));
    }
    Ok((v[0], v[1], v[2], v[3]))
}
fn meminfo() -> io::Result<(u64, u64, u64, u64)> {
    parse_meminfo(&fs::read_to_string("/proc/meminfo")?)
}
fn psi() -> (Option<Psi>, Option<Psi>) {
    let Ok(text) = fs::read_to_string("/proc/pressure/memory") else {
        return (None, None);
    };
    let mut out = [None, None];
    for line in text.lines() {
        let mut p = line.split_whitespace();
        let kind = p.next().unwrap_or("");
        let values: Vec<_> = p.collect();
        let parse = |name: &str| {
            values
                .iter()
                .find_map(|x| x.strip_prefix(name)?.parse().ok())
        };
        if let Some(total_us) = parse("total=") {
            let value = Psi {
                avg10: parse("avg10=").unwrap_or(0),
                avg60: parse("avg60=").unwrap_or(0),
                avg300: parse("avg300=").unwrap_or(0),
                total_us,
            };
            if kind == "some" {
                out[0] = Some(value);
            } else if kind == "full" {
                out[1] = Some(value);
            }
        }
    }
    (out[0], out[1])
}

pub fn psi_delta(previous: Option<Psi>, current: Option<Psi>, elapsed_secs: u64) -> Option<u64> {
    let (Some(a), Some(b)) = (previous, current) else {
        return None;
    };
    if elapsed_secs == 0 || elapsed_secs > 300 || b.total_us < a.total_us {
        return None;
    }
    Some((b.total_us - a.total_us).saturating_mul(1000) / elapsed_secs)
}
pub fn read() -> io::Result<Snapshot> {
    let (a, t, st, sf) = meminfo()?;
    let (some, full) = psi();
    Ok(Snapshot {
        available: a,
        total: t,
        swap_total: st,
        swap_free: sf,
        psi_some: some,
        psi_full: full,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_meminfo, psi_delta, Psi};
    use std::io::ErrorKind;
    #[test]
    fn kib_is_bytes() {
        assert_eq!("3".parse::<u64>().unwrap() * 1024, 3072);
    }
    #[test]
    fn psi_counter_reset_and_long_gap_are_ignored() {
        let a = Psi {
            avg10: 0,
            avg60: 0,
            avg300: 0,
            total_us: 100,
        };
        let b = Psi { total_us: 50, ..a };
        assert_eq!(psi_delta(Some(a), Some(b), 1), None);
        assert_eq!(psi_delta(Some(a), Some(a), 301), None);
    }
    #[test]
    fn psi_streams_delta_independently_when_other_stream_is_missing() {
        let previous = Psi {
            avg10: 0,
            avg60: 0,
            avg300: 0,
            total_us: 100,
        };
        let current = Psi {
            total_us: 250,
            ..previous
        };

        assert_eq!(psi_delta(Some(previous), Some(current), 1), Some(150_000));
        assert_eq!(psi_delta(None, None, 1), None);
        assert_eq!(psi_delta(None, Some(current), 1), None);
    }
    #[test]
    fn required_meminfo_fields_are_strict() {
        let text = "MemTotal: 10 kB\nMemAvailable: 5 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n";
        assert_eq!(parse_meminfo(text).unwrap().0, 5120);
        assert_eq!(
            parse_meminfo("MemTotal: nope kB\n").unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert!(parse_meminfo("MemAvailable: 5 kB\n").is_err());
        assert!(parse_meminfo(
            "MemTotal: 10 kB extra\nMemAvailable: 5 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n"
        )
        .is_err());
        assert!(parse_meminfo("MemTotal: 10 kB\nMemTotal: 10 kB\nMemAvailable: 5 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n").is_err());
    }
}
