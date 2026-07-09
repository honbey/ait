pub fn now_timestamp() -> i64 {
    let now = js_sys::Date::new_0();
    (now.get_time() / 1000.0) as i64
}

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

pub fn clamp_range(start: i64, end: i64, now: i64) -> (i64, i64) {
    let end = if end > now + 3600 { now + 3600 } else { end };
    if start >= end {
        (end - 86400, end)
    } else {
        (start, end)
    }
}

pub fn date_str_to_ts(s: &str) -> Option<i64> {
    let d = js_sys::Date::new(&s.into());
    let ts = d.get_time() / 1000.0;
    if ts.is_nan() { None } else { Some(ts as i64) }
}

pub fn ts_to_date_str(ts: i64) -> String {
    let date = js_sys::Date::new(&((ts as f64) * 1000.0).into());
    let y = date.get_full_year();
    let m = date.get_month() + 1;
    let d = date.get_date();
    format!("{:04}-{:02}-{:02}", y, m, d)
}

pub fn ts_to_datetime_str(ts: i64) -> String {
    let d = js_sys::Date::new(&((ts as f64) * 1000.0).into());
    let y = d.get_full_year();
    let mo = d.get_month() + 1;
    let day = d.get_date();
    let h = d.get_hours();
    let min = d.get_minutes();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}", y, mo, day, h, min)
}
