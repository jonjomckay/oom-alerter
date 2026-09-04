use std::{fs, io, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct PsiRatePpm(pub u32);

impl PsiRatePpm {
    pub const fn from_ppm(ppm: u32) -> Self {
        if ppm > 1_000_000 {
            Self(1_000_000)
        } else {
            Self(ppm)
        }
    }

    pub fn from_percent(pct: f64) -> Self {
        let ppm = (pct * 10_000.0).round();
        Self((ppm as u64).clamp(0, 1_000_000) as u32)
    }

    pub const fn as_ppm(self) -> u32 {
        self.0
    }

    pub fn to_percent_f64(self) -> f64 {
        self.0 as f64 / 10_000.0
    }
}

impl std::fmt::Display for PsiRatePpm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pct = self.to_percent_f64();
        if pct > 0.0 && pct < 0.1 {
            write!(f, "{pct:.2}%")
        } else {
            write!(f, "{pct:.1}%")
        }
    }
}

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

pub fn calculate_psi_rate_ppm(
    previous: Option<Psi>,
    current: Option<Psi>,
    elapsed: Duration,
) -> Option<PsiRatePpm> {
    let (Some(prev), Some(curr)) = (previous, current) else {
        return None;
    };
    let elapsed_nanos = elapsed.as_nanos();
    if elapsed_nanos == 0 || elapsed > Duration::from_secs(300) || curr.total_us < prev.total_us {
        return None;
    }
    let delta_us = (curr.total_us - prev.total_us) as u128;
    // ppm = delta_us * 1_000_000_000 / elapsed_nanos, rounded and clamped to 1_000_000.
    // u128 arithmetic avoids overflow for any valid delta_us.
    let num = delta_us.saturating_mul(1_000_000_000);
    let half = elapsed_nanos / 2;
    let ppm = num
        .checked_add(half)
        .map(|sum| sum / elapsed_nanos)
        .unwrap_or(1_000_000);
    let clamped_ppm = ppm.min(1_000_000) as u32;
    Some(PsiRatePpm(clamped_ppm))
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
    use super::{calculate_psi_rate_ppm, parse_meminfo, Psi, PsiRatePpm};
    use std::io::ErrorKind;
    use std::time::Duration;

    #[test]
    fn kib_is_bytes() {
        assert_eq!("3".parse::<u64>().unwrap() * 1024, 3072);
    }

    #[test]
    fn psi_rate_ppm_semantic_cases() {
        let prev = Psi {
            avg10: 0,
            avg60: 0,
            avg300: 0,
            total_us: 1_000_000,
        };

        // 0.01% of 1 second = 100 us stall -> 100 ppm
        let curr_0_01 = Psi {
            total_us: 1_000_100,
            ..prev
        };
        assert_eq!(
            calculate_psi_rate_ppm(Some(prev), Some(curr_0_01), Duration::from_secs(1)),
            Some(PsiRatePpm(100))
        );
        assert_eq!(PsiRatePpm(100).to_percent_f64(), 0.01);

        // 1% of 1 second = 10_000 us stall -> 10_000 ppm
        let curr_1 = Psi {
            total_us: 1_010_000,
            ..prev
        };
        assert_eq!(
            calculate_psi_rate_ppm(Some(prev), Some(curr_1), Duration::from_secs(1)),
            Some(PsiRatePpm(10_000))
        );
        assert_eq!(PsiRatePpm(10_000).to_percent_f64(), 1.0);

        // 5% of 1 second = 50_000 us stall -> 50_000 ppm
        let curr_5 = Psi {
            total_us: 1_050_000,
            ..prev
        };
        assert_eq!(
            calculate_psi_rate_ppm(Some(prev), Some(curr_5), Duration::from_secs(1)),
            Some(PsiRatePpm(50_000))
        );
        assert_eq!(PsiRatePpm(50_000).to_percent_f64(), 5.0);

        // 10% of 1 second = 100_000 us stall -> 100_000 ppm
        let curr_10 = Psi {
            total_us: 1_100_000,
            ..prev
        };
        assert_eq!(
            calculate_psi_rate_ppm(Some(prev), Some(curr_10), Duration::from_secs(1)),
            Some(PsiRatePpm(100_000))
        );
        assert_eq!(PsiRatePpm(100_000).to_percent_f64(), 10.0);

        // 100% of 1 second = 1_000_000 us stall -> 1_000_000 ppm
        let curr_100 = Psi {
            total_us: 2_000_000,
            ..prev
        };
        assert_eq!(
            calculate_psi_rate_ppm(Some(prev), Some(curr_100), Duration::from_secs(1)),
            Some(PsiRatePpm(1_000_000))
        );
        assert_eq!(PsiRatePpm(1_000_000).to_percent_f64(), 100.0);

        // Jitter > 100% saturates to 1_000_000 ppm
        let curr_over = Psi {
            total_us: 2_500_000,
            ..prev
        };
        assert_eq!(
            calculate_psi_rate_ppm(Some(prev), Some(curr_over), Duration::from_secs(1)),
            Some(PsiRatePpm(1_000_000))
        );

        // Test with subsecond duration: 50_000 us stall over 500 ms = 100_000 ppm (10%)
        assert_eq!(
            calculate_psi_rate_ppm(Some(prev), Some(curr_5), Duration::from_millis(500)),
            Some(PsiRatePpm(100_000))
        );

        // Nanosecond resolution test: 1 us stall over 10_000_000 ns (10 ms) -> 100 ppm (0.01%)
        let curr_1us = Psi {
            total_us: 1_000_001,
            ..prev
        };
        assert_eq!(
            calculate_psi_rate_ppm(Some(prev), Some(curr_1us), Duration::from_nanos(10_000_000)),
            Some(PsiRatePpm(100))
        );
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
        assert_eq!(
            calculate_psi_rate_ppm(Some(a), Some(b), Duration::from_secs(1)),
            None
        );
        assert_eq!(
            calculate_psi_rate_ppm(Some(a), Some(a), Duration::from_secs(301)),
            None
        );
        assert_eq!(
            calculate_psi_rate_ppm(Some(a), Some(a), Duration::from_secs(0)),
            None
        );
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

        // 150 us over 1 second = 150 ppm (0.015%)
        assert_eq!(
            calculate_psi_rate_ppm(Some(previous), Some(current), Duration::from_secs(1)),
            Some(PsiRatePpm(150))
        );
        assert_eq!(
            calculate_psi_rate_ppm(None, None, Duration::from_secs(1)),
            None
        );
        assert_eq!(
            calculate_psi_rate_ppm(None, Some(current), Duration::from_secs(1)),
            None
        );
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
