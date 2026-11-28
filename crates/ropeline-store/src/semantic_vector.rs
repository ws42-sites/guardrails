//! What counts as a valid cached embedding, and how one is written to the cache.

use crate::error::{Result, StoreError};
use ropeline_engine::py_value::{PyValue, float_repr};

pub const DIMENSIONS: usize = 768;

fn invalid() -> StoreError {
    StoreError::new("SemanticCacheError", "Invalid cached embedding")
}

/// `math.fsum`: the exactly rounded sum, which a plain loop is not (CPython's
/// Shewchuk partials). The unit-length bound below is tight enough to notice.
pub fn fsum(values: &[f64]) -> f64 {
    let mut partials: Vec<f64> = Vec::new();
    for &value in values {
        let mut x = value;
        let mut kept = 0;
        for index in 0..partials.len() {
            let mut y = partials[index];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let high = x + y;
            let low = y - (high - x);
            if low != 0.0 {
                partials[kept] = low;
                kept += 1;
            }
            x = high;
        }
        partials.truncate(kept);
        partials.push(x);
    }
    let Some(mut high) = partials.pop() else {
        return 0.0;
    };
    while let Some(y) = partials.pop() {
        let x = high;
        high = x + y;
        let low = y - (high - x);
        if low != 0.0 {
            // Round half to even: the next partial decides whether `high` is exact.
            if let Some(&next) = partials.last()
                && ((low < 0.0 && next < 0.0) || (low > 0.0 && next > 0.0))
            {
                let doubled = low * 2.0;
                let candidate = high + doubled;
                if doubled == candidate - high {
                    high = candidate;
                }
            }
            break;
        }
    }
    high
}

/// Python's `_vector`: 768 finite numbers (not booleans) in range with unit length.
pub fn validate(value: &PyValue) -> Result<Vec<f64>> {
    let PyValue::List(items) = value else {
        return Err(invalid());
    };
    if items.len() != DIMENSIONS {
        return Err(invalid());
    }
    let mut numbers = Vec::with_capacity(DIMENSIONS);
    for item in items {
        let number = match item {
            PyValue::Int(int) => *int as f64,
            PyValue::Float(float) => *float,
            _ => return Err(invalid()),
        };
        if !(-1.001..=1.001).contains(&number) {
            return Err(invalid());
        }
        numbers.push(number);
    }
    let squares: Vec<f64> = numbers.iter().map(|number| number * number).collect();
    let length = fsum(&squares);
    if !(length > 0.999 && length < 1.001) {
        return Err(invalid());
    }
    Ok(numbers)
}

/// `json.dumps(tuple_of_floats, allow_nan=False)`.
pub fn encode(vector: &[f64]) -> String {
    let parts: Vec<String> = vector.iter().map(|value| float_repr(*value)).collect();
    format!("[{}]", parts.join(", "))
}
