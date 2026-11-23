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
}
