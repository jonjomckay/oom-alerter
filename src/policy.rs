#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Normal,
    Warning,
    Critical,
}
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub warning: u64,
    pub critical: u64,
    pub hysteresis: u64,
    pub dwell: u64,
}
pub struct Policy {
    state: State,
    candidate: State,
    since: u64,
}
impl Policy {
    pub fn new() -> Self {
        Self {
            state: State::Normal,
            candidate: State::Normal,
            since: 0,
        }
    }
    pub fn update(
        &mut self,
        available: u64,
        slope: u64,
        psi_some_rate: u64,
        psi_full_rate: u64,
        now: u64,
        c: Config,
    ) -> State {
        let target = if available <= c.critical || slope >= c.warning || psi_full_rate >= 100_000 {
            State::Critical
        } else if available <= c.warning || slope >= c.critical || psi_some_rate >= 100_000 {
            State::Warning
        } else if available >= c.warning.saturating_add(c.hysteresis) {
            State::Normal
        } else {
            self.state
        };
        if target != self.candidate {
            self.candidate = target;
            self.since = now
        }
        if target == self.candidate && now.saturating_sub(self.since) >= c.dwell {
            self.state = target
        }
        self.state
    }

    pub fn state(&self) -> State {
        self.state
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dwell_and_recover() {
        let c = Config {
            warning: 3,
            critical: 1,
            hysteresis: 1,
            dwell: 2,
        };
        let mut p = Policy::new();
        assert_eq!(p.update(2, 0, 0, 0, 0, c), State::Normal);
        assert_eq!(p.update(2, 0, 0, 0, 2, c), State::Warning);
        assert_eq!(p.update(5, 0, 0, 0, 3, c), State::Warning);
        assert_eq!(p.update(5, 0, 0, 0, 5, c), State::Normal);
    }

    #[test]
    fn explicit_policy_transitions() {
        let c = Config {
            warning: 30,
            critical: 10,
            hysteresis: 5,
            dwell: 2,
        };
        let mut p = Policy::new();
        assert_eq!(p.update(100, 0, 0, 0, 0, c), State::Normal);
        assert_eq!(p.update(20, 0, 0, 0, 1, c), State::Normal);
        assert_eq!(p.update(20, 0, 0, 0, 3, c), State::Warning);
        assert_eq!(p.update(5, 0, 0, 0, 3, c), State::Warning);
        assert_eq!(p.update(5, 0, 0, 0, 5, c), State::Critical);
        assert_eq!(p.update(34, 0, 0, 0, 6, c), State::Critical);
        assert_eq!(p.update(36, 0, 0, 0, 7, c), State::Critical);
        assert_eq!(p.update(36, 0, 0, 0, 8, c), State::Critical);
        assert_eq!(p.update(36, 0, 0, 0, 9, c), State::Normal);
    }
}
