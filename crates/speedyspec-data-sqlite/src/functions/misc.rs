//! MySQL's control, comparison, network, lock and session functions.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use rusqlite::types::{Value, ValueRef};
use speedyspec_data_translate::SessionVars;

use super::{deterministic, number, text, user_error, volatile};
use crate::collation::fold;

fn truthy(v: ValueRef<'_>) -> bool {
    match v {
        ValueRef::Null => false,
        ValueRef::Integer(n) => n != 0,
        ValueRef::Real(f) => f != 0.0,
        ValueRef::Text(b) | ValueRef::Blob(b) => {
            super::numeric_prefix(&String::from_utf8_lossy(b)) != 0.0
        }
    }
}

fn owned(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => Value::Integer(n),
        ValueRef::Real(f) => Value::Real(f),
        ValueRef::Text(b) => Value::Text(String::from_utf8_lossy(b).into_owned()),
        ValueRef::Blob(b) => Value::Blob(b.to_vec()),
    }
}

/// `GREATEST` (`pick_max`) or `LEAST`: NULL if any argument is; numeric when every argument is a number.
fn extreme(ctx: &rusqlite::functions::Context<'_>, pick_max: bool) -> Value {
    let values: Vec<ValueRef<'_>> = (0..ctx.len()).map(|i| ctx.get_raw(i)).collect();
    if values.is_empty() || values.iter().any(|v| matches!(v, ValueRef::Null)) {
        return Value::Null;
    }
    let numeric = values
        .iter()
        .all(|v| matches!(v, ValueRef::Integer(_) | ValueRef::Real(_)));
    if numeric {
        let all_int = values.iter().all(|v| matches!(v, ValueRef::Integer(_)));
        let nums: Vec<f64> = (0..ctx.len()).filter_map(|i| number(ctx, i)).collect();
        let best = nums
            .iter()
            .copied()
            .fold(if pick_max { f64::MIN } else { f64::MAX }, |a, b| {
                if pick_max { a.max(b) } else { a.min(b) }
            });
        return if all_int {
            Value::Integer(best as i64)
        } else {
            Value::Real(best)
        };
    }
    let texts: Vec<String> = (0..ctx.len()).filter_map(|i| text(ctx, i)).collect();
    let best = texts
        .iter()
        .max_by(|a, b| {
            let order = fold(a).cmp(&fold(b));
            if pick_max { order } else { order.reverse() }
        })
        .cloned()
        .unwrap_or_default();
    Value::Text(best)
}

pub(crate) fn register(
    conn: &Connection,
    session: Arc<Mutex<SessionVars>>,
) -> rusqlite::Result<()> {
    let f = deterministic();
    conn.create_scalar_function("if", 3, f, |ctx| {
        Ok(if truthy(ctx.get_raw(0)) {
            owned(ctx.get_raw(1))
        } else {
            owned(ctx.get_raw(2))
        })
    })?;
    // MySQL's ISNULL(x) cannot be a SQLite function (ISNULL is SQLite's postfix keyword): the DML pass writes `x IS NULL`.
    conn.create_scalar_function("greatest", -1, f, |ctx| Ok(extreme(ctx, true)))?;
    conn.create_scalar_function("least", -1, f, |ctx| Ok(extreme(ctx, false)))?;
    conn.create_scalar_function("rand", 0, volatile(), |_| {
        let bits = uuid::Uuid::new_v4().as_u128() >> 75;
        Ok(Value::Real(bits as f64 / (1u64 << 53) as f64))
    })?;
    // A seeded rand(N) is the same for one seed, as MySQL's first value of a seeded sequence is.
    conn.create_scalar_function("rand", 1, f, |ctx| {
        let seed = super::int(ctx, 0).unwrap_or(0) as u64;
        let mut x = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0x6A09_E667_F3BC_C909);
        x ^= x >> 33;
        x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        x ^= x >> 33;
        Ok(Value::Real((x >> 11) as f64 / (1u64 << 53) as f64))
    })?;
    conn.create_scalar_function("uuid", 0, volatile(), |_| {
        Ok(Value::Text(uuid::Uuid::new_v4().to_string()))
    })?;
    conn.create_scalar_function("inet_aton", 1, f, |ctx| {
        let Some(s) = text(ctx, 0) else {
            return Ok(Value::Null);
        };
        let parts: Vec<&str> = s.split('.').collect();
        if parts.is_empty() || parts.len() > 4 {
            return Ok(Value::Null);
        }
        let mut nums = Vec::new();
        for p in &parts {
            match p.parse::<u64>() {
                Ok(n) if n <= 255 => nums.push(n),
                _ => return Ok(Value::Null),
            }
        }
        // MySQL reads a short address's last part as the low bytes ('127.1' is 127.0.0.1).
        let last = nums.pop().unwrap_or(0);
        let mut value = 0u64;
        for (i, n) in nums.iter().enumerate() {
            value |= n << (24 - 8 * i);
        }
        Ok(Value::Integer((value | last) as i64))
    })?;
    conn.create_scalar_function("inet_ntoa", 1, f, |ctx| {
        let Some(n) = super::int(ctx, 0) else {
            return Ok(Value::Null);
        };
        if !(0..=u32::MAX as i64).contains(&n) {
            return Ok(Value::Null);
        }
        let n = n as u32;
        Ok(Value::Text(format!(
            "{}.{}.{}.{}",
            n >> 24,
            (n >> 16) & 255,
            (n >> 8) & 255,
            n & 255
        )))
    })?;
    for (name, n_arg) in [("get_lock", 2), ("release_lock", 1), ("is_free_lock", 1)] {
        // One writer at a time already: every named lock is granted.
        conn.create_scalar_function(name, n_arg, volatile(), |_| Ok(Value::Integer(1)))?;
    }
    conn.create_scalar_function("connection_id", 0, f, |_| {
        Ok(Value::Integer(i64::from(std::process::id())))
    })?;
    let s = session.clone();
    conn.create_scalar_function("version", 0, volatile(), move |_| {
        let s = s
            .lock()
            .map_err(|_| user_error("the session is poisoned"))?;
        Ok(s.get("version")
            .map_or(Value::Null, |v| Value::Text(v.to_string())))
    })?;
    let s = session.clone();
    conn.create_scalar_function("database", 0, volatile(), move |_| {
        let s = s
            .lock()
            .map_err(|_| user_error("the session is poisoned"))?;
        Ok(Value::Text(s.database().to_string()))
    })?;
    conn.create_scalar_function("speedyspec_sysvar", 1, volatile(), move |ctx| {
        let Some(name) = text(ctx, 0) else {
            return Ok(Value::Null);
        };
        let s = session
            .lock()
            .map_err(|_| user_error("the session is poisoned"))?;
        Ok(s.get(&name)
            .map_or(Value::Null, |v| Value::Text(v.to_string())))
    })?;
    Ok(())
}
