use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase {
    Focus, 
    Break,
}

///Everything for the timer needs to remember 
#[derive(Clone, Debug)]
pub struct Pomodoro {
    pub focus_len: Duration,        //how long a focus lasts 
    pub break_len: Duration,        //how long a break lasts 
    pub phase: Phase,               //Focus or Break 
    pub started: Option<Instant>,   //Some moment while running, None while paused
    pub done: Duration,             //time already done in the phase before the last pause
}

///impl holds the functions ("methods") that belong to Pomodoro
impl Pomodoro {
/// A new, paused timer. `Pomodoro::new(25, 5)` = 25 min focus, 5 min break.
    pub fn new(focus_minutes: u64, break_minutes: u64) -> Pomodoro {
    Pomodoro {
        focus_len: Duration::from_secs(focus_minutes * 60),
        break_len: Duration::from_secs(break_minutes * 60),
        phase: Phase::Focus,
        started:None,
        done: Duration::ZERO,
    }
}

    pub fn is_running(&self) -> bool{
        self.started.is_some()
}

pub fn phase_len(&self) -> Duration{
    match self.phase {
        Phase::Focus => self.focus_len,
        Phase::Break => self.break_len,
        }
    }

    pub fn elapsed(&self, now: Instant) -> Duration{
        let since_start = match.self.started {
            Some(start) => now.saturating_duration_since(start),
            None => duration::ZERO
        };
        self.done + since_start
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.phase_len().saurating_sub(self.elapsed(now))
    }

    pub fn start(&mut self, now: Instant) {
        if self.started.is_none() {
            self.started = Some(now);
        }
    }

    pub fn pause(&mut self, now:Instant) {
        if let Some(start) = self.started.take() {
            self.done += now.saturating_duration_since(start);
        }
    }

    pub fn reset (&mut self) {
        self.phase = Phase::focus;
        self.started = None;
        self.done = Duration::ZERO;
    }

    pub fn tick(&mut self, now: Instant) -> bool{
        if self.is_running() && self.remaining(now).is_zero() {
            self.phase = match self.phase {
                Phase::Focus => Phase::Break
                Phase::Break => Phase::Focus
            };

            self.done = Duration::ZERO;
            self.started = Some(now);
            return true;
        }
        false
    }

    pub fn label(&self, now: Instant) -> String {
        let secs = self.remaining(now).as_secs();
        format!("{:02}:{02}", secs / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mins(m: u64) -> Duration { Duration:: from_secs(m * 60) }
    
    #[test]
    fn pause_keep_the_time_done() {
        let t0 = Instant::now();
        let mut p = Pomodoro::new(25, 5);
        p.start(t0);
        p.pause(t0 + mins(10));
        assert!(!p.is_running());
        assert_eq!(p.remaining(t0 + mins(20)), mins(15));
        p.start(t0 + mins(30));
        assert_eq!(p.elapsed(t0 + mins(31)), mins(11));
    }
}
