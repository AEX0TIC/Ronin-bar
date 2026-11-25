use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase {
    Focus, 
    Break,
}

/// Pomodoro (focus, then a break by itself, over and over) or a plain timer (set any length,
/// or 0 to count up with no limit).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Pomodoro,
    Timer,
}

/// The Pomodoro rounds on offer: (focus minutes, break minutes).
pub const PRESETS: [(u64, u64); 2] = [(25, 5), (50, 10)];

///Everything for the timer needs to remember 
#[derive(Clone, Debug)]
pub struct Pomodoro {
    pub focus_len: Duration,        //how long a focus lasts 
    pub break_len: Duration,        //how long a break lasts 
    pub phase: Phase,               //Focus or Break 
    pub started: Option<Instant>,   //Some moment while running, None while paused
    pub done: Duration,             //time already done in the phase before the last pause
    pub task: String,               //what you are working on, e.g. "Write the report" (can be empty)
    pub mode: Mode,                 //Pomodoro or a plain timer
}

/// The longest task name kept (it is shown on the bar, so it stays short).
pub const MAX_TASK: usize = 60;

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
        task: String::new(),
        mode: Mode::Pomodoro,
    }
}

    /// A Pomodoro round: `set_pomodoro(50, 10)` = 50 min focus, then a 10-min break by itself.
    pub fn set_pomodoro(&mut self, focus_minutes: u64, break_minutes: u64) {
        self.mode = Mode::Pomodoro;
        self.focus_len = Duration::from_secs(focus_minutes * 60);
        self.break_len = Duration::from_secs(break_minutes * 60);
        self.reset();
    }

    /// A plain timer of any length (hours are fine); zero makes it count up with no limit.
    pub fn set_timer(&mut self, length: Duration) {
        self.mode = Mode::Timer;
        self.focus_len = length;
        self.break_len = Duration::ZERO;
        self.reset();
    }

    /// A timer set to zero counts up instead of down.
    pub fn counts_up(&self) -> bool {
        self.mode == Mode::Timer && self.focus_len.is_zero()
    }

    /// A timer that has run all the way down (a Pomodoro never finishes: it goes on to a break).
    pub fn finished(&self) -> bool {
        self.mode == Mode::Timer && !self.counts_up() && !self.is_running() && self.done >= self.focus_len
    }

    /// Sets what you are working on: spaces at the ends removed, line breaks and other control
    /// characters dropped, at most MAX_TASK characters.
    pub fn set_task(&mut self, text: &str) {
        self.task = text.trim().chars().filter(|c| !c.is_control()).take(MAX_TASK).collect();
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
        let since_start = match self.started {
            Some(start) => now.saturating_duration_since(start),
            None => Duration::ZERO
        };
        self.done + since_start
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.phase_len().saturating_sub(self.elapsed(now))
    }

    pub fn start(&mut self, now: Instant) {
        // A finished timer starts again from the beginning.
        if self.finished() { self.done = Duration::ZERO; }
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
        self.phase = Phase::Focus;
        self.started = None;
        self.done = Duration::ZERO;
    }

    pub fn tick(&mut self, now: Instant) -> bool{
        // A plain timer stops when its time is up; one counting up never ends.
        if self.mode == Mode::Timer {
            if self.is_running() && !self.counts_up() && self.remaining(now).is_zero() {
                self.started = None;
                self.done = self.focus_len;
                return true;
            }
            return false;
        }
        if self.is_running() && self.remaining(now).is_zero() {
            self.phase = match self.phase {
                Phase::Focus => Phase::Break,
                Phase::Break => Phase::Focus,
            };

            self.done = Duration::ZERO;
            self.started = Some(now);
            return true;
        }
        false
    }

    /// Time left as text, like "24:59", or "1:30:00" when an hour or more is left. A timer
    /// counting up shows the time so far instead.
    pub fn label(&self, now: Instant) -> String {
        let secs = if self.counts_up() { self.elapsed(now) } else { self.remaining(now) }.as_secs();
        if secs >= 3600 {
            format!("{}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
        } else {
            format!("{:02}:{:02}", secs / 60, secs % 60)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mins(m: u64) -> Duration { Duration:: from_secs(m * 60) }
    
    #[test]
    fn fifty_minutes_then_a_ten_minute_break() {
        let t0 = Instant::now();
        let mut p = Pomodoro::new(25, 5);
        p.set_pomodoro(50, 10);
        p.start(t0);
        assert!(!p.tick(t0 + mins(49)));
        assert!(p.tick(t0 + mins(50)));               // focus over: the break starts by itself
        assert_eq!(p.phase, Phase::Break);
        assert!(p.is_running());
        assert_eq!(p.label(t0 + mins(50)), "10:00");
        assert!(p.tick(t0 + mins(60)));               // break over: focus again
        assert_eq!(p.phase, Phase::Focus);
    }

    #[test]
    fn timer_stops_when_time_is_up() {
        let t0 = Instant::now();
        let mut p = Pomodoro::new(25, 5);
        p.set_timer(mins(90));
        assert_eq!(p.label(t0), "1:30:00");           // an hour or more shows hours
        p.start(t0);
        assert!(!p.tick(t0 + mins(89)));
        assert!(p.tick(t0 + mins(90)));               // done: it stops, no break
        assert!(!p.is_running());
        assert!(p.finished());
        assert_eq!(p.phase, Phase::Focus);
        p.start(t0 + mins(100));                      // Start again runs it from the beginning
        assert_eq!(p.label(t0 + mins(100)), "1:30:00");
    }

    #[test]
    fn timer_at_zero_counts_up_without_limit() {
        let t0 = Instant::now();
        let mut p = Pomodoro::new(25, 5);
        p.set_timer(Duration::ZERO);
        assert!(p.counts_up());
        p.start(t0);
        assert!(!p.tick(t0 + mins(60 * 30)));         // 30 hours in: still going
        assert_eq!(p.label(t0 + mins(60 * 30) + Duration::from_secs(5)), "30:00:05");
        assert!(!p.finished());
    }

    #[test]
    fn task_is_cleaned_and_kept_short() {
        let mut p = Pomodoro::new(25, 5);
        p.set_task("  Write the report\n ");
        assert_eq!(p.task, "Write the report");
        p.set_task(&"x".repeat(100));
        assert_eq!(p.task.chars().count(), MAX_TASK);
        p.reset();
        assert_eq!(p.task.chars().count(), MAX_TASK);  // reset keeps the task for the next round
    }

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
