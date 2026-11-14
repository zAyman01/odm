//! Queue scheduling evaluation and models.

/// Clock trait for time-dependent scheduling operations.
pub trait Clock: Send + Sync {
    /// Returns (day_of_week, "HH:MM") where day_of_week is "mon", "tue", etc.
    fn now(&self) -> (String, String);
}

/// Weekday abbreviations in calendar order, as used by schedules.
const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// System clock reading the host's local time zone, so schedules mean wall-clock time.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> (String, String) {
        use chrono::Datelike;

        let now = chrono::Local::now();
        (
            WEEKDAYS[now.weekday().num_days_from_monday() as usize].to_string(),
            now.format("%H:%M").to_string(),
        )
    }
}

/// Returns the weekday before `day`, or `day` itself when it is not a weekday name.
fn previous_day(day: &str) -> String {
    let lower = day.trim().to_ascii_lowercase();
    WEEKDAYS
        .iter()
        .position(|name| lower.starts_with(name))
        .map_or(lower, |index| WEEKDAYS[(index + 6) % 7].to_string())
}

/// A schedule definition for a queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueSchedule {
    pub start_time: String,
    pub stop_time: Option<String>,
    pub days: Vec<String>,
}

impl QueueSchedule {
    pub fn new(
        start_time: impl Into<String>,
        stop_time: Option<String>,
        days: Vec<String>,
    ) -> Self {
        Self {
            start_time: start_time.into(),
            stop_time,
            days,
        }
    }

    /// Evaluates whether the schedule is active on the given weekday ("mon", "tue", etc.).
    pub fn matches_day(&self, day: &str) -> bool {
        if self.days.is_empty() {
            return true;
        }
        let target = day.trim().to_ascii_lowercase();
        self.days.iter().any(|d| {
            let d_clean = d.trim().to_ascii_lowercase();
            d_clean == target || target.starts_with(&d_clean) || d_clean.starts_with(&target)
        })
    }

    /// Returns whether `time` ("HH:MM") on `day` lies inside the schedule's window.
    /// A window that ends before it starts runs past midnight into the next day;
    /// without a stop time the window lasts until the end of the start day.
    pub fn is_active_at(&self, day: &str, time: &str) -> bool {
        let time = time.trim();
        let start = self.start_time.trim();
        match self.stop_time.as_deref().map(str::trim) {
            None => self.matches_day(day) && time >= start,
            Some(stop) if start <= stop => self.matches_day(day) && time >= start && time < stop,
            Some(stop) => {
                (self.matches_day(day) && time >= start)
                    || (self.matches_day(&previous_day(day)) && time < stop)
            }
        }
    }

    /// Checks if current time matches start time on an active day.
    pub fn should_start(&self, day: &str, time: &str) -> bool {
        self.matches_day(day) && self.start_time.trim() == time.trim()
    }

    /// Checks if current time matches stop time on an active day.
    pub fn should_stop(&self, day: &str, time: &str) -> bool {
        let Some(stop) = &self.stop_time else {
            return false;
        };
        // A window that runs past midnight stops on the day after it started.
        let wraps = stop.trim() < self.start_time.trim();
        let start_day_matches = if wraps {
            self.matches_day(&previous_day(day))
        } else {
            self.matches_day(day)
        };
        start_day_matches && stop.trim() == time.trim()
    }
}
