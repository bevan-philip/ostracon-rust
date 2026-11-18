//! The user's regional conventions.

/// Whether numeric dates put the day first (`27/1`), as almost everywhere but the US and a few others.
///
/// `OSTRACON_DATE_ORDER=mdy` or `dmy` overrides what the system locale says.
pub fn day_first() -> bool {
    if let Ok(order) = std::env::var("OSTRACON_DATE_ORDER") {
        match order.to_ascii_lowercase().as_str() {
            "mdy" => return false,
            "dmy" => return true,
            _ => {}
        }
    }
    sys_locale::get_locale().is_none_or(|locale| day_first_in(&locale))
}

/// Whether `locale` (such as `en-US` or `en_GB`) writes the day before the month.
pub fn day_first_in(locale: &str) -> bool {
    const MONTH_FIRST: [&str; 11] = [
        "US", "PH", "FM", "MH", "PW", "GU", "AS", "MP", "PR", "UM", "VI",
    ];
    let region = locale
        .split(['-', '_', '.', '@'])
        .skip(1)
        .find(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()));
    !region.is_some_and(|region| MONTH_FIRST.contains(&region.to_ascii_uppercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::day_first_in;

    #[test]
    fn regions() {
        assert!(day_first_in("en-GB"));
        assert!(day_first_in("en_AU.UTF-8"));
        assert!(!day_first_in("en-US"));
        assert!(!day_first_in("en_US"));
        assert!(!day_first_in("zh-Hant-US"));
        assert!(day_first_in("fr"));
    }
}
