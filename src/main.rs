mod memory;
mod notify;
mod policy;
use clap::Parser;
use policy::{Config, Policy, State};
use std::{thread, time::Duration};

fn available_slope(previous: Option<u64>, current: u64, elapsed: u64) -> u64 {
    let Some(previous) = previous else {
        return 0;
    };
    if elapsed == 0 || elapsed > 300 {
        return 0;
    }
    previous
        .checked_sub(current)
        .map(|decrease| decrease / elapsed)
        .unwrap_or(0)
}

fn parse_memory_size(value: &str) -> Result<u64, String> {
    let value = value.trim();
    let split = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    if split == 0 {
        return Err("expected a positive byte value or binary size such as 768M or 3GiB".into());
    }
    let number: u64 = value[..split]
        .parse()
        .map_err(|_| "size number is too large".to_string())?;
    let suffix = value[split..].to_ascii_lowercase();
    let multiplier = match suffix.as_str() {
        "" => 1,
        "k" | "ki" | "kib" => 1024,
        "m" | "mi" | "mib" => 1024_u64.pow(2),
        "g" | "gi" | "gib" => 1024_u64.pow(3),
        _ => {
            return Err(format!(
                "unknown size suffix {suffix:?}; use K, M, G, KiB, MiB, or GiB"
            ))
        }
    };
    number
        .checked_mul(multiplier)
        .filter(|&size| size > 0)
        .ok_or_else(|| "size must be greater than zero and fit in an unsigned 64-bit value".into())
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value_t = 1)]
    #[arg(value_parser = parse_memory_size)]
    interval: u64,
    #[arg(long, default_value_t = 3 * 1024 * 1024 * 1024)]
    #[arg(value_parser = parse_memory_size)]
    warning: u64,
    #[arg(long, default_value_t = 768 * 1024 * 1024)]
    #[arg(value_parser = parse_memory_size)]
    critical: u64,
    #[arg(long, default_value_t = 256 * 1024 * 1024)]
    #[arg(value_parser = clap::value_parser!(u64).range(1..))]
    hysteresis: u64,
    #[arg(long, default_value_t = 10)]
    dwell: u64,
    #[arg(long, default_value_t = 60)]
    warning_repeat: u64,
    #[arg(long, default_value_t = 15, value_parser = clap::value_parser!(u64).range(1..))]
    critical_repeat: u64,
    #[arg(long)]
    once: bool,
}

#[cfg(test)]
mod tests {
    use super::{available_slope, parse_memory_size};

    #[test]
    fn slope_does_not_underflow_when_memory_increases() {
        // Keep this regression test for the former `then_some` eager
        // evaluation panic in the runtime loop.
        assert_eq!(available_slope(Some(20), 30, 1), 0);
        assert_eq!(available_slope(Some(30), 20, 2), 5);
        assert_eq!(available_slope(None, 20, 1), 0);
    }

    #[test]
    fn parses_binary_sizes_and_bytes() {
        assert_eq!(parse_memory_size("20G"), Ok(20 * 1024_u64.pow(3)));
        assert_eq!(parse_memory_size("20GiB"), Ok(20 * 1024_u64.pow(3)));
        assert_eq!(parse_memory_size("768M"), Ok(768 * 1024_u64.pow(2)));
        assert_eq!(parse_memory_size("3GiB"), Ok(3 * 1024_u64.pow(3)));
        assert_eq!(parse_memory_size("4096"), Ok(4096));
        assert_eq!(parse_memory_size("2mib"), Ok(2 * 1024_u64.pow(2)));
    }

    #[test]
    fn rejects_invalid_or_overflowing_sizes() {
        assert!(parse_memory_size("0").is_err());
        assert!(parse_memory_size("12GB").is_err());
        assert!(parse_memory_size("abc").is_err());
        assert!(parse_memory_size("18446744073709551615G").is_err());
        assert!(parse_memory_size("1.5G").is_err());
    }
}
fn main() {
    let a = Args::parse();
    if a.critical >= a.warning {
        eprintln!("error: --critical must be less than --warning");
        std::process::exit(2);
    }
    if a.hysteresis == 0 || a.dwell == 0 {
        eprintln!("error: --hysteresis and --dwell must be greater than zero");
        std::process::exit(2);
    }
    let c = Config {
        warning: a.warning,
        critical: a.critical,
        hysteresis: a.hysteresis,
        dwell: a.dwell,
    };
    let mut p = Policy::new();
    let mut prev: Option<(u64, std::time::Instant)> = None;
    let mut prev_psi_some: Option<memory::Psi> = None;
    let mut prev_psi_full: Option<memory::Psi> = None;
    let mut last_notify: Option<std::time::Instant> = None;
    let mut notifier = notify::Notifier::new();
    let start = std::time::Instant::now();
    loop {
        match memory::read() {
            Ok(s) => {
                let now_instant = std::time::Instant::now();
                let elapsed = prev
                    .and_then(|(_, t)| now_instant.checked_duration_since(t))
                    .map_or(0, |duration| duration.as_secs());
                let slope =
                    available_slope(prev.map(|(available, _)| available), s.available, elapsed);
                let psi_some = memory::psi_delta(prev_psi_some, s.psi_some, elapsed).unwrap_or(0);
                let psi_full = memory::psi_delta(prev_psi_full, s.psi_full, elapsed).unwrap_or(0);
                let previous_state = p.state();
                let st = p.update(
                    s.available,
                    slope,
                    psi_some,
                    psi_full,
                    start.elapsed().as_secs(),
                    c,
                );
                if a.once {
                    println!(
                    "available={} total={} swap_used={} psi_some_ms={:?} psi_full_ms={:?} state={st:?}",
                    s.available,
                    s.total,
                    s.swap_total.saturating_sub(s.swap_free),
                    s.psi_some.map(|x| x.total_us / 1000),
                    s.psi_full.map(|x| x.total_us / 1000)
                );
                    break;
                }
                let repeat = match st {
                    State::Warning => a.warning_repeat,
                    State::Critical => a.critical_repeat,
                    _ => 0,
                };
                if st != State::Normal
                    && (last_notify.is_none_or(|t| {
                        previous_state == State::Warning && st == State::Critical
                            || now_instant
                                .checked_duration_since(t)
                                .is_some_and(|duration| duration.as_secs() >= repeat.max(1))
                    }))
                {
                    if let Err(e) = notifier.send(
                        &format!("{st:?}"),
                        &format!("MemAvailable: {} MiB", s.available / 1024 / 1024),
                    ) {
                        eprintln!("notification failed: {e:#}");
                    } else {
                        last_notify = Some(now_instant);
                    }
                }
                if st == State::Normal {
                    last_notify = None;
                    if let Err(e) = notifier.close() {
                        eprintln!("notification close failed: {e:#}");
                    }
                }
                prev = Some((s.available, now_instant));
                prev_psi_some = s.psi_some;
                prev_psi_full = s.psi_full;
            }
            Err(e) => eprintln!("memory read failed: {e}"),
        }
        thread::sleep(Duration::from_secs(a.interval));
    }
}
