//! The `YYYY-MM` key of the usage ledger, from the clock in UTC.

use std::time::{SystemTime, UNIX_EPOCH};

/// `datetime.fromtimestamp(seconds, UTC).strftime("%Y-%m")` for years 1000 and later.
pub fn month_of(epoch_seconds: i64) -> String {
    let days = epoch_seconds.div_euclid(86_400);
    // Days since 0000-03-01, then the civil date (Howard Hinnant's algorithm).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}")
}

/// The current month in UTC, as `SemanticStore.month()` reports it.
pub fn current_month() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
    month_of(seconds)
}
