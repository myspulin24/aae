//! Kalendář kampaně: herní den 0 = 1. 1. 2028 (výchozí začátek z návrhu).

/// Datum v gregoriánském kalendáři.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Algoritmus H. Hinnanta, počet dní od 1970-01-01.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

const fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

const EPOCH: i64 = days_from_civil(2028, 1, 1);

/// Datum herního dne `day` (den 0 = 1. 1. 2028).
pub const fn date_of_day(day: u64) -> Date {
    let (y, m, d) = civil_from_days(EPOCH + day as i64);
    Date {
        year: y as i32,
        month: m as u32,
        day: d as u32,
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}. {}. {}", self.day, self.month, self.year)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dates() {
        assert_eq!(
            date_of_day(0),
            Date {
                year: 2028,
                month: 1,
                day: 1
            }
        );
        // 2028 je přestupný rok.
        assert_eq!(
            date_of_day(59),
            Date {
                year: 2028,
                month: 2,
                day: 29
            }
        );
        assert_eq!(
            date_of_day(365),
            Date {
                year: 2028,
                month: 12,
                day: 31
            }
        );
        assert_eq!(
            date_of_day(366),
            Date {
                year: 2029,
                month: 1,
                day: 1
            }
        );
        assert_eq!(
            date_of_day(730),
            Date {
                year: 2029,
                month: 12,
                day: 31
            }
        );
        assert_eq!(date_of_day(0).to_string(), "1. 1. 2028");
    }
}
