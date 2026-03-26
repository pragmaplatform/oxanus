#[askama::filter_fn]
pub fn relative_time(ts: &i64, _env: &dyn askama::Values) -> askama::Result<String> {
    let now = chrono::Utc::now().timestamp();
    let diff = now - ts;

    if diff < 15 {
        return Ok("now".to_string());
    }

    let result = if diff < 60 {
        format!("{diff}s ago")
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else {
        format!("{}h ago", diff / 3600)
    };

    Ok(result)
}

#[askama::filter_fn]
pub fn relative_time_micros(ts: &i64, _env: &dyn askama::Values) -> askama::Result<String> {
    let now_micros = chrono::Utc::now().timestamp_micros();
    let diff_secs = (now_micros - ts) / 1_000_000;

    if diff_secs.abs() < 15 {
        return Ok("now".to_string());
    }

    let abs = diff_secs.abs();
    let (prefix, suffix) = if diff_secs > 0 {
        ("", " ago")
    } else {
        ("in ", "")
    };

    let result = if abs < 60 {
        format!("{prefix}{abs}s{suffix}")
    } else if abs < 3600 {
        format!("{prefix}{}m{suffix}", abs / 60)
    } else {
        let h = abs / 3600;
        let m = (abs % 3600) / 60;
        if m == 0 {
            format!("{prefix}{h}h{suffix}")
        } else {
            format!("{prefix}{h}h{m}m{suffix}")
        }
    };

    Ok(result)
}

#[askama::filter_fn]
pub fn format_latency(secs: &f64, _env: &dyn askama::Values) -> askama::Result<String> {
    if *secs > 3600.0 {
        Ok(format!("{:.1}h", secs / 3600.0))
    } else if *secs > 600.0 {
        Ok(format!("{:.1}m", secs / 60.0))
    } else {
        Ok(format!("{secs:.1}s"))
    }
}

#[askama::filter_fn]
pub fn pretty_json(val: &serde_json::Value, _env: &dyn askama::Values) -> askama::Result<String> {
    Ok(serde_json::to_string_pretty(val).unwrap_or_else(|_| val.to_string()))
}

#[askama::filter_fn]
pub fn relative_time_micros_opt(
    ts: &Option<i64>,
    _env: &dyn askama::Values,
) -> askama::Result<String> {
    let Some(ts) = ts else {
        return Ok("—".to_string());
    };

    let now_micros = chrono::Utc::now().timestamp_micros();
    let diff_secs = (now_micros - ts) / 1_000_000;

    if diff_secs.abs() < 15 {
        return Ok("now".to_string());
    }

    let abs = diff_secs.abs();
    let (prefix, suffix) = if diff_secs > 0 {
        ("", " ago")
    } else {
        ("in ", "")
    };

    let result = if abs < 60 {
        format!("{prefix}{abs}s{suffix}")
    } else if abs < 3600 {
        format!("{prefix}{}m{suffix}", abs / 60)
    } else {
        let h = abs / 3600;
        let m = (abs % 3600) / 60;
        if m == 0 {
            format!("{prefix}{h}h{suffix}")
        } else {
            format!("{prefix}{h}h{m}m{suffix}")
        }
    };

    Ok(result)
}

fn format_with_commas(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }

    let mut groups = Vec::new();
    while n > 0 {
        groups.push(n % 1000);
        n /= 1000;
    }

    let mut result = groups.last().map_or_else(String::new, |g| g.to_string());
    for g in groups.iter().rev().skip(1) {
        result.push_str(&format!(",{g:03}"));
    }
    result
}

#[askama::filter_fn]
pub fn format_number(val: &usize, _env: &dyn askama::Values) -> askama::Result<String> {
    Ok(format_with_commas(*val as u64))
}

#[askama::filter_fn]
pub fn format_number_i64(val: &i64, _env: &dyn askama::Values) -> askama::Result<String> {
    let (prefix, abs) = if *val < 0 {
        ("-", val.unsigned_abs())
    } else {
        ("", *val as u64)
    };
    Ok(format!("{prefix}{}", format_with_commas(abs)))
}

#[askama::filter_fn]
pub fn has_args(val: &serde_json::Value, _env: &dyn askama::Values) -> askama::Result<bool> {
    let empty = match val {
        serde_json::Value::Null => true,
        serde_json::Value::Object(m) => m.is_empty(),
        _ => false,
    };
    Ok(!empty)
}
