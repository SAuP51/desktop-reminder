import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type Priority = "Low" | "Normal" | "High" | "Critical";
type DisplayPosition = "Top" | "Middle" | "Bottom";
type DayFilter = "EveryDay" | "Weekdays";

type DisplayPolicy = {
  duration_seconds: number;
  speed_px_per_second: number;
  position: DisplayPosition;
  font_size: number;
  opacity_percent: number;
  click_through: boolean;
  repeat_on_screen: number;
};

type ScheduleRule = {
  date_range: null | {
    start: string;
    end: string | null;
  };
  day_filter: DayFilter | { SpecificDates: string[] };
  time_windows: Array<Record<string, unknown>>;
  exclusions: {
    dates: string[];
  };
  missed_policy: "Skip" | "FireOnce" | { FireAllLimited: { max_count: number } };
};

type Reminder = {
  id: string;
  title: string;
  message: string;
  enabled: boolean;
  priority: Priority;
  utc_offset_seconds: number;
  schedule: ScheduleRule;
  display: DisplayPolicy;
};

type AgentStatus = {
  running: boolean;
  enabled_reminders: number;
  next_fire_at: string | null;
  paused_until_utc: string | null;
};

type HistoryEntry = {
  id: number;
  reminder_id: string;
  fired_at_utc: string;
  displayed_at_utc: string | null;
  result: string;
};

type RuleMode = "once" | "daily" | "interval";

type Draft = {
  id: string | null;
  title: string;
  message: string;
  enabled: boolean;
  priority: Priority;
  mode: RuleMode;
  date: string;
  time: string;
  dayFilter: DayFilter;
  windowStart: string;
  windowEnd: string;
  intervalMinutes: number;
  display: DisplayPolicy;
};

const defaultDisplay: DisplayPolicy = {
  duration_seconds: 8,
  speed_px_per_second: 160,
  position: "Top",
  font_size: 28,
  opacity_percent: 92,
  click_through: true,
  repeat_on_screen: 1,
};

const initialDraft: Draft = {
  id: null,
  title: "Drink water",
  message: "Drink water",
  enabled: true,
  priority: "Normal",
  mode: "interval",
  date: todayInputValue(),
  time: "09:00",
  dayFilter: "EveryDay",
  windowStart: "09:00",
  windowEnd: "18:00",
  intervalMinutes: 30,
  display: defaultDisplay,
};

function App() {
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [reminders, setReminders] = useState<Reminder[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [draft, setDraft] = useState<Draft>(initialDraft);
  const [preview, setPreview] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const draftRule = useMemo(() => makeScheduleRule(draft), [draft]);
  const editing = draft.id !== null;

  async function refresh() {
    setError(null);
    try {
      const [nextStatus, nextReminders, nextHistory] = await Promise.all([
        invoke<AgentStatus>("get_status"),
        invoke<Reminder[]>("list_reminders"),
        invoke<HistoryEntry[]>("get_history", { limit: 20 }),
      ]);
      setStatus(nextStatus);
      setReminders(nextReminders);
      setHistory(nextHistory);
    } catch (err) {
      setStatus(null);
      setReminders([]);
      setHistory([]);
      setError(`Agent unavailable: ${String(err)}`);
    }
  }

  async function startAgent() {
    setBusy(true);
    setError(null);
    try {
      await invoke("start_agent");
      window.setTimeout(() => void refresh(), 650);
    } catch (err) {
      setError(`Start failed: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  }

  async function refreshPreview() {
    setError(null);
    try {
      setPreview(
        await invoke<string[]>("preview_schedule", {
          rule: draftRule,
          after: null,
          limit: 10,
        }),
      );
    } catch (err) {
      setPreview([]);
      setError(`Preview failed: ${String(err)}`);
    }
  }

  async function saveReminder() {
    setBusy(true);
    setError(null);
    try {
      const reminder = makeReminder(draft, draftRule);
      if (editing) {
        await invoke<string>("update_reminder", { reminder });
      } else {
        await invoke<string>("create_reminder", { reminder });
      }
      resetDraft();
      await refresh();
    } catch (err) {
      setError(`${editing ? "Update" : "Create"} failed: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  }

  async function setEnabled(id: string, enabled: boolean) {
    setBusy(true);
    setError(null);
    try {
      await invoke<boolean>("set_reminder_enabled", { id, enabled });
      await refresh();
    } catch (err) {
      setError(`Update failed: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  }

  async function deleteReminder(id: string) {
    setBusy(true);
    setError(null);
    try {
      await invoke<boolean>("delete_reminder", { id });
      if (draft.id === id) {
        resetDraft();
      }
      await refresh();
    } catch (err) {
      setError(`Delete failed: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  }

  async function testOverlay() {
    setError(null);
    try {
      await invoke("show_test_reminder", {
        title: draft.title || "Reminder",
        message: draft.message || "Test reminder",
        policy: draft.display,
      });
    } catch (err) {
      setError(`Test failed: ${String(err)}`);
    }
  }

  async function pause(minutes: number) {
    setError(null);
    try {
      await invoke("pause_for_duration", { minutes });
      await refresh();
    } catch (err) {
      setError(`Pause failed: ${String(err)}`);
    }
  }

  async function resume() {
    setError(null);
    try {
      await invoke("resume");
      await refresh();
    } catch (err) {
      setError(`Resume failed: ${String(err)}`);
    }
  }

  function editReminder(reminder: Reminder) {
    setDraft(reminderToDraft(reminder));
    setPreview([]);
  }

  function resetDraft() {
    setDraft({
      ...initialDraft,
      id: null,
      date: todayInputValue(),
      display: { ...defaultDisplay },
    });
    setPreview([]);
  }

  useEffect(() => {
    void refresh();
  }, []);

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <h1>Reminder Settings</h1>
          <p>{status ? statusLine(status) : "Agent not connected"}</p>
        </div>
        <div className="toolbar">
          {!status && (
            <button type="button" onClick={() => void startAgent()} disabled={busy}>
              Start Agent
            </button>
          )}
          <button type="button" onClick={() => void refresh()} disabled={busy}>
            Refresh
          </button>
          <button type="button" onClick={() => void pause(30)} disabled={!status || busy}>
            Pause 30m
          </button>
          <button type="button" onClick={() => void resume()} disabled={!status || busy}>
            Resume
          </button>
        </div>
      </header>

      {error && <div className="notice">{error}</div>}

      <section className="layout">
        <section className="panel editor">
          <div className="section-title">
            <h2>{editing ? "Edit Reminder" : "Create Reminder"}</h2>
            <div className="toolbar compact">
              {editing && (
                <button type="button" onClick={resetDraft} disabled={busy}>
                  New
                </button>
              )}
              <button type="button" onClick={() => void testOverlay()} disabled={!status}>
                Test
              </button>
            </div>
          </div>

          <div className="grid2">
            <label>
              Title
              <input value={draft.title} onChange={(event) => setDraft({ ...draft, title: event.target.value })} />
            </label>
            <label>
              Priority
              <select
                value={draft.priority}
                onChange={(event) => setDraft({ ...draft, priority: event.target.value as Priority })}
              >
                <option value="Low">Low</option>
                <option value="Normal">Normal</option>
                <option value="High">High</option>
                <option value="Critical">Critical</option>
              </select>
            </label>
          </div>

          <label>
            Message
            <textarea value={draft.message} onChange={(event) => setDraft({ ...draft, message: event.target.value })} />
          </label>

          <div className="segmented three">
            <button
              type="button"
              className={draft.mode === "once" ? "active" : ""}
              onClick={() => setDraft({ ...draft, mode: "once" })}
            >
              Once
            </button>
            <button
              type="button"
              className={draft.mode === "daily" ? "active" : ""}
              onClick={() => setDraft({ ...draft, mode: "daily" })}
            >
              Daily
            </button>
            <button
              type="button"
              className={draft.mode === "interval" ? "active" : ""}
              onClick={() => setDraft({ ...draft, mode: "interval" })}
            >
              Interval
            </button>
          </div>

          {draft.mode === "once" ? (
            <div className="grid2">
              <label>
                Date
                <input
                  type="date"
                  value={draft.date}
                  onChange={(event) => setDraft({ ...draft, date: event.target.value })}
                />
              </label>
              <label>
                Time
                <input
                  type="time"
                  value={draft.time}
                  onChange={(event) => setDraft({ ...draft, time: event.target.value })}
                />
              </label>
            </div>
          ) : (
            <>
              <div className="grid2">
                <label>
                  Days
                  <select
                    value={draft.dayFilter}
                    onChange={(event) => setDraft({ ...draft, dayFilter: event.target.value as DayFilter })}
                  >
                    <option value="EveryDay">Every day</option>
                    <option value="Weekdays">Weekdays</option>
                  </select>
                </label>
                {draft.mode === "daily" && (
                  <label>
                    Time
                    <input
                      type="time"
                      value={draft.time}
                      onChange={(event) => setDraft({ ...draft, time: event.target.value })}
                    />
                  </label>
                )}
              </div>

              {draft.mode === "interval" && (
                <div className="grid3">
                  <label>
                    Start
                    <input
                      type="time"
                      value={draft.windowStart}
                      onChange={(event) => setDraft({ ...draft, windowStart: event.target.value })}
                    />
                  </label>
                  <label>
                    End
                    <input
                      type="time"
                      value={draft.windowEnd}
                      onChange={(event) => setDraft({ ...draft, windowEnd: event.target.value })}
                    />
                  </label>
                  <label>
                    Minutes
                    <input
                      type="number"
                      min={1}
                      value={draft.intervalMinutes}
                      onChange={(event) =>
                        setDraft({ ...draft, intervalMinutes: Number(event.target.value) || 1 })
                      }
                    />
                  </label>
                </div>
              )}
            </>
          )}

          <fieldset>
            <legend>Display</legend>
            <div className="grid3">
              <label>
                Position
                <select
                  value={draft.display.position}
                  onChange={(event) =>
                    setDraft({
                      ...draft,
                      display: { ...draft.display, position: event.target.value as DisplayPosition },
                    })
                  }
                >
                  <option value="Top">Top</option>
                  <option value="Middle">Middle</option>
                  <option value="Bottom">Bottom</option>
                </select>
              </label>
              <label>
                Font
                <input
                  type="number"
                  min={12}
                  max={96}
                  value={draft.display.font_size}
                  onChange={(event) =>
                    setDraft({
                      ...draft,
                      display: { ...draft.display, font_size: Number(event.target.value) || 28 },
                    })
                  }
                />
              </label>
              <label>
                Seconds
                <input
                  type="number"
                  min={1}
                  max={120}
                  value={draft.display.duration_seconds}
                  onChange={(event) =>
                    setDraft({
                      ...draft,
                      display: { ...draft.display, duration_seconds: Number(event.target.value) || 8 },
                    })
                  }
                />
              </label>
            </div>
            <div className="grid3">
              <label>
                Speed
                <input
                  type="number"
                  min={20}
                  max={800}
                  value={draft.display.speed_px_per_second}
                  onChange={(event) =>
                    setDraft({
                      ...draft,
                      display: { ...draft.display, speed_px_per_second: Number(event.target.value) || 160 },
                    })
                  }
                />
              </label>
              <label>
                Opacity
                <input
                  type="number"
                  min={10}
                  max={100}
                  value={draft.display.opacity_percent}
                  onChange={(event) =>
                    setDraft({
                      ...draft,
                      display: { ...draft.display, opacity_percent: Number(event.target.value) || 92 },
                    })
                  }
                />
              </label>
              <label className="switch boxed">
                <input
                  type="checkbox"
                  checked={draft.display.click_through}
                  onChange={(event) =>
                    setDraft({
                      ...draft,
                      display: { ...draft.display, click_through: event.target.checked },
                    })
                  }
                />
                <span>Click-through</span>
              </label>
            </div>
          </fieldset>

          <div className="toolbar">
            <button type="button" onClick={() => void refreshPreview()} disabled={!status || busy}>
              Preview
            </button>
            <button type="button" onClick={() => void saveReminder()} disabled={!status || busy || !draft.title.trim()}>
              {editing ? "Save" : "Create"}
            </button>
          </div>

          <ol className="preview">
            {preview.map((item) => (
              <li key={item}>{formatDateTime(item)}</li>
            ))}
          </ol>
        </section>

        <section className="panel list">
          <div className="section-title">
            <h2>Reminders</h2>
            <span>{reminders.length}</span>
          </div>
          {reminders.length === 0 ? (
            <p className="empty">No reminders found.</p>
          ) : (
            <div className="rows">
              {reminders.map((reminder) => (
                <article className={draft.id === reminder.id ? "row selected" : "row"} key={reminder.id}>
                  <div>
                    <h3>{reminder.title}</h3>
                    <p>{reminderSummary(reminder)}</p>
                  </div>
                  <div className="row-actions">
                    <label className="switch">
                      <input
                        type="checkbox"
                        checked={reminder.enabled}
                        onChange={(event) => void setEnabled(reminder.id, event.target.checked)}
                      />
                      <span>{reminder.enabled ? "On" : "Off"}</span>
                    </label>
                    <button type="button" onClick={() => editReminder(reminder)} disabled={busy}>
                      Edit
                    </button>
                    <button type="button" onClick={() => void deleteReminder(reminder.id)} disabled={busy}>
                      Delete
                    </button>
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>
      </section>

      <section className="panel history">
        <div className="section-title">
          <h2>History</h2>
          <span>{history.length}</span>
        </div>
        {history.length === 0 ? (
          <p className="empty">No history yet.</p>
        ) : (
          <table>
            <thead>
              <tr>
                <th>Fired</th>
                <th>Reminder</th>
                <th>Result</th>
              </tr>
            </thead>
            <tbody>
              {history.map((entry) => (
                <tr key={entry.id}>
                  <td>{formatDateTime(entry.fired_at_utc)}</td>
                  <td>{entry.reminder_id}</td>
                  <td>{entry.result}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
    </main>
  );
}

function makeReminder(draft: Draft, rule: ScheduleRule): Reminder {
  return {
    id: draft.id ?? crypto.randomUUID(),
    title: draft.title.trim(),
    message: draft.message.trim(),
    enabled: draft.enabled,
    priority: draft.priority,
    utc_offset_seconds: new Date().getTimezoneOffset() * -60,
    schedule: rule,
    display: draft.display,
  };
}

function makeScheduleRule(draft: Draft): ScheduleRule {
  if (draft.mode === "once") {
    return {
      date_range: { start: draft.date, end: draft.date },
      day_filter: { SpecificDates: [draft.date] },
      time_windows: [{ FixedTimes: { times: [toRustTime(draft.time)] } }],
      exclusions: { dates: [] },
      missed_policy: "FireOnce",
    };
  }

  const timeWindows =
    draft.mode === "daily"
      ? [{ FixedTimes: { times: [toRustTime(draft.time)] } }]
      : [
          {
            Interval: {
              start: toRustTime(draft.windowStart),
              end: toRustTime(draft.windowEnd),
              interval_minutes: Math.max(1, draft.intervalMinutes),
              include_end: true,
            },
          },
        ];

  return {
    date_range: null,
    day_filter: draft.dayFilter,
    time_windows: timeWindows,
    exclusions: { dates: [] },
    missed_policy: "FireOnce",
  };
}

function reminderToDraft(reminder: Reminder): Draft {
  const mode = detectRuleMode(reminder.schedule);
  const fixedTimes = getFixedTimes(reminder.schedule);
  const interval = getInterval(reminder.schedule);
  const date = getSpecificDate(reminder.schedule) ?? todayInputValue();

  return {
    id: reminder.id,
    title: reminder.title,
    message: reminder.message,
    enabled: reminder.enabled,
    priority: reminder.priority,
    mode,
    date,
    time: fixedTimes[0]?.slice(0, 5) ?? "09:00",
    dayFilter: typeof reminder.schedule.day_filter === "string" ? reminder.schedule.day_filter : "EveryDay",
    windowStart: interval?.start.slice(0, 5) ?? "09:00",
    windowEnd: interval?.end.slice(0, 5) ?? "18:00",
    intervalMinutes: interval?.interval_minutes ?? 30,
    display: { ...defaultDisplay, ...reminder.display },
  };
}

function detectRuleMode(rule: ScheduleRule): RuleMode {
  if (rule.date_range?.end && rule.date_range.start === rule.date_range.end) {
    return "once";
  }
  return getInterval(rule) ? "interval" : "daily";
}

function getFixedTimes(rule: ScheduleRule): string[] {
  const window = rule.time_windows.find((item) => "FixedTimes" in item) as
    | { FixedTimes?: { times?: string[] } }
    | undefined;
  return window?.FixedTimes?.times ?? [];
}

function getInterval(rule: ScheduleRule) {
  const window = rule.time_windows.find((item) => "Interval" in item) as
    | {
        Interval?: {
          start: string;
          end: string;
          interval_minutes: number;
        };
      }
    | undefined;
  return window?.Interval ?? null;
}

function getSpecificDate(rule: ScheduleRule) {
  if (typeof rule.day_filter === "object" && "SpecificDates" in rule.day_filter) {
    return rule.day_filter.SpecificDates[0] ?? null;
  }
  return rule.date_range?.start ?? null;
}

function reminderSummary(reminder: Reminder) {
  const mode = detectRuleMode(reminder.schedule);
  const prefix = mode === "once" ? "Once" : mode === "daily" ? "Daily" : "Interval";
  return `${prefix} | ${reminder.message || "No message"}`;
}

function toRustTime(value: string) {
  return value.length === 5 ? `${value}:00` : value;
}

function statusLine(status: AgentStatus) {
  const next = status.next_fire_at ? formatDateTime(status.next_fire_at) : "no scheduled reminder";
  const pause = status.paused_until_utc ? `, paused until ${formatDateTime(status.paused_until_utc)}` : "";
  return `${status.enabled_reminders} enabled, next ${next}${pause}`;
}

function formatDateTime(value: string) {
  const normalized = value.includes("T") || value.endsWith("Z") ? value : value.replace(" ", "T");
  const date = new Date(normalized);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString();
}

function todayInputValue() {
  const today = new Date();
  const year = today.getFullYear();
  const month = String(today.getMonth() + 1).padStart(2, "0");
  const day = String(today.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
