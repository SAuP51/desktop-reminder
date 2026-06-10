use std::cmp::Ordering;
use std::collections::BinaryHeap;

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};

use crate::error::CoreError;
use crate::model::{DateRange, DayFilter, Reminder, ReminderId, ScheduleRule, TimeWindow, Weekday};

const MAX_LOOKAHEAD_DAYS: i64 = 366 * 50;

pub struct ScheduleEngine;

impl ScheduleEngine {
    pub fn validate(rule: &ScheduleRule) -> Result<(), CoreError> {
        if rule.time_windows.is_empty() {
            return Err(CoreError::EmptyTimeWindows);
        }

        if let Some(range) = &rule.date_range {
            if let Some(end) = range.end {
                if end < range.start {
                    return Err(CoreError::InvalidDateRange);
                }
            }
        }

        for window in &rule.time_windows {
            match window {
                TimeWindow::FixedTimes { times } if times.is_empty() => {
                    return Err(CoreError::EmptyFixedTimes);
                }
                TimeWindow::Interval {
                    interval_minutes, ..
                } if *interval_minutes == 0 => {
                    return Err(CoreError::InvalidInterval);
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub fn next_fire_after(
        rule: &ScheduleRule,
        after: NaiveDateTime,
    ) -> Result<Option<NaiveDateTime>, CoreError> {
        Self::validate(rule)?;

        let first_anchor_day = after.date() - Duration::days(1);
        let mut best: Option<NaiveDateTime> = None;

        for day_offset in 0..=MAX_LOOKAHEAD_DAYS {
            let Some(anchor_day) = first_anchor_day.checked_add_signed(Duration::days(day_offset))
            else {
                break;
            };

            if !Self::anchor_day_allowed(rule, anchor_day) {
                if Self::range_ended(rule.date_range.as_ref(), anchor_day) {
                    return Ok(best);
                }
                continue;
            }

            for window in &rule.time_windows {
                if let Some(candidate) =
                    Self::next_in_window(window, anchor_day, after, &rule.exclusions.dates)
                {
                    let is_better = match best {
                        Some(current) => candidate < current,
                        None => true,
                    };
                    if candidate > after && is_better {
                        best = Some(candidate);
                    }
                }
            }

            if let Some(candidate) = best {
                if candidate.date() <= anchor_day {
                    return Ok(Some(candidate));
                }
            }

            if Self::range_ended(rule.date_range.as_ref(), anchor_day) {
                return Ok(best);
            }
        }

        Err(CoreError::LookaheadLimitReached)
    }

    pub fn next_fire_after_utc(
        rule: &ScheduleRule,
        after_utc: DateTime<Utc>,
        utc_offset_seconds: i32,
    ) -> Result<Option<DateTime<Utc>>, CoreError> {
        let offset =
            FixedOffset::east_opt(utc_offset_seconds).ok_or(CoreError::InvalidUtcOffset)?;
        let after_local = after_utc.with_timezone(&offset).naive_local();
        let Some(next_local) = Self::next_fire_after(rule, after_local)? else {
            return Ok(None);
        };

        let Some(next_with_offset) = next_local.and_local_timezone(offset).single() else {
            return Ok(None);
        };

        Ok(Some(next_with_offset.with_timezone(&Utc)))
    }

    pub fn preview_after(
        rule: &ScheduleRule,
        after: NaiveDateTime,
        limit: usize,
    ) -> Result<Vec<NaiveDateTime>, CoreError> {
        let mut result = Vec::new();
        let mut cursor = after;

        while result.len() < limit {
            let Some(next) = Self::next_fire_after(rule, cursor)? else {
                break;
            };

            result.push(next);
            cursor = next;
        }

        Ok(result)
    }

    fn next_in_window(
        window: &TimeWindow,
        anchor_day: NaiveDate,
        after: NaiveDateTime,
        excluded_dates: &[NaiveDate],
    ) -> Option<NaiveDateTime> {
        match window {
            TimeWindow::FixedTimes { times } => times
                .iter()
                .filter_map(|time| {
                    let candidate = NaiveDateTime::new(anchor_day, *time);
                    Self::candidate_allowed(candidate, after, excluded_dates).then_some(candidate)
                })
                .min(),
            TimeWindow::Interval {
                start,
                end,
                interval_minutes,
                include_end,
            } => Self::next_interval_candidate(
                anchor_day,
                *start,
                *end,
                *interval_minutes,
                *include_end,
                after,
                excluded_dates,
            ),
        }
    }

    fn next_interval_candidate(
        anchor_day: NaiveDate,
        start: NaiveTime,
        end: NaiveTime,
        interval_minutes: u32,
        include_end: bool,
        after: NaiveDateTime,
        excluded_dates: &[NaiveDate],
    ) -> Option<NaiveDateTime> {
        let start_dt = NaiveDateTime::new(anchor_day, start);
        let end_day = if start <= end {
            anchor_day
        } else {
            anchor_day.checked_add_signed(Duration::days(1))?
        };
        let end_dt = NaiveDateTime::new(end_day, end);
        let interval = Duration::minutes(i64::from(interval_minutes));

        if end_dt < after || (!include_end && end_dt <= after) {
            return None;
        }

        let mut candidate = if after < start_dt {
            start_dt
        } else {
            let elapsed = after.signed_duration_since(start_dt);
            let elapsed_ms = elapsed.num_milliseconds().max(0);
            let interval_ms = interval.num_milliseconds();
            let steps = (elapsed_ms / interval_ms) + 1;
            start_dt + Duration::milliseconds(steps * interval_ms)
        };

        if !include_end && candidate == end_dt {
            candidate += interval;
        }

        while candidate <= end_dt {
            if Self::candidate_allowed(candidate, after, excluded_dates) {
                return Some(candidate);
            }
            candidate += interval;
        }

        None
    }

    fn candidate_allowed(
        candidate: NaiveDateTime,
        after: NaiveDateTime,
        excluded_dates: &[NaiveDate],
    ) -> bool {
        candidate > after && !excluded_dates.contains(&candidate.date())
    }

    fn anchor_day_allowed(rule: &ScheduleRule, date: NaiveDate) -> bool {
        Self::date_in_range(rule.date_range.as_ref(), date)
            && !rule.exclusions.dates.contains(&date)
            && Self::day_filter_allows(&rule.day_filter, date)
    }

    fn date_in_range(range: Option<&DateRange>, date: NaiveDate) -> bool {
        let Some(range) = range else {
            return true;
        };

        if date < range.start {
            return false;
        }

        match range.end {
            Some(end) => date <= end,
            None => true,
        }
    }

    fn range_ended(range: Option<&DateRange>, date: NaiveDate) -> bool {
        range
            .and_then(|range| range.end)
            .is_some_and(|end| date > end)
    }

    fn day_filter_allows(filter: &DayFilter, date: NaiveDate) -> bool {
        match filter {
            DayFilter::EveryDay => true,
            DayFilter::Weekdays => matches!(
                Weekday::from_chrono(date.weekday()),
                Weekday::Mon | Weekday::Tue | Weekday::Wed | Weekday::Thu | Weekday::Fri
            ),
            DayFilter::SelectedWeekdays(days) => {
                days.contains(&Weekday::from_chrono(date.weekday()))
            }
            DayFilter::MonthDays(days) => days.contains(&date.day()),
            DayFilter::SpecificDates(dates) => dates.contains(&date),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ScheduledReminder {
    pub reminder_id: ReminderId,
    pub next_fire_at: NaiveDateTime,
}

impl Ord for ScheduledReminder {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .next_fire_at
            .cmp(&self.next_fire_at)
            .then_with(|| other.reminder_id.cmp(&self.reminder_id))
    }
}

impl PartialOrd for ScheduledReminder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone)]
pub struct DueReminder {
    pub reminder_id: ReminderId,
    pub due_at: NaiveDateTime,
}

#[derive(Debug, Default)]
pub struct SchedulerQueue {
    heap: BinaryHeap<ScheduledReminder>,
}

impl SchedulerQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rebuild(reminders: &[Reminder], after: NaiveDateTime) -> Result<Self, CoreError> {
        let mut queue = Self::new();
        for reminder in reminders.iter().filter(|item| item.enabled) {
            if let Some(next_fire_at) = ScheduleEngine::next_fire_after(&reminder.schedule, after)?
            {
                queue.push(ScheduledReminder {
                    reminder_id: reminder.id,
                    next_fire_at,
                });
            }
        }
        Ok(queue)
    }

    pub fn push(&mut self, item: ScheduledReminder) {
        self.heap.push(item);
    }

    pub fn peek_next(&self) -> Option<&ScheduledReminder> {
        self.heap.peek()
    }

    pub fn pop_due(&mut self, now: NaiveDateTime) -> Vec<DueReminder> {
        let mut due = Vec::new();
        while self
            .heap
            .peek()
            .is_some_and(|item| item.next_fire_at <= now)
        {
            let item = self.heap.pop().expect("heap item existed after peek");
            due.push(DueReminder {
                reminder_id: item.reminder_id,
                due_at: item.next_fire_at,
            });
        }
        due
    }

    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

    use crate::model::{
        DateRange, DayFilter, Exclusions, MissedPolicy, ScheduleRule, TimeWindow, Weekday,
    };

    use super::{ScheduleEngine, SchedulerQueue};

    fn dt(date: &str, time: &str) -> NaiveDateTime {
        NaiveDateTime::new(
            NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            NaiveTime::parse_from_str(time, "%H:%M").unwrap(),
        )
    }

    fn time(value: &str) -> NaiveTime {
        NaiveTime::parse_from_str(value, "%H:%M").unwrap()
    }

    #[test]
    fn interval_window_returns_next_slot_inside_window() {
        let rule = ScheduleRule::daily_interval(time("09:00"), time("18:00"), 25);
        let next = ScheduleEngine::next_fire_after(&rule, dt("2026-07-01", "10:13")).unwrap();
        assert_eq!(next, Some(dt("2026-07-01", "10:15")));
    }

    #[test]
    fn interval_window_jumps_to_next_day_after_window_end() {
        let rule = ScheduleRule::daily_interval(time("09:00"), time("18:00"), 30);
        let next = ScheduleEngine::next_fire_after(&rule, dt("2026-07-01", "18:01")).unwrap();
        assert_eq!(next, Some(dt("2026-07-02", "09:00")));
    }

    #[test]
    fn once_rule_expires_after_its_single_time() {
        let rule = ScheduleRule::once(NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(), time("09:00"));
        let next = ScheduleEngine::next_fire_after(&rule, dt("2026-07-01", "09:01")).unwrap();
        assert_eq!(next, None);
    }

    #[test]
    fn fixed_daily_times_pick_earliest_future_time() {
        let rule = ScheduleRule::daily_at(vec![time("08:00"), time("14:30"), time("22:00")]);
        let next = ScheduleEngine::next_fire_after(&rule, dt("2026-07-01", "09:00")).unwrap();
        assert_eq!(next, Some(dt("2026-07-01", "14:30")));
    }

    #[test]
    fn selected_weekdays_are_respected() {
        let rule = ScheduleRule {
            date_range: None,
            day_filter: DayFilter::SelectedWeekdays(vec![Weekday::Mon, Weekday::Wed]),
            time_windows: vec![TimeWindow::FixedTimes {
                times: vec![time("10:00")],
            }],
            exclusions: Exclusions::default(),
            missed_policy: MissedPolicy::FireOnce,
        };
        let next = ScheduleEngine::next_fire_after(&rule, dt("2026-07-02", "09:00")).unwrap();
        assert_eq!(next, Some(dt("2026-07-06", "10:00")));
    }

    #[test]
    fn date_range_end_is_respected() {
        let rule = ScheduleRule {
            date_range: Some(DateRange {
                start: NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
                end: Some(NaiveDate::from_ymd_opt(2026, 7, 3).unwrap()),
            }),
            day_filter: DayFilter::EveryDay,
            time_windows: vec![TimeWindow::FixedTimes {
                times: vec![time("10:00")],
            }],
            exclusions: Exclusions::default(),
            missed_policy: MissedPolicy::FireOnce,
        };
        let next = ScheduleEngine::next_fire_after(&rule, dt("2026-07-03", "10:01")).unwrap();
        assert_eq!(next, None);
    }

    #[test]
    fn excluded_dates_are_skipped() {
        let mut rule = ScheduleRule::daily_at(vec![time("09:00")]);
        rule.exclusions
            .dates
            .push(NaiveDate::from_ymd_opt(2026, 7, 2).unwrap());
        let next = ScheduleEngine::next_fire_after(&rule, dt("2026-07-01", "09:01")).unwrap();
        assert_eq!(next, Some(dt("2026-07-03", "09:00")));
    }

    #[test]
    fn cross_midnight_window_uses_anchor_day() {
        let rule = ScheduleRule::daily_interval(time("22:00"), time("02:00"), 30);
        let next = ScheduleEngine::next_fire_after(&rule, dt("2026-07-01", "23:45")).unwrap();
        assert_eq!(next, Some(dt("2026-07-02", "00:00")));
    }

    #[test]
    fn cross_midnight_window_can_continue_after_midnight() {
        let rule = ScheduleRule::daily_interval(time("22:00"), time("02:00"), 30);
        let next = ScheduleEngine::next_fire_after(&rule, dt("2026-07-02", "00:10")).unwrap();
        assert_eq!(next, Some(dt("2026-07-02", "00:30")));
    }

    #[test]
    fn scheduler_queue_orders_by_earliest_due_time() {
        let reminders = vec![
            crate::model::Reminder::new(
                "late",
                "late",
                ScheduleRule::daily_at(vec![time("12:00")]),
            ),
            crate::model::Reminder::new(
                "early",
                "early",
                ScheduleRule::daily_at(vec![time("09:00")]),
            ),
        ];
        let queue = SchedulerQueue::rebuild(&reminders, dt("2026-07-01", "08:00")).unwrap();
        assert_eq!(
            queue.peek_next().unwrap().next_fire_at,
            dt("2026-07-01", "09:00")
        );
    }

    #[test]
    fn preview_after_returns_multiple_future_slots() {
        let rule = ScheduleRule::daily_interval(time("09:00"), time("10:00"), 30);
        let preview = ScheduleEngine::preview_after(&rule, dt("2026-07-01", "08:59"), 4).unwrap();

        assert_eq!(
            preview,
            vec![
                dt("2026-07-01", "09:00"),
                dt("2026-07-01", "09:30"),
                dt("2026-07-01", "10:00"),
                dt("2026-07-02", "09:00"),
            ]
        );
    }
}
