mod memory;
mod notify;
mod policy;
use clap::Parser;
use memory::PsiRatePpm;
use policy::{Config, Policy, State, TriggerReason};
use std::{thread, time::Duration};

fn available_slope(previous: Option<u64>, current: u64, elapsed: Duration) -> u64 {
    let Some(previous) = previous else {
        return 0;
    };
    let elapsed_nanos = elapsed.as_nanos();
    if elapsed_nanos == 0 || elapsed > Duration::from_secs(300) {
        return 0;
    }
    let Some(decrease) = previous.checked_sub(current) else {
        return 0;
    };
    // decrease (bytes) * 1_000_000_000 (ns/s) / elapsed_nanos (ns) = bytes/sec
    // Using u128 arithmetic with rounding.
    let num = (decrease as u128).saturating_mul(1_000_000_000);
    let half = elapsed_nanos / 2;
    num.checked_add(half)
        .map(|sum| sum / elapsed_nanos)
        .and_then(|bps| u64::try_from(bps).ok())
        .unwrap_or(u64::MAX)
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

fn parse_percentage(value: &str) -> Result<PsiRatePpm, String> {
    let s = value.trim().strip_suffix('%').unwrap_or(value).trim();
    let pct: f64 = s
        .parse()
        .map_err(|_| format!("invalid percentage: {value:?}"))?;
    if pct <= 0.0 || pct > 100.0 || pct.is_nan() {
        return Err(format!(
            "percentage must be greater than 0 and at most 100, got {pct}"
        ));
    }
    Ok(PsiRatePpm::from_percent(pct))
}

#[derive(Parser, Clone, Debug)]
#[command(
    version,
    about = "Desktop notification daemon for pre-OOM memory pressure on Linux"
)]
struct Args {
    #[arg(
        long,
        default_value = "1",
        help = "Sampling interval in seconds (must be greater than 0)"
    )]
    interval: u64,

    #[arg(
        long,
        default_value = "3GiB",
        value_parser = parse_memory_size,
        help = "MemAvailable threshold for Warning alert"
    )]
    warning: u64,

    #[arg(
        long,
        default_value = "768MiB",
        value_parser = parse_memory_size,
        help = "MemAvailable threshold for Critical alert"
    )]
    critical: u64,

    #[arg(
        long,
        default_value = "256MiB",
        value_parser = parse_memory_size,
        help = "MemAvailable hysteresis above warning threshold required for recovery to Normal"
    )]
    hysteresis: u64,

    #[arg(
        long,
        default_value = "10",
        help = "Dwell time in seconds before entering Warning/Normal states"
    )]
    dwell: u64,

    #[arg(
        long,
        default_value = "10",
        value_parser = parse_percentage,
        help = "PSI some stall percentage for Warning alert sustained over dwell (e.g. 10 or 10%)"
    )]
    psi_some_warning: PsiRatePpm,

    #[arg(
        long,
        default_value = "5",
        value_parser = parse_percentage,
        help = "PSI full stall percentage for Critical alert sustained over dwell (e.g. 5 or 5%)"
    )]
    psi_full_critical: PsiRatePpm,

    #[arg(
        long,
        default_value = "1GiB",
        value_parser = parse_memory_size,
        help = "Decline rate (bytes/min) for Warning alert (e.g. 1GiB)"
    )]
    decline_warning: u64,

    #[arg(
        long,
        default_value = "6GiB",
        value_parser = parse_memory_size,
        help = "MemAvailable gate below which Warning decline rate triggers alert"
    )]
    decline_warning_gate: u64,

    #[arg(
        long,
        default_value = "2GiB",
        value_parser = parse_memory_size,
        help = "Decline rate (bytes/min) for Critical alert (e.g. 2GiB)"
    )]
    decline_critical: u64,

    #[arg(
        long,
        default_value = "4GiB",
        value_parser = parse_memory_size,
        help = "MemAvailable gate below which Critical decline rate triggers alert"
    )]
    decline_critical_gate: u64,

    #[arg(
        long,
        default_value = "300",
        help = "Warning reminder interval in seconds (default 5 minutes, must be greater than 0)"
    )]
    warning_repeat: u64,

    #[arg(
        long,
        default_value = "60",
        help = "Critical reminder interval in seconds (default 60 seconds, must be greater than 0)"
    )]
    critical_repeat: u64,

    #[arg(
        long,
        help = "Take a single diagnostic snapshot (rates unavailable) and exit immediately without alerting"
    )]
    once: bool,

    #[arg(
        long,
        short,
        help = "Verbose/diagnostic logging of every sample and policy evaluation"
    )]
    verbose: bool,
}

fn should_notify(
    current_state: State,
    previous_state: State,
    last_notify: Option<std::time::Instant>,
    now: std::time::Instant,
    warning_repeat_secs: u64,
    critical_repeat_secs: u64,
) -> bool {
    if current_state == State::Normal {
        return false;
    }
    // Immediate escalation from Warning to Critical
    if previous_state == State::Warning && current_state == State::Critical {
        return true;
    }
    let repeat = match current_state {
        State::Warning => warning_repeat_secs,
        State::Critical => critical_repeat_secs,
        State::Normal => 0,
    };
    last_notify.is_none_or(|t| {
        now.checked_duration_since(t)
            .is_some_and(|duration| duration.as_secs() >= repeat.max(1))
    })
}

#[cfg(test)]
mod tests {
    use super::{available_slope, parse_memory_size, parse_percentage};
    use crate::memory::PsiRatePpm;
    use std::time::Duration;

    #[test]
    fn slope_does_not_underflow_when_memory_increases() {
        assert_eq!(available_slope(Some(20), 30, Duration::from_secs(1)), 0);
        assert_eq!(available_slope(Some(30), 20, Duration::from_secs(2)), 5);
        assert_eq!(available_slope(None, 20, Duration::from_secs(1)), 0);
    }

    #[test]
    fn slope_boundary_and_nanosecond_precision() {
        // Zero elapsed
        assert_eq!(available_slope(Some(100), 50, Duration::ZERO), 0);
        // Elapsed > 300s
        assert_eq!(available_slope(Some(100), 50, Duration::from_secs(301)), 0);
        // Subsecond: 50 bytes decrease in 500ms (500_000_000 ns) -> 100 bytes/sec
        assert_eq!(
            available_slope(Some(100), 50, Duration::from_millis(500)),
            100
        );
        // Rounding: 1 byte decrease in 300_000_000 ns -> 1 * 1e9 / 3e8 = 3.33 -> 3 bytes/sec
        assert_eq!(
            available_slope(Some(10), 9, Duration::from_nanos(300_000_000)),
            3
        );
        // Rounding: 2 bytes decrease in 300_000_000 ns -> 2 * 1e9 / 3e8 = 6.67 -> 7 bytes/sec
        assert_eq!(
            available_slope(Some(10), 8, Duration::from_nanos(300_000_000)),
            7
        );
        // Large decrease: 10 GiB in 1 second
        let gib10 = 10 * 1024 * 1024 * 1024;
        assert_eq!(
            available_slope(Some(gib10), 0, Duration::from_secs(1)),
            gib10
        );
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

    #[test]
    fn parses_percentages_properly() {
        assert_eq!(parse_percentage("10"), Ok(PsiRatePpm(100_000)));
        assert_eq!(parse_percentage("10%"), Ok(PsiRatePpm(100_000)));
        assert_eq!(parse_percentage("5"), Ok(PsiRatePpm(50_000)));
        assert_eq!(parse_percentage("0.01%"), Ok(PsiRatePpm(100)));
        assert_eq!(parse_percentage("100"), Ok(PsiRatePpm(1_000_000)));
        assert!(parse_percentage("0").is_err());
        assert!(parse_percentage("0.0").is_err());
        assert!(parse_percentage("-5").is_err());
        assert!(parse_percentage("101").is_err());
        assert!(parse_percentage("abc").is_err());
    }

    #[test]
    fn reminder_and_escalation_timing() {
        use super::should_notify;
        use crate::policy::State;
        use std::time::Instant;

        let t0 = Instant::now();
        let t_59s = t0 + Duration::from_secs(59);
        let t_60s = t0 + Duration::from_secs(60);
        let t_300s = t0 + Duration::from_secs(300);

        // Normal state should never notify
        assert!(!should_notify(
            State::Normal,
            State::Normal,
            None,
            t0,
            300,
            60
        ));

        // First notification in Warning (last_notify is None)
        assert!(should_notify(
            State::Warning,
            State::Normal,
            None,
            t0,
            300,
            60
        ));

        // Warning reminder: not due at 59s or 299s, due at 300s
        assert!(!should_notify(
            State::Warning,
            State::Warning,
            Some(t0),
            t_59s,
            300,
            60
        ));
        assert!(should_notify(
            State::Warning,
            State::Warning,
            Some(t0),
            t_300s,
            300,
            60
        ));

        // Immediate escalation from Warning to Critical even if 0s since last warning
        assert!(should_notify(
            State::Critical,
            State::Warning,
            Some(t0),
            t0,
            300,
            60
        ));

        // Critical reminder: not due at 59s, due at 60s
        assert!(!should_notify(
            State::Critical,
            State::Critical,
            Some(t0),
            t_59s,
            300,
            60
        ));
        assert!(should_notify(
            State::Critical,
            State::Critical,
            Some(t0),
            t_60s,
            300,
            60
        ));
    }
    #[test]
    fn cli_validation_checks() {
        use super::{validate_args, Args};

        let valid_args = Args {
            interval: 1,
            warning: 3 * 1024 * 1024 * 1024,
            critical: 768 * 1024 * 1024,
            hysteresis: 256 * 1024 * 1024,
            dwell: 10,
            psi_some_warning: PsiRatePpm(100_000),
            psi_full_critical: PsiRatePpm(50_000),
            decline_warning: 1024 * 1024 * 1024,
            decline_warning_gate: 6 * 1024 * 1024 * 1024,
            decline_critical: 2 * 1024 * 1024 * 1024,
            decline_critical_gate: 4 * 1024 * 1024 * 1024,
            warning_repeat: 300,
            critical_repeat: 60,
            once: false,
            verbose: false,
        };
        assert!(validate_args(&valid_args).is_ok());

        // critical >= warning
        let mut bad = valid_args.clone();
        bad.critical = bad.warning;
        assert!(validate_args(&bad).is_err());

        // interval == 0
        let mut bad = valid_args.clone();
        bad.interval = 0;
        assert!(validate_args(&bad).is_err());

        // hysteresis == 0
        let mut bad = valid_args.clone();
        bad.hysteresis = 0;
        assert!(validate_args(&bad).is_err());

        // dwell == 0
        let mut bad = valid_args.clone();
        bad.dwell = 0;
        assert!(validate_args(&bad).is_err());

        // decline rates == 0
        let mut bad = valid_args.clone();
        bad.decline_warning = 0;
        assert!(validate_args(&bad).is_err());
        let mut bad = valid_args.clone();
        bad.decline_critical = 0;
        assert!(validate_args(&bad).is_err());

        // decline_critical <= decline_warning
        let mut bad = valid_args.clone();
        bad.decline_critical = bad.decline_warning;
        assert!(validate_args(&bad).is_err());

        // repeats == 0
        let mut bad = valid_args.clone();
        bad.warning_repeat = 0;
        assert!(validate_args(&bad).is_err());
        let mut bad = valid_args.clone();
        bad.critical_repeat = 0;
        assert!(validate_args(&bad).is_err());

        // warning + hysteresis overflow
        let mut bad = valid_args.clone();
        bad.warning = u64::MAX;
        bad.hysteresis = 1;
        assert!(validate_args(&bad).is_err());

        // decline critical gate > decline warning gate
        let mut bad = valid_args.clone();
        bad.decline_critical_gate = bad.decline_warning_gate + 1;
        assert!(validate_args(&bad).is_err());
    }

    #[test]
    fn format_notification_body_cases() {
        use super::format_notification_body;
        use crate::policy::TriggerReason;

        // With no reasons
        let body = format_notification_body(1024 * 1024 * 1024, 0, None, None, &[]);
        assert_eq!(
            body,
            "MemAvailable: 1024 MiB\nSlope: 0 MiB/min\nPSI some: n/a\nPSI full: n/a"
        );

        // With active reasons (e.g. LowAvailable and Recovering)
        let reasons = vec![
            TriggerReason::LowAvailable {
                current: 500 * 1024 * 1024,
                threshold: 768 * 1024 * 1024,
            },
            TriggerReason::Recovering {
                detail: "PSI full 4.0% >= exit threshold 2.5%".to_string(),
            },
        ];
        let body = format_notification_body(
            500 * 1024 * 1024,
            (100 * 1024 * 1024 + 59) / 60,
            Some(PsiRatePpm(100_000)),
            Some(PsiRatePpm(40_000)),
            &reasons,
        );
        let expected = "Triggers:\n• MemAvailable low: 500 MiB\n• recovering (PSI full 4.0% >= exit threshold 2.5%)\n\nMemAvailable: 500 MiB\nSlope: 100 MiB/min\nPSI some: 10.0%\nPSI full: 4.0%";
        assert_eq!(body, expected);
    }
}

fn validate_args(a: &Args) -> Result<(), &'static str> {
    if a.critical >= a.warning {
        return Err("--critical must be less than --warning");
    }
    if a.warning.checked_add(a.hysteresis).is_none() {
        return Err("--warning + --hysteresis overflows 64-bit integer");
    }
    if a.interval == 0 {
        return Err("--interval must be greater than zero");
    }
    if a.hysteresis == 0 || a.dwell == 0 {
        return Err("--hysteresis and --dwell must be greater than zero");
    }
    if a.decline_warning == 0 || a.decline_critical == 0 {
        return Err("--decline-warning and --decline-critical must be greater than zero");
    }
    if a.decline_critical <= a.decline_warning {
        return Err("--decline-critical must be greater than --decline-warning");
    }
    if a.warning_repeat == 0 || a.critical_repeat == 0 {
        return Err("--warning-repeat and --critical-repeat must be greater than zero");
    }
    if a.decline_critical_gate > a.decline_warning_gate {
        return Err("--decline-critical-gate must not be greater than --decline-warning-gate");
    }
    Ok(())
}

fn format_notification_body(
    available: u64,
    slope_per_sec: u64,
    psi_some: Option<PsiRatePpm>,
    psi_full: Option<PsiRatePpm>,
    reasons: &[TriggerReason],
) -> String {
    let slope_per_min = slope_per_sec.saturating_mul(60);
    let psi_some_str = psi_some.map_or("n/a".to_string(), |p| p.to_string());
    let psi_full_str = psi_full.map_or("n/a".to_string(), |p| p.to_string());

    let reasons_str = if reasons.is_empty() {
        String::new()
    } else {
        let joined = reasons
            .iter()
            .map(|r| format!("• {r}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("Triggers:\n{joined}\n\n")
    };

    format!(
        "{reasons_str}MemAvailable: {} MiB\nSlope: {} MiB/min\nPSI some: {}\nPSI full: {}",
        available / (1024 * 1024),
        slope_per_min / (1024 * 1024),
        psi_some_str,
        psi_full_str
    )
}

fn main() {
    let a = Args::parse();
    if let Err(msg) = validate_args(&a) {
        eprintln!("error: {msg}");
        std::process::exit(2);
    }

    let c = Config {
        warning_available: a.warning,
        critical_available: a.critical,
        hysteresis: a.hysteresis,
        dwell: a.dwell,
        psi_some_warning: a.psi_some_warning,
        psi_full_critical: a.psi_full_critical,
        decline_warning_rate: a.decline_warning,
        decline_warning_gate: a.decline_warning_gate,
        decline_critical_rate: a.decline_critical,
        decline_critical_gate: a.decline_critical_gate,
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
                    .unwrap_or(Duration::ZERO);

                let slope =
                    available_slope(prev.map(|(available, _)| available), s.available, elapsed);
                let psi_some = memory::calculate_psi_rate_ppm(prev_psi_some, s.psi_some, elapsed);
                let psi_full = memory::calculate_psi_rate_ppm(prev_psi_full, s.psi_full, elapsed);

                if a.once {
                    println!(
                        "MemAvailable: {} MiB (total: {} MiB, swap_used: {} MiB)",
                        s.available / (1024 * 1024),
                        s.total / (1024 * 1024),
                        s.swap_total.saturating_sub(s.swap_free) / (1024 * 1024)
                    );
                    println!(
                        "PSI some: {} (total: {} ms)",
                        psi_some.map_or("n/a (first sample, no rate delta)".to_string(), |p| p
                            .to_string()),
                        s.psi_some
                            .map_or("n/a".to_string(), |x| (x.total_us / 1000).to_string())
                    );
                    println!(
                        "PSI full: {} (total: {} ms)",
                        psi_full.map_or("n/a (first sample, no rate delta)".to_string(), |p| p
                            .to_string()),
                        s.psi_full
                            .map_or("n/a".to_string(), |x| (x.total_us / 1000).to_string())
                    );
                    let eval = p.update(
                        s.available,
                        slope,
                        psi_some,
                        psi_full,
                        start.elapsed().as_secs(),
                        c,
                    );
                    println!("State: {:?}", eval.state);
                    break;
                }

                let previous_state = p.state();
                let eval = p.update(
                    s.available,
                    slope,
                    psi_some,
                    psi_full,
                    start.elapsed().as_secs(),
                    c,
                );
                let st = eval.state;

                if a.verbose {
                    let psi_some_str = psi_some.map_or("n/a".to_string(), |p| p.to_string());
                    let psi_full_str = psi_full.map_or("n/a".to_string(), |p| p.to_string());
                    let swap_used = s.swap_total.saturating_sub(s.swap_free);
                    let reasons_str = if eval.active_reasons.is_empty() {
                        if eval.candidate_reasons.is_empty() {
                            "none".to_string()
                        } else {
                            format!(
                                "candidate: {}",
                                eval.candidate_reasons
                                    .iter()
                                    .map(|r| r.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        }
                    } else {
                        eval.active_reasons
                            .iter()
                            .map(|r| r.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    println!(
                        "[sample] available={} MiB (swap_used={} MiB/{} MiB), slope={} MiB/min, psi_some={}, psi_full={}, candidate={:?}, state={:?}, reasons=[{}]",
                        s.available / (1024 * 1024),
                        swap_used / (1024 * 1024),
                        s.swap_total / (1024 * 1024),
                        slope.saturating_mul(60) / (1024 * 1024),
                        psi_some_str,
                        psi_full_str,
                        eval.candidate,
                        st,
                        reasons_str
                    );
                }

                if st != previous_state {
                    let reason_desc = if eval.active_reasons.is_empty() {
                        "recovered".to_string()
                    } else {
                        eval.active_reasons
                            .iter()
                            .map(|r| r.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    println!(
                        "State transition: {:?} -> {:?} ({}) [MemAvailable: {} MiB]",
                        previous_state,
                        st,
                        reason_desc,
                        s.available / (1024 * 1024)
                    );
                }

                let is_repeat_due = should_notify(
                    st,
                    previous_state,
                    last_notify,
                    now_instant,
                    a.warning_repeat,
                    a.critical_repeat,
                );

                if is_repeat_due {
                    let primary_reason = eval.active_reasons.first().map(|r| r.to_string());
                    let summary = match primary_reason {
                        Some(reason) => format!("{st:?}: {reason}"),
                        None => format!("{st:?}"),
                    };
                    let body = format_notification_body(
                        s.available,
                        slope,
                        psi_some,
                        psi_full,
                        &eval.active_reasons,
                    );
                    if let Err(e) = notifier.send(&summary, &body) {
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
