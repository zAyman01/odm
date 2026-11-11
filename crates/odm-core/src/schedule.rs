//! Queue scheduling evaluation and models.

/// Clock trait for time-dependent scheduling operations.
pub trait Clock: Send + Sync {
    /// Returns (day_of_week, "HH:MM") where day_of_week is "mon", "tue", etc.
    fn now(&self) -> (String, String);
}

/// System clock using UTC time calculation.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> (String, String) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let day_secs = now % 86400;
        let hours = day_secs / 3600;
        let minutes = (day_secs % 3600) / 60;
        let days = now / 86400;
        let weekday = match (days + 4) % 7 {
            0 => "sun",
            1 => "mon",
            2 => "tue",
            3 => "wed",
            4 => "thu",
            5 => "fri",
            _ => "sat",
        };
        (weekday.to_string(), format!("{hours:02}:{minutes:02}"))
    }
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

    /// Checks if current time matches start time on an active day.
    pub fn should_start(&self, day: &str, time: &str) -> bool {
        self.matches_day(day) && self.start_time.trim() == time.trim()
    }

    /// Checks if current time matches stop time on an active day.
    pub fn should_stop(&self, day: &str, time: &str) -> bool {
        if let Some(stop) = &self.stop_time {
            self.matches_day(day) && stop.trim() == time.trim()
        } else {
            false
        }
    }
}
