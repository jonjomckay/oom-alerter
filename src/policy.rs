use crate::memory::PsiRatePpm;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Normal,
    Warning,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TriggerReason {
    LowAvailable {
        current: u64,
        threshold: u64,
    },
    RapidDecline {
        slope_per_min: u64,
        threshold_rate: u64,
        available: u64,
        gate: u64,
    },
    PsiSomePressure {
        rate: PsiRatePpm,
        threshold: PsiRatePpm,
    },
    PsiFullPressure {
        rate: PsiRatePpm,
        threshold: PsiRatePpm,
    },
    Recovering {
        detail: String,
    },
}

impl fmt::Display for TriggerReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LowAvailable { current, .. } => {
                write!(f, "MemAvailable low: {} MiB", current / (1024 * 1024))
            }
            Self::RapidDecline { slope_per_min, .. } => {
                write!(
                    f,
                    "rapid decline: {} MiB/min",
                    slope_per_min / (1024 * 1024)
                )
            }
            Self::PsiSomePressure { rate, .. } => {
                write!(f, "PSI some {rate}")
            }
            Self::PsiFullPressure { rate, .. } => {
                write!(f, "PSI full {rate}")
            }
            Self::Recovering { detail } => {
                write!(f, "recovering ({detail})")
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub warning_available: u64,
    pub critical_available: u64,
    pub hysteresis: u64,
    pub dwell: u64,
    pub psi_some_warning: PsiRatePpm,
    pub psi_full_critical: PsiRatePpm,
    pub decline_warning_rate: u64,
    pub decline_warning_gate: u64,
    pub decline_critical_rate: u64,
    pub decline_critical_gate: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evaluation {
    pub state: State,
    pub active_reasons: Vec<TriggerReason>,
    pub candidate: State,
    pub candidate_reasons: Vec<TriggerReason>,
}

pub struct Policy {
    state: State,
    candidate: State,
    candidate_reasons: Vec<TriggerReason>,
    active_reasons: Vec<TriggerReason>,
    since: u64,
}

impl Policy {
    pub fn new() -> Self {
        Self {
            state: State::Normal,
            candidate: State::Normal,
            candidate_reasons: Vec::new(),
            active_reasons: Vec::new(),
            since: 0,
        }
    }

    /// Evaluates current metrics against alert thresholds and returns the active triggers
    /// for Critical and Warning levels.
    ///
    /// Slope values passed here are in bytes per second (`bytes_per_sec`).
    /// The slope decline rates in `Config` are configured as bytes per minute (`bytes_per_min`).
    /// Therefore, slope decline rate comparison compares `slope_per_min` (slope * 60) against `rate`.
    pub fn evaluate_triggers(
        available: u64,
        slope_per_sec: u64,
        psi_some: Option<PsiRatePpm>,
        psi_full: Option<PsiRatePpm>,
        c: &Config,
    ) -> (Vec<TriggerReason>, Vec<TriggerReason>) {
        let slope_per_min = slope_per_sec.saturating_mul(60);
        let mut critical_reasons = Vec::new();

        if available <= c.critical_available {
            critical_reasons.push(TriggerReason::LowAvailable {
                current: available,
                threshold: c.critical_available,
            });
        }

        if available < c.decline_critical_gate && slope_per_min >= c.decline_critical_rate {
            critical_reasons.push(TriggerReason::RapidDecline {
                slope_per_min,
                threshold_rate: c.decline_critical_rate,
                available,
                gate: c.decline_critical_gate,
            });
        }

        if let Some(full) = psi_full {
            if full >= c.psi_full_critical {
                critical_reasons.push(TriggerReason::PsiFullPressure {
                    rate: full,
                    threshold: c.psi_full_critical,
                });
            }
        }

        let mut warning_reasons = Vec::new();

        if available <= c.warning_available {
            warning_reasons.push(TriggerReason::LowAvailable {
                current: available,
                threshold: c.warning_available,
            });
        }

        if available < c.decline_warning_gate && slope_per_min >= c.decline_warning_rate {
            warning_reasons.push(TriggerReason::RapidDecline {
                slope_per_min,
                threshold_rate: c.decline_warning_rate,
                available,
                gate: c.decline_warning_gate,
            });
        }

        if let Some(some) = psi_some {
            if some >= c.psi_some_warning {
                warning_reasons.push(TriggerReason::PsiSomePressure {
                    rate: some,
                    threshold: c.psi_some_warning,
                });
            }
        }

        (critical_reasons, warning_reasons)
    }

    /// Evaluates recovery conditions from Warning or Critical to Normal.
    ///
    /// Returns a list of recovery blockers (if any). If the returned list is empty,
    /// all recovery conditions are met.
    ///
    /// For Normal recovery:
    /// - Available memory must exceed `warning_available + hysteresis`.
    /// - Decline slope must be below warning decline rate (or available memory above warning gate).
    /// - PSI some must be below PSI recovery threshold (half of warning threshold).
    /// - PSI full must be below PSI critical recovery threshold (half of full critical threshold).
    pub fn recovery_blockers(
        available: u64,
        slope_per_sec: u64,
        psi_some: Option<PsiRatePpm>,
        psi_full: Option<PsiRatePpm>,
        c: &Config,
    ) -> Vec<TriggerReason> {
        let mut blockers = Vec::new();
        let slope_per_min = slope_per_sec.saturating_mul(60);
        let normal_available_threshold = c.warning_available.saturating_add(c.hysteresis);
        if available < normal_available_threshold {
            blockers.push(TriggerReason::Recovering {
                detail: format!(
                    "MemAvailable {} MiB < recovery threshold {} MiB",
                    available / (1024 * 1024),
                    normal_available_threshold / (1024 * 1024)
                ),
            });
        }

        // Decline slope must not be actively triggering decline warning
        if available < c.decline_warning_gate && slope_per_min >= c.decline_warning_rate {
            blockers.push(TriggerReason::RapidDecline {
                slope_per_min,
                threshold_rate: c.decline_warning_rate,
                available,
                gate: c.decline_warning_gate,
            });
        }

        // PSI some hysteresis: must be below half of warning threshold
        let psi_some_exit = PsiRatePpm::from_ppm(c.psi_some_warning.as_ppm() / 2);
        if let Some(some) = psi_some {
            if some >= psi_some_exit {
                blockers.push(TriggerReason::Recovering {
                    detail: format!("PSI some {some} >= exit threshold {psi_some_exit}"),
                });
            }
        }

        // PSI full hysteresis: must be below half of critical threshold
        let psi_full_exit = PsiRatePpm::from_ppm(c.psi_full_critical.as_ppm() / 2);
        if let Some(full) = psi_full {
            if full >= psi_full_exit {
                blockers.push(TriggerReason::Recovering {
                    detail: format!("PSI full {full} >= exit threshold {psi_full_exit}"),
                });
            }
        }

        blockers
    }

    pub fn update(
        &mut self,
        available: u64,
        slope_per_sec: u64,
        psi_some: Option<PsiRatePpm>,
        psi_full: Option<PsiRatePpm>,
        now_secs: u64,
        c: Config,
    ) -> Evaluation {
        let (critical_reasons, warning_reasons) =
            Self::evaluate_triggers(available, slope_per_sec, psi_some, psi_full, &c);

        let (target_state, target_reasons) = if !critical_reasons.is_empty() {
            (State::Critical, critical_reasons)
        } else if !warning_reasons.is_empty() {
            (State::Warning, warning_reasons)
        } else {
            // No critical and no warning triggers are actively firing
            let blockers =
                Self::recovery_blockers(available, slope_per_sec, psi_some, psi_full, &c);
            if blockers.is_empty() {
                (State::Normal, Vec::new())
            } else {
                // If any recovery blockers (e.g. MemAvailable hysteresis or PSI exit threshold) remain,
                // the target state is Warning (downgrading from Critical if needed).
                (State::Warning, blockers)
            }
        };

        // Immediate escalation: Warning -> Critical bypasses dwell
        if self.state == State::Warning && target_state == State::Critical {
            self.state = State::Critical;
            self.candidate = State::Critical;
            self.candidate_reasons = target_reasons.clone();
            self.active_reasons = target_reasons;
            self.since = now_secs;
            return Evaluation {
                state: self.state,
                active_reasons: self.active_reasons.clone(),
                candidate: self.candidate,
                candidate_reasons: self.candidate_reasons.clone(),
            };
        }

        if target_state != self.candidate {
            self.candidate = target_state;
            self.candidate_reasons = target_reasons;
            self.since = now_secs;
        } else {
            // Keep latest reasons up to date for candidate
            self.candidate_reasons = target_reasons;
        }

        if target_state == self.candidate && now_secs.saturating_sub(self.since) >= c.dwell {
            self.state = target_state;
            self.active_reasons = self.candidate_reasons.clone();
        } else if self.state == target_state {
            // Update active reasons while sustaining the same state
            self.active_reasons = self.candidate_reasons.clone();
        }

        Evaluation {
            state: self.state,
            active_reasons: self.active_reasons.clone(),
            candidate: self.candidate,
            candidate_reasons: self.candidate_reasons.clone(),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn default_test_config() -> Config {
        Config {
            warning_available: 3 * 1024 * 1024 * 1024, // 3 GiB
            critical_available: 768 * 1024 * 1024,     // 768 MiB
            hysteresis: 256 * 1024 * 1024,             // 256 MiB
            dwell: 10,
            psi_some_warning: PsiRatePpm::from_percent(10.0), // 10% = 100_000 ppm
            psi_full_critical: PsiRatePpm::from_percent(5.0), // 5% = 50_000 ppm
            decline_warning_rate: 1024 * 1024 * 1024,         // 1 GiB/min
            decline_warning_gate: 6 * 1024 * 1024 * 1024,     // 6 GiB
            decline_critical_rate: 2 * 1024 * 1024 * 1024,    // 2 GiB/min
            decline_critical_gate: 4 * 1024 * 1024 * 1024,    // 4 GiB
        }
    }

    #[test]
    fn regression_9gib_healthy_with_negligible_psi_remains_normal() {
        let c = default_test_config();
        let mut p = Policy::new();
        let available_9gib = 9073 * 1024 * 1024; // 9073 MiB as reported in bug
                                                 // PSI stall around 0.01% (100 ppm)
        let negligible_psi = Some(PsiRatePpm(100));

        let eval = p.update(available_9gib, 0, negligible_psi, None, 0, c);
        assert_eq!(eval.state, State::Normal);
        assert!(eval.active_reasons.is_empty());

        let eval_later = p.update(available_9gib, 0, negligible_psi, None, 60, c);
        assert_eq!(eval_later.state, State::Normal);
        assert!(eval_later.active_reasons.is_empty());
    }

    #[test]
    fn psi_warning_and_critical_thresholds_and_recovery() {
        let c = default_test_config();
        let mut p = Policy::new();
        let healthy_mem = 8 * 1024 * 1024 * 1024;

        // PSI some at 9.9% -> Normal
        let eval = p.update(
            healthy_mem,
            0,
            Some(PsiRatePpm::from_percent(9.9)),
            None,
            0,
            c,
        );
        assert_eq!(eval.state, State::Normal);

        // PSI some at 10.0% -> Candidate Warning, after dwell becomes Warning
        let eval = p.update(
            healthy_mem,
            0,
            Some(PsiRatePpm::from_percent(10.0)),
            None,
            0,
            c,
        );
        assert_eq!(eval.state, State::Normal);
        assert_eq!(eval.candidate, State::Warning);

        let eval = p.update(
            healthy_mem,
            0,
            Some(PsiRatePpm::from_percent(10.0)),
            None,
            10,
            c,
        );
        assert_eq!(eval.state, State::Warning);
        assert!(matches!(
            eval.active_reasons[0],
            TriggerReason::PsiSomePressure { .. }
        ));

        // PSI full at 5.0% while in Warning -> Immediate escalation to Critical!
        let eval = p.update(
            healthy_mem,
            0,
            Some(PsiRatePpm::from_percent(10.0)),
            Some(PsiRatePpm::from_percent(5.0)),
            12,
            c,
        );
        assert_eq!(eval.state, State::Critical);
        assert!(matches!(
            eval.active_reasons[0],
            TriggerReason::PsiFullPressure { .. }
        ));

        // Recovery: if PSI drops to 4% (between 2.5% and 5%), full is below 5% but not below exit threshold (2.5%)
        let eval = p.update(
            healthy_mem,
            0,
            Some(PsiRatePpm::from_percent(1.0)),
            Some(PsiRatePpm::from_percent(4.0)),
            15,
            c,
        );
        // Target is Warning
        assert_eq!(eval.candidate, State::Warning);

        // When PSI full drops to 1% (< 2.5%) and some drops to 1% (< 5%), can recover to Normal after dwell
        let eval = p.update(
            healthy_mem,
            0,
            Some(PsiRatePpm::from_percent(1.0)),
            Some(PsiRatePpm::from_percent(1.0)),
            20,
            c,
        );
        assert_eq!(eval.candidate, State::Normal);
        let eval = p.update(
            healthy_mem,
            0,
            Some(PsiRatePpm::from_percent(1.0)),
            Some(PsiRatePpm::from_percent(1.0)),
            30,
            c,
        );
        assert_eq!(eval.state, State::Normal);
    }

    #[test]
    fn slope_warning_critical_gates_and_rates() {
        let c = default_test_config();
        let mut p = Policy::new();

        // 7 GiB available (> 6 GiB gate): rapid decline of 10 GiB/min is ignored because above gate
        let slope_10gib_per_sec = (10 * 1024 * 1024 * 1024) / 60;
        let eval = p.update(
            7 * 1024 * 1024 * 1024,
            slope_10gib_per_sec,
            None,
            None,
            0,
            c,
        );
        assert_eq!(eval.candidate, State::Normal);

        // 5 GiB available (< 6 GiB warning gate, but >= 4 GiB critical gate):
        // 1.5 GiB/min decline triggers warning
        let slope_1_5gib_per_sec = (1536 * 1024 * 1024) / 60;
        let eval = p.update(
            5 * 1024 * 1024 * 1024,
            slope_1_5gib_per_sec,
            None,
            None,
            0,
            c,
        );
        assert_eq!(eval.candidate, State::Warning);

        // Even with 3 GiB/min decline at 5 GiB, it only triggers Warning because MemAvailable (5 GiB) is above critical gate (4 GiB)
        let slope_3gib_per_sec = (3 * 1024 * 1024 * 1024) / 60;
        let eval = p.update(5 * 1024 * 1024 * 1024, slope_3gib_per_sec, None, None, 0, c);
        assert_eq!(eval.candidate, State::Warning);

        // At 3.5 GiB (< 4 GiB critical gate), 2.5 GiB/min decline triggers Critical
        let slope_2_5gib_per_sec = (2560 * 1024 * 1024) / 60;
        let eval = p.update(3500 * 1024 * 1024, slope_2_5gib_per_sec, None, None, 0, c);
        assert_eq!(eval.candidate, State::Critical);
    }

    #[test]
    fn critical_persistence_and_recovery_sequence() {
        let c = default_test_config();
        let mut p = Policy::new();

        // 1. Critical low-memory persistence:
        // Available = 500 MiB (<= 768 MiB critical threshold)
        let eval = p.update(500 * 1024 * 1024, 0, None, None, 0, c);
        assert_eq!(eval.state, State::Normal);
        assert_eq!(eval.candidate, State::Critical);
        // After dwell
        let eval = p.update(500 * 1024 * 1024, 0, None, None, 10, c);
        assert_eq!(eval.state, State::Critical);
        assert!(matches!(
            eval.active_reasons[0],
            TriggerReason::LowAvailable { .. }
        ));

        // Available increases to 600 MiB (still <= 768 MiB critical) -> Critical persists!
        let eval = p.update(600 * 1024 * 1024, 0, None, None, 20, c);
        assert_eq!(eval.state, State::Critical);
        assert_eq!(eval.candidate, State::Critical);
        assert!(matches!(
            eval.active_reasons[0],
            TriggerReason::LowAvailable { .. }
        ));

        // 2. Downgrade to Warning when critical triggers clear but warning triggers remain:
        // Available increases to 1 GiB (> 768 MiB critical, but <= 3 GiB warning)
        let eval = p.update(1024 * 1024 * 1024, 0, None, None, 30, c);
        // Candidate is Warning
        assert_eq!(eval.candidate, State::Warning);
        // Still Critical until dwell satisfies downgrade
        assert_eq!(eval.state, State::Critical);
        let eval = p.update(1024 * 1024 * 1024, 0, None, None, 40, c);
        assert_eq!(eval.state, State::Warning);
        assert!(matches!(
            eval.active_reasons[0],
            TriggerReason::LowAvailable { .. }
        ));

        // 3. Critical decline persistence:
        // Decline slope at 2.5 GiB/min with 2 GiB available (< 4 GiB critical gate)
        // Escalates immediately Warning -> Critical!
        let slope_2_5gib = (2560 * 1024 * 1024) / 60;
        let eval = p.update(2 * 1024 * 1024 * 1024, slope_2_5gib, None, None, 41, c);
        assert_eq!(eval.state, State::Critical);
        assert!(matches!(
            eval.active_reasons[0],
            TriggerReason::RapidDecline { .. }
        ));

        // Decline slope slows to 0, but available is 2 GiB (<= 3 GiB warning).
        // Critical trigger cleared, but warning trigger is active.
        let eval = p.update(2 * 1024 * 1024 * 1024, 0, None, None, 42, c);
        assert_eq!(eval.candidate, State::Warning);
        assert_eq!(eval.state, State::Critical);
        let eval = p.update(2 * 1024 * 1024 * 1024, 0, None, None, 52, c);
        assert_eq!(eval.state, State::Warning);

        // 4. PSI full persistence and half-threshold exit:
        // Memory 8 GiB (healthy). PSI full at 6% (>= 5% critical).
        let eval = p.update(
            8 * 1024 * 1024 * 1024,
            0,
            None,
            Some(PsiRatePpm::from_percent(6.0)),
            53,
            c,
        );
        // Escalates Warning -> Critical immediately
        assert_eq!(eval.state, State::Critical);

        // PSI full drops to 4% (< 5% threshold, but >= 2.5% half-threshold exit).
        // Critical trigger cleared, but recovery blocker keeps it in Warning candidate!
        let eval = p.update(
            8 * 1024 * 1024 * 1024,
            0,
            None,
            Some(PsiRatePpm::from_percent(4.0)),
            54,
            c,
        );
        assert_eq!(eval.candidate, State::Warning);
        // Reason must not be empty - must indicate recovering/hysteresis blocker!
        assert!(!eval.candidate_reasons.is_empty());
        assert!(matches!(
            eval.candidate_reasons[0],
            TriggerReason::Recovering { .. }
        ));

        // Wait dwell for downgrade to Warning
        let eval = p.update(
            8 * 1024 * 1024 * 1024,
            0,
            None,
            Some(PsiRatePpm::from_percent(4.0)),
            64,
            c,
        );
        assert_eq!(eval.state, State::Warning);
        assert!(matches!(
            eval.active_reasons[0],
            TriggerReason::Recovering { .. }
        ));

        // 5. Normal only after all trigger classes satisfy exit/hysteresis and dwell:
        // PSI full drops to 2% (< 2.5% half-threshold exit).
        let eval = p.update(
            8 * 1024 * 1024 * 1024,
            0,
            None,
            Some(PsiRatePpm::from_percent(2.0)),
            65,
            c,
        );
        assert_eq!(eval.candidate, State::Normal);
        assert_eq!(eval.state, State::Warning); // Dwell not reached

        let eval = p.update(
            8 * 1024 * 1024 * 1024,
            0,
            None,
            Some(PsiRatePpm::from_percent(2.0)),
            75,
            c,
        );
        assert_eq!(eval.state, State::Normal);
        assert!(eval.active_reasons.is_empty());
    }
}
