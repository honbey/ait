/// Returns the current time as a UTC Unix timestamp.
pub fn now_timestamp() -> i64 {
    let now = js_sys::Date::new_0();
    (now.get_time() / 1000.0) as i64
}

/// Given a UTC timestamp, returns the timestamp for midnight of that day
/// in the user's local timezone.
pub fn midnight_ts(ts: i64) -> i64 {
    let ms = (ts as f64) * 1000.0;
    let date = js_sys::Date::new(&ms.into());
    let midnight = js_sys::Date::new_with_year_month_day(
        date.get_full_year(),
        date.get_month() as i32,
        date.get_date() as i32,
    );
    (midnight.get_time() / 1000.0) as i64
}

/// Clamps the end timestamp to at most `now + 3600s`. If start >= end
/// after clamping, returns a 1-day window ending at `end`.
pub fn clamp_range(start: i64, end: i64, now: i64) -> (i64, i64) {
    let end = if end > now + 3600 { now + 3600 } else { end };
    if start >= end {
        (end - 86400, end)
    } else {
        (start, end)
    }
}

/// Parses a local date string (`YYYY-MM-DD` or `YYYY-MM-DDTHH:MM`) and
/// returns the corresponding UTC Unix timestamp at local midnight (or the
/// given local hour/minute).
pub fn date_str_to_ts(s: &str) -> Option<i64> {
    let (date_part, time_part) = s.split_once('T').unzip();
    let date_part = date_part.unwrap_or(s);
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y = parts[0].parse::<i32>().ok()?;
    let m = parts[1].parse::<i32>().ok()?;
    let d = parts[2].parse::<i32>().ok()?;
    let month = m - 1;

    let (h, min) = match time_part {
        Some(t) => (
            t.split(':')
                .next()
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(0),
            t.split(':')
                .nth(1)
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(0),
        ),
        None => (0, 0),
    };

    let date = if time_part.is_some() {
        js_sys::Date::new_with_year_month_day_hr_min_sec(y as u32, month, d, h, min, 0)
    } else {
        js_sys::Date::new_with_year_month_day(y as u32, month, d)
    };

    // JS Date silently rolls out-of-range components (`2026-02-30` becomes
    // March 2), so verify the round trip instead of trusting `get_time()`.
    if date.get_full_year() as i32 != y
        || date.get_month() as i32 != month
        || date.get_date() as i32 != d
        || (time_part.is_some()
            && (date.get_hours() as i32 != h || date.get_minutes() as i32 != min))
    {
        return None;
    }

    let ts = date.get_time() / 1000.0;
    if ts.is_nan() { None } else { Some(ts as i64) }
}

/// Formats a UTC timestamp as a local date string (`YYYY-MM-DD`).
pub fn ts_to_date_str(ts: i64) -> String {
    let date = js_sys::Date::new(&((ts as f64) * 1000.0).into());
    let y = date.get_full_year();
    let m = date.get_month() + 1;
    let d = date.get_date();
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Formats a UTC timestamp as a local datetime string (`YYYY-MM-DDTHH:MM`).
pub fn ts_to_datetime_str(ts: i64) -> String {
    let d = js_sys::Date::new(&((ts as f64) * 1000.0).into());
    let y = d.get_full_year();
    let mo = d.get_month() + 1;
    let day = d.get_date();
    let h = d.get_hours();
    let min = d.get_minutes();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}", y, mo, day, h, min)
}
