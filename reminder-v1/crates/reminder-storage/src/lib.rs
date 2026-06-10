use std::path::Path;

use chrono::{DateTime, Utc};
use reminder_core::{DisplayPolicy, Priority, Reminder, ReminderId, ScheduleRule};
use rusqlite::{params, Connection};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Uuid(#[from] uuid::Error),

    #[error(transparent)]
    DateTime(#[from] chrono::ParseError),
}

pub struct ReminderStore {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct HistoryRow {
    pub id: i64,
    pub reminder_id: ReminderId,
    pub fired_at_utc: DateTime<Utc>,
    pub displayed_at_utc: Option<DateTime<Utc>>,
    pub result: String,
}

impl ReminderStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn migrate(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS reminders (
                id TEXT PRIMARY KEY NOT NULL,
                title TEXT NOT NULL,
                message TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                priority TEXT NOT NULL,
                utc_offset_seconds INTEGER NOT NULL,
                schedule_json TEXT NOT NULL,
                display_json TEXT NOT NULL,
                created_at_utc TEXT NOT NULL,
                updated_at_utc TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS reminder_runtime (
                reminder_id TEXT PRIMARY KEY NOT NULL,
                next_fire_at_utc TEXT,
                last_fire_at_utc TEXT,
                missed_count INTEGER NOT NULL DEFAULT 0,
                updated_at_utc TEXT NOT NULL,
                FOREIGN KEY(reminder_id) REFERENCES reminders(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS reminder_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                reminder_id TEXT NOT NULL,
                fired_at_utc TEXT NOT NULL,
                displayed_at_utc TEXT,
                result TEXT NOT NULL,
                FOREIGN KEY(reminder_id) REFERENCES reminders(id) ON DELETE CASCADE
            );
            "#,
        )?;
        Ok(())
    }

    pub fn upsert_reminder(&self, reminder: &Reminder) -> Result<(), StorageError> {
        let now = Utc::now().to_rfc3339();
        let priority = serde_json::to_string(&reminder.priority)?;
        let schedule_json = serde_json::to_string(&reminder.schedule)?;
        let display_json = serde_json::to_string(&reminder.display)?;

        self.conn.execute(
            r#"
            INSERT INTO reminders (
                id, title, message, enabled, priority, utc_offset_seconds,
                schedule_json, display_json, created_at_utc, updated_at_utc
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                message = excluded.message,
                enabled = excluded.enabled,
                priority = excluded.priority,
                utc_offset_seconds = excluded.utc_offset_seconds,
                schedule_json = excluded.schedule_json,
                display_json = excluded.display_json,
                updated_at_utc = excluded.updated_at_utc
            "#,
            params![
                reminder.id.to_string(),
                &reminder.title,
                &reminder.message,
                i64::from(u8::from(reminder.enabled)),
                priority,
                reminder.utc_offset_seconds,
                schedule_json,
                display_json,
                &now,
                &now,
            ],
        )?;
        Ok(())
    }

    pub fn list_reminders(&self) -> Result<Vec<Reminder>, StorageError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, title, message, enabled, priority, utc_offset_seconds, schedule_json, display_json
            FROM reminders
            ORDER BY updated_at_utc DESC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let priority_json: String = row.get(4)?;
            let schedule_json: String = row.get(6)?;
            let display_json: String = row.get(7)?;

            Ok(RawReminderRow {
                id,
                title: row.get(1)?,
                message: row.get(2)?,
                enabled: row.get::<_, i64>(3)? != 0,
                priority_json,
                utc_offset_seconds: row.get(5)?,
                schedule_json,
                display_json,
            })
        })?;

        rows.map(|row| {
            row.map_err(StorageError::from)
                .and_then(RawReminderRow::into_reminder)
        })
        .collect()
    }

    pub fn get_reminder(&self, id: ReminderId) -> Result<Option<Reminder>, StorageError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, title, message, enabled, priority, utc_offset_seconds, schedule_json, display_json
            FROM reminders WHERE id = ?1
            "#,
        )?;
        let mut rows = stmt.query(params![id.to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };

        RawReminderRow {
            id: row.get(0)?,
            title: row.get(1)?,
            message: row.get(2)?,
            enabled: row.get::<_, i64>(3)? != 0,
            priority_json: row.get(4)?,
            utc_offset_seconds: row.get(5)?,
            schedule_json: row.get(6)?,
            display_json: row.get(7)?,
        }
        .into_reminder()
        .map(Some)
    }

    pub fn set_enabled(&self, id: ReminderId, enabled: bool) -> Result<bool, StorageError> {
        let changed = self.conn.execute(
            "UPDATE reminders SET enabled = ?1, updated_at_utc = ?2 WHERE id = ?3",
            params![
                i64::from(u8::from(enabled)),
                Utc::now().to_rfc3339(),
                id.to_string()
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_reminder(&self, id: ReminderId) -> Result<bool, StorageError> {
        let changed = self.conn.execute(
            "DELETE FROM reminders WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(changed > 0)
    }

    pub fn record_history(
        &self,
        reminder_id: ReminderId,
        fired_at_utc: DateTime<Utc>,
        displayed_at_utc: Option<DateTime<Utc>>,
        result: &str,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            r#"
            INSERT INTO reminder_history (reminder_id, fired_at_utc, displayed_at_utc, result)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![
                reminder_id.to_string(),
                fired_at_utc.to_rfc3339(),
                displayed_at_utc.map(|value| value.to_rfc3339()),
                result,
            ],
        )?;
        Ok(())
    }

    pub fn list_history(&self, limit: usize) -> Result<Vec<HistoryRow>, StorageError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, reminder_id, fired_at_utc, displayed_at_utc, result
            FROM reminder_history
            ORDER BY id DESC
            LIMIT ?1
            "#,
        )?;

        let rows = stmt.query_map(params![limit.max(1) as i64], |row| {
            Ok(RawHistoryRow {
                id: row.get(0)?,
                reminder_id: row.get(1)?,
                fired_at_utc: row.get(2)?,
                displayed_at_utc: row.get(3)?,
                result: row.get(4)?,
            })
        })?;

        rows.map(|row| {
            row.map_err(StorageError::from)
                .and_then(RawHistoryRow::into_history)
        })
        .collect()
    }
}

struct RawReminderRow {
    id: String,
    title: String,
    message: String,
    enabled: bool,
    priority_json: String,
    utc_offset_seconds: i32,
    schedule_json: String,
    display_json: String,
}

impl RawReminderRow {
    fn into_reminder(self) -> Result<Reminder, StorageError> {
        Ok(Reminder {
            id: Uuid::parse_str(&self.id)?,
            title: self.title,
            message: self.message,
            enabled: self.enabled,
            priority: serde_json::from_str::<Priority>(&self.priority_json)?,
            utc_offset_seconds: self.utc_offset_seconds,
            schedule: serde_json::from_str::<ScheduleRule>(&self.schedule_json)?,
            display: serde_json::from_str::<DisplayPolicy>(&self.display_json)?,
        })
    }
}

struct RawHistoryRow {
    id: i64,
    reminder_id: String,
    fired_at_utc: String,
    displayed_at_utc: Option<String>,
    result: String,
}

impl RawHistoryRow {
    fn into_history(self) -> Result<HistoryRow, StorageError> {
        Ok(HistoryRow {
            id: self.id,
            reminder_id: Uuid::parse_str(&self.reminder_id)?,
            fired_at_utc: DateTime::parse_from_rfc3339(&self.fired_at_utc)?.with_timezone(&Utc),
            displayed_at_utc: self
                .displayed_at_utc
                .map(|value| {
                    DateTime::parse_from_rfc3339(&value).map(|parsed| parsed.with_timezone(&Utc))
                })
                .transpose()?,
            result: self.result,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveTime};
    use reminder_core::{Reminder, ScheduleRule};

    use super::ReminderStore;

    #[test]
    fn upsert_and_list_roundtrip() {
        let store = ReminderStore::open_in_memory().unwrap();
        let reminder = Reminder::new(
            "Drink water",
            "Drink water now",
            ScheduleRule::daily_interval(
                NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
                30,
            ),
        );

        store.upsert_reminder(&reminder).unwrap();
        let reminders = store.list_reminders().unwrap();

        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].id, reminder.id);
        assert_eq!(reminders[0].title, "Drink water");
    }

    #[test]
    fn delete_reminder_removes_row() {
        let store = ReminderStore::open_in_memory().unwrap();
        let reminder = Reminder::new(
            "Once",
            "Once",
            ScheduleRule::once(
                NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
                NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            ),
        );
        let id = reminder.id;

        store.upsert_reminder(&reminder).unwrap();
        assert!(store.delete_reminder(id).unwrap());
        assert!(store.get_reminder(id).unwrap().is_none());
    }
}
