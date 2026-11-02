//! The `Date` class (`CSXDate`).
//!
//! Written with reference to `SXDate.cpp` of Wintermute Engine 1.x
//! (MIT License, Copyright (c) 2003-2013 Jan Nedoma); see `NOTICE`.
//!
//! The original keeps a C `struct tm` and runs `mktime` after every
//! change, which normalises out-of-range fields (month 13 is January of
//! the next year, day 0 the last day of the previous month) and computes
//! the weekday. This object keeps the same instant as a count of seconds
//! in the local time zone, with no zone of its own: the civil fields
//! come from the proleptic Gregorian calendar, so dates compare and
//! normalise as `mktime` does, without the daylight-saving adjustments
//! of the C library.

use std::fmt;

use super::{ClassHost, arg};
use crate::host::Return;
use crate::value::{Natives, Value};

const SECONDS_PER_DAY: i64 = 86_400;
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A `Date` object: seconds since 1970-01-01 00:00:00 in the local zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DateObject {
    seconds: i64,
}

/// The civil fields of a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fields {
    /// The year.
    pub year: i64,
    /// The month, 1 to 12.
    pub month: u32,
    /// The day of the month, 1 to 31.
    pub day: u32,
    /// Hours, 0 to 23.
    pub hour: u32,
    /// Minutes, 0 to 59.
    pub minute: u32,
    /// Seconds, 0 to 59.
    pub second: u32,
    /// The day of the week, 0 (Sunday) to 6.
    pub weekday: u32,
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(year: i64, month: u32, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The seconds of possibly out-of-range fields (`mktime`): `month0` is
/// zero-based, as `tm_mon`.
fn normalize(year: i64, month0: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    let year = year + month0.div_euclid(12);
    let month = month0.rem_euclid(12) as u32 + 1;
    (days_from_civil(year, month, 1) + day - 1) * SECONDS_PER_DAY
        + hour * 3600
        + minute * 60
        + second
}

impl DateObject {
    /// `new Date(year, month, day, hours, minutes, seconds)`: missing
    /// fields are 0 (so `new Date(2020)` is month 0, day 0 of 2020:
    /// 30 November 2019, after normalisation); `new Date()` is now.
    pub fn new<H: ClassHost>(args: &[Value], host: &mut H) -> DateObject {
        let year = arg(args, 0);
        if year.is_null() {
            return DateObject::from_seconds(host.local_time());
        }
        let natives: &dyn Natives = host;
        let field = |n: usize| i64::from(arg(args, n).get_int(natives));
        DateObject::from_seconds(normalize(
            field(0),
            field(1) - 1,
            field(2),
            field(3),
            field(4),
            field(5),
        ))
    }

    /// A date from its seconds in the local zone.
    pub fn from_seconds(seconds: i64) -> DateObject {
        DateObject { seconds }
    }

    /// The seconds in the local zone.
    pub fn seconds(&self) -> i64 {
        self.seconds
    }

    /// The civil fields.
    pub fn fields(&self) -> Fields {
        let days = self.seconds.div_euclid(SECONDS_PER_DAY);
        let rem = self.seconds.rem_euclid(SECONDS_PER_DAY) as u32;
        let (year, month, day) = civil_from_days(days);
        Fields {
            year,
            month,
            day,
            hour: rem / 3600,
            minute: rem / 60 % 60,
            second: rem % 60,
            // 1970-01-01 was a Thursday.
            weekday: (days + 4).rem_euclid(7) as u32,
        }
    }

    /// Replaces one field and renormalises, as the setters do.
    fn with(&self, change: impl FnOnce(&mut [i64; 6])) -> DateObject {
        let f = self.fields();
        let mut fields = [
            f.year,
            i64::from(f.month) - 1,
            i64::from(f.day),
            i64::from(f.hour),
            i64::from(f.minute),
            i64::from(f.second),
        ];
        change(&mut fields);
        let [year, month0, day, hour, minute, second] = fields;
        DateObject::from_seconds(normalize(year, month0, day, hour, minute, second))
    }

    /// `Type`; any other name reads `null`.
    pub fn get_property(&self, name: &str) -> Value {
        match name {
            "Type" => Value::from("date"),
            _ => Value::Null,
        }
    }

    /// The getters, the setters and `SetCurrentTime`.
    pub fn call_method<H: ClassHost>(
        &mut self,
        name: &str,
        args: &[Value],
        host: &mut H,
    ) -> Option<Return> {
        let f = self.fields();
        let value = |n: usize| -> i64 {
            let natives: &dyn Natives = host;
            i64::from(arg(args, n).get_int(natives))
        };
        let int = |v: i64| Return::value(v as i32);
        Some(match name {
            "GetYear" => int(f.year),
            "GetMonth" => int(i64::from(f.month)),
            "GetDate" => int(i64::from(f.day)),
            "GetHours" => int(i64::from(f.hour)),
            "GetMinutes" => int(i64::from(f.minute)),
            "GetSeconds" => int(i64::from(f.second)),
            "GetWeekday" => int(i64::from(f.weekday)),
            "SetYear" => {
                let v = value(0);
                *self = self.with(|fields| fields[0] = v);
                Return::null()
            }
            "SetMonth" => {
                let v = value(0);
                *self = self.with(|fields| fields[1] = v - 1);
                Return::null()
            }
            "SetDate" => {
                let v = value(0);
                *self = self.with(|fields| fields[2] = v);
                Return::null()
            }
            "SetHours" => {
                let v = value(0);
                *self = self.with(|fields| fields[3] = v);
                Return::null()
            }
            "SetMinutes" => {
                let v = value(0);
                *self = self.with(|fields| fields[4] = v);
                Return::null()
            }
            "SetSeconds" => {
                let v = value(0);
                *self = self.with(|fields| fields[5] = v);
                Return::null()
            }
            "SetCurrentTime" => {
                *self = DateObject::from_seconds(host.local_time());
                Return::null()
            }
            _ => return None,
        })
    }
}

/// The string form, as C's `asctime` prints it, trailing newline
/// included: `Wed Dec 31 13:30:25 2003\n`.
impl fmt::Display for DateObject {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let f = self.fields();
        writeln!(
            out,
            "{} {}{:>3} {:02}:{:02}:{:02} {}",
            WEEKDAYS[f.weekday as usize],
            MONTHS[f.month as usize - 1],
            f.day,
            f.hour,
            f.minute,
            f.second,
            f.year
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trips() {
        for days in [-1_000_000, -719_468, -1, 0, 1, 10_957, 19_000, 1_000_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, i64::from(d)), days, "{days}");
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(days_from_civil(2000, 2, 29), 11_016);
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn normalises_like_mktime() {
        // Month 13 of 2003 is January 2004; day 0 is the previous day.
        assert_eq!(
            normalize(2003, 12, 1, 0, 0, 0),
            normalize(2004, 0, 1, 0, 0, 0)
        );
        assert_eq!(
            normalize(2020, 0, 0, 0, 0, 0),
            normalize(2019, 11, 31, 0, 0, 0)
        );
        assert_eq!(
            normalize(2020, 0, 1, 24, 0, -1),
            normalize(2020, 0, 1, 23, 59, 59)
        );
        let d = DateObject::from_seconds(normalize(1969, 11, 31, 23, 59, 59));
        assert_eq!(d.seconds(), -1);
        assert_eq!(d.fields().weekday, 3, "Wednesday");
        assert_eq!(d.to_string(), "Wed Dec 31 23:59:59 1969\n");
        let d = DateObject::from_seconds(normalize(1973, 8, 5, 1, 3, 52));
        assert_eq!(d.to_string(), "Wed Sep  5 01:03:52 1973\n");
    }
}
