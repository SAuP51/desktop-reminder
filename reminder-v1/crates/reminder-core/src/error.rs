use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("schedule rule has no time windows")]
    EmptyTimeWindows,

    #[error("time window has no fixed times")]
    EmptyFixedTimes,

    #[error("interval_minutes must be greater than zero")]
    InvalidInterval,

    #[error("utc_offset_seconds must be between -86400 and 86400")]
    InvalidUtcOffset,

    #[error("date range end is before start")]
    InvalidDateRange,

    #[error("lookahead limit reached while searching for the next fire time")]
    LookaheadLimitReached,
}
