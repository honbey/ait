//! The DuckDB queries behind the analytics endpoints.
//!
//! Kept apart from the worker plumbing in `analytics.rs`: this is the only
//! module that touches SQL, and every function here is a pure query over one
//! connection, which makes them easy to read and to test on their own.

use chrono::DateTime;
use duckdb::{Connection, params, params_from_iter};

use crate::db::models::{
    BucketEntry, ModelDistEntry, OverviewMetrics, ProxyLogEntryResponse, ProxyLogQueryParams,
    ProxyLogQueryResult, TokenDistEntry,
};

fn ts_range(start_ts: i64, end_ts: i64) -> (chrono::NaiveDateTime, chrono::NaiveDateTime) {
    let start = DateTime::from_timestamp(start_ts, 0)
        .unwrap_or(DateTime::UNIX_EPOCH)
        .naive_utc();
    let end = DateTime::from_timestamp(end_ts, 0)
        .unwrap_or(DateTime::UNIX_EPOCH)
        .naive_utc();
    (start, end)
}

/// Totals for a range in one scan: request count, total tokens, and the three
/// token-kind sums that `token_dist_from_sums` splits into categories.
/// `overview_impl` previously issued three separate scans for these.
pub(crate) fn totals_impl(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> Result<(u64, u64, i64, i64, i64), duckdb::Error> {
    let (start, end) = ts_range(start_ts, end_ts);
    let mut stmt = conn.prepare_cached(
        "SELECT COUNT(*), \
                COALESCE(SUM(total_tokens), 0), \
                COALESCE(SUM(prompt_tokens), 0), \
                COALESCE(SUM(completion_tokens), 0), \
                COALESCE(SUM(cached_tokens), 0) \
         FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2",
    )?;
    stmt.query_row(params![start, end], |row| {
        Ok((
            row.get::<_, u64>(0)?,
            row.get::<_, u64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
        ))
    })
}

/// Hourly request and token buckets in one scan; shared by the standalone
/// endpoints and by `overview_impl`.
pub(crate) fn hourly_buckets_impl(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> Result<(Vec<BucketEntry>, Vec<BucketEntry>), duckdb::Error> {
    let (start, end) = ts_range(start_ts, end_ts);
    let mut stmt = conn.prepare_cached(
        "SELECT epoch(DATE_TRUNC('hour', timestamp)) AS bucket_ts, \
                COUNT(*), \
                COALESCE(SUM(total_tokens), 0) \
         FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2 \
         GROUP BY bucket_ts ORDER BY bucket_ts",
    )?;
    let rows = stmt.query_map(params![start, end], |row| {
        Ok((
            row.get::<_, f64>(0)? as i64,
            row.get::<_, i64>(1)? as u64,
            row.get::<_, i64>(2)? as u64,
        ))
    })?;
    let mut requests = Vec::new();
    let mut tokens = Vec::new();
    for r in rows.flatten() {
        requests.push(BucketEntry {
            timestamp: r.0,
            count: r.1,
        });
        tokens.push(BucketEntry {
            timestamp: r.0,
            count: r.2,
        });
    }
    Ok((requests, tokens))
}

pub(crate) fn requests_impl(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<BucketEntry>, duckdb::Error> {
    hourly_buckets_impl(conn, start_ts, end_ts).map(|buckets| buckets.0)
}

pub(crate) fn tokens_impl(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<BucketEntry>, duckdb::Error> {
    hourly_buckets_impl(conn, start_ts, end_ts).map(|buckets| buckets.1)
}

pub(crate) fn model_dist_impl(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<ModelDistEntry>, duckdb::Error> {
    let (start, end) = ts_range(start_ts, end_ts);
    let mut stmt = conn.prepare_cached(
        "SELECT model_name, COUNT(*) AS count \
         FROM proxy_log WHERE timestamp >= ?1 AND timestamp < ?2 \
         GROUP BY model_name ORDER BY count DESC",
    )?;
    let rows = stmt.query_map(params![start, end], |row| {
        Ok(ModelDistEntry {
            model: row.get(0)?,
            count: row.get::<_, i64>(1)? as u64,
        })
    })?;
    let mut out = Vec::new();
    for r in rows.flatten() {
        out.push(r);
    }
    Ok(out)
}

/// Split token sums into the three reported categories. Pure so
/// `overview_impl` can reuse the sums `totals_impl` already fetched.
fn token_dist_from_sums(prompt: i64, completion: i64, cached: i64) -> Vec<TokenDistEntry> {
    let prompt = prompt.max(0) as u64;
    let completion = completion.max(0) as u64;
    let cached = cached.max(0) as u64;
    let uncached_input = prompt - cached.min(prompt);

    vec![
        TokenDistEntry {
            category: "uncached_input".into(),
            count: uncached_input,
        },
        TokenDistEntry {
            category: "cached_input".into(),
            count: cached.min(prompt),
        },
        TokenDistEntry {
            category: "output".into(),
            count: completion,
        },
    ]
}

pub(crate) fn token_dist_impl(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> Result<Vec<TokenDistEntry>, duckdb::Error> {
    let (_, _, prompt, completion, cached) = totals_impl(conn, start_ts, end_ts)?;
    Ok(token_dist_from_sums(prompt, completion, cached))
}

pub(crate) fn overview_impl(
    conn: &Connection,
    start_ts: i64,
    end_ts: i64,
) -> Result<OverviewMetrics, duckdb::Error> {
    // Three scans cover all six aggregates the dashboard needs.
    let (total_requests, total_tokens, prompt, completion, cached) =
        totals_impl(conn, start_ts, end_ts)?;
    let (request_buckets, token_buckets) = hourly_buckets_impl(conn, start_ts, end_ts)?;
    Ok(OverviewMetrics {
        total_requests,
        total_tokens,
        request_buckets,
        token_buckets,
        model_dist: model_dist_impl(conn, start_ts, end_ts)?,
        token_dist: token_dist_from_sums(prompt, completion, cached),
    })
}

pub(crate) fn query_proxy_logs_impl(
    conn: &Connection,
    params: ProxyLogQueryParams,
) -> Result<ProxyLogQueryResult, duckdb::Error> {
    use duckdb::types::ToSql;

    let mut conditions: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();

    let push_naive = |values: &mut Vec<Box<dyn ToSql>>, ts: i64| {
        let naive = DateTime::from_timestamp(ts, 0)
            .unwrap_or(DateTime::UNIX_EPOCH)
            .naive_utc();
        values.push(Box::new(naive));
    };

    if let Some(start) = params.start_ts {
        conditions.push("timestamp >= ?".into());
        push_naive(&mut values, start);
    }
    if let Some(end) = params.end_ts {
        conditions.push("timestamp < ?".into());
        push_naive(&mut values, end);
    }
    if let Some(ref model_name) = params.model_name
        && !model_name.is_empty()
    {
        conditions.push("model_name = ?".into());
        values.push(Box::new(model_name.clone()));
    }
    if let Some(ref provider_name) = params.provider_name
        && !provider_name.is_empty()
    {
        conditions.push("provider_name = ?".into());
        values.push(Box::new(provider_name.clone()));
    }
    if let Some(ref status) = params.status
        && !status.is_empty()
    {
        conditions.push("status = ?".into());
        values.push(Box::new(status.clone()));
    }
    if let Some(ref api_key_name) = params.api_key_name
        && !api_key_name.is_empty()
    {
        conditions.push("api_key_name = ?".into());
        values.push(Box::new(api_key_name.clone()));
    }
    if let Some(ref endpoint) = params.endpoint
        && !endpoint.is_empty()
    {
        conditions.push("endpoint = ?".into());
        values.push(Box::new(endpoint.clone()));
    }
    if let Some(is_streaming) = params.is_streaming {
        conditions.push("is_streaming = ?".into());
        values.push(Box::new(is_streaming));
    }
    if let Some(ref upstream_model) = params.upstream_model
        && !upstream_model.is_empty()
    {
        conditions.push("upstream_model = ?".into());
        values.push(Box::new(upstream_model.clone()));
    }
    if let Some(ref provider_type) = params.provider_type
        && !provider_type.is_empty()
    {
        conditions.push("provider_type = ?".into());
        values.push(Box::new(provider_type.clone()));
    }
    if let Some(ref client_ip) = params.client_ip
        && !client_ip.is_empty()
    {
        conditions.push("client_ip = ?".into());
        values.push(Box::new(client_ip.clone()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let values_ref: Vec<&dyn ToSql> = values.iter().map(|v| v.as_ref()).collect();

    // Count query
    let count_sql = format!("SELECT COUNT(*) FROM proxy_log{where_clause}");
    let mut count_stmt = conn.prepare_cached(&count_sql)?;
    let total: u64 = count_stmt.query_row(params_from_iter(values_ref.clone()), |row| {
        row.get::<_, u64>(0)
    })?;

    // Data query. Both operands are user-controlled, so saturate instead of
    // overflowing: an arithmetic panic here would kill the analytics worker.
    let offset = params
        .page
        .saturating_sub(1)
        .saturating_mul(params.per_page);

    // A page past the end has no rows to return, but OFFSET still walks every
    // row before it. Skipping the scan keeps a deep out-of-range page — the
    // cheapest way to occupy a worker — from costing anything at all.
    if offset >= total {
        return Ok(ProxyLogQueryResult {
            items: Vec::new(),
            total,
        });
    }

    let data_sql = format!(
        "SELECT timestamp, api_key_name, model_name, provider_name, \
         prompt_tokens, completion_tokens, total_tokens, cached_tokens, latency_ms, status, \
         endpoint, is_streaming, time_to_first_token_ms, upstream_model, provider_type, \
         response_body_size, error_message, client_ip, request_id \
         FROM proxy_log{where_clause} ORDER BY timestamp DESC LIMIT ? OFFSET ?"
    );

    let mut data_params: Vec<&dyn ToSql> = values_ref;
    let limit_val: i64 = params.per_page as i64;
    let offset_val: i64 = offset as i64;
    data_params.push(&limit_val);
    data_params.push(&offset_val);

    let mut stmt = conn.prepare_cached(&data_sql)?;
    let rows = stmt.query_map(params_from_iter(data_params), |row| {
        Ok(ProxyLogEntryResponse {
            timestamp: row
                .get::<_, chrono::NaiveDateTime>(0)?
                .and_utc()
                .timestamp(),
            api_key_name: row.get(1)?,
            model_name: row.get(2)?,
            provider_name: row.get(3)?,
            prompt_tokens: row.get(4)?,
            completion_tokens: row.get(5)?,
            total_tokens: row.get(6)?,
            cached_tokens: row.get(7)?,
            latency_ms: row.get(8)?,
            status: row.get(9)?,
            endpoint: row.get(10)?,
            is_streaming: row.get(11)?,
            time_to_first_token_ms: row.get(12)?,
            upstream_model: row.get(13)?,
            provider_type: row.get(14)?,
            response_body_size: row.get(15)?,
            error_message: row.get(16)?,
            client_ip: row.get(17)?,
            request_id: row.get(18)?,
        })
    })?;

    let mut items = Vec::new();
    for item in rows.flatten() {
        items.push(item);
    }

    Ok(ProxyLogQueryResult { items, total })
}
