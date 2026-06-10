use chrono::{NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type ReminderId = Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    pub id: ReminderId,
    pub title: String,
    pub message: String,
    pub enabled: bool,
    pub priority: Priority,
    pub utc_offset_seconds: i32,
    pub schedule: ScheduleRule,
    pub display: DisplayPolicy,
}

impl Reminder {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        schedule: ScheduleRule,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            message: message.into(),
            enabled: true,
            priority: Priority::Normal,
            utc_offset_seconds: 0,
            schedule,
            display: DisplayPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRule {
    pub date_range: Option<DateRange>,
    pub day_filter: DayFilter,
    pub time_windows: Vec<TimeWindow>,
    pub exclusions: Exclusions,
    pub missed_policy: MissedPolicy,
}

impl ScheduleRule {
    pub fn once(date: NaiveDate, time: NaiveTime) -> Self {
        Self {
            date_range: Some(DateRange {
                start: date,
                end: Some(date),
            }),
            day_filter: DayFilter::SpecificDates(vec![date]),
            time_windows: vec![TimeWindow::FixedTimes { times: vec![time] }],
            exclusions: Exclusions::default(),
            missed_policy: MissedPolicy::FireOnce,
        }
    }

    pub fn daily_at(times: Vec<NaiveTime>) -> Self {
        Self {
            date_range: None,
            day_filter: DayFilter::EveryDay,
            time_windows: vec![TimeWindow::FixedTimes { times }],
            exclusions: Exclusions::default(),
            missed_policy: MissedPolicy::FireOnce,
        }
    }

    pub fn daily_interval(start: NaiveTime, end: NaiveTime, interval_minutes: u32) -> Self {
        Self {
            date_range: None,
            day_filter: DayFilter::EveryDay,
            time_windows: vec![TimeWindow::Interval {
                start,
                end,
                interval_minutes,
                include_end: true,
            }],
            exclusions: Exclusions::default(),
            missed_policy: MissedPolicy::FireOnce,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateRange {
    pub start: NaiveDate,
    pub end: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DayFilter {
    EveryDay,
    Weekdays,
    SelectedWeekdays(Vec<Weekday>),
    MonthDays(Vec<u32>),
    SpecificDates(Vec<NaiveDate>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub fn from_chrono(value: chrono::Weekday) -> Self {
        match value {
            chrono::Weekday::Mon => Self::Mon,
            chrono::Weekday::Tue => Self::Tue,
            chrono::Weekday::Wed => Self::Wed,
            chrono::Weekday::Thu => Self::Thu,
            chrono::Weekday::Fri => Self::Fri,
            chrono::Weekday::Sat => Self::Sat,
            chrono::Weekday::Sun => Self::Sun,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeWindow {
    FixedTimes {
        times: Vec<NaiveTime>,
    },
    Interval {
        start: NaiveTime,
        end: NaiveTime,
        interval_minutes: u32,
        include_end: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exclusions {
    pub dates: Vec<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MissedPolicy {
    Skip,
    FireOnce,
    FireAllLimited { max_count: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayPolicy {
    pub duration_seconds: u32,
    pub speed_px_per_second: u32,
    pub position: DisplayPosition,
    pub font_size: u32,
    pub opacity_percent: u8,
    pub click_through: bool,
    pub repeat_on_screen: u32,
}

impl Default for DisplayPolicy {
    fn default() -> Self {
        Self {
            duration_seconds: 8,
            speed_px_per_second: 160,
            position: DisplayPosition::Top,
            font_size: 28,
            opacity_percent: 92,
            click_through: true,
            repeat_on_screen: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayPosition {
    Top,
    Middle,
    Bottom,
}
