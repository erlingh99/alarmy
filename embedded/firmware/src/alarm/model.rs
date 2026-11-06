//! Alarm data structures and deserialization
use core::iter::once;
use heapless::{String, Vec};
use micromath::F32Ext;
use serde::Deserialize;
use smart_leds::RGB8;
use time::{Date, Duration, OffsetDateTime, Time, UtcOffset};

use crate::led::deserialize::deserialize_hex;
use crate::time_lib::deserialize::{deserialize_option_date_vec, deserialize_time};

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum RecurrenceType {
    #[serde(alias = "daily")]
    Daily,
    #[serde(alias = "weekly")]
    Weekly,
    #[serde(alias = "custom")]
    Custom,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct RecurrencePattern {
    pub recurrence_type: RecurrenceType, // "daily", "weekly", "custom"
    pub days: Option<[bool; 7]>,
    #[serde(deserialize_with = "deserialize_option_date_vec")]
    pub custom_dates: Option<Vec<Date, 5>>, // date strings
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum CurveType {
    #[serde(alias = "linear")]
    Linear,
    #[serde(alias = "asymptotic")]
    Asymptotic,
    #[serde(alias = "s-curve")]
    SCurve,
    #[serde(alias = "custom")]
    Custom,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ControlPoint {
    x: f32,
    y: f32,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct IntensityCurve {
    pub start_intensity: u8,
    pub end_intensity: u8,
    pub curve: CurveType,
    pub hyper_parameter: Option<i8>,
    pub control_points: Option<Vec<ControlPoint, 5>>,
}

impl IntensityCurve {
    pub fn intensity(&self, progress: f32) -> f32 {
        let start = self.start_intensity as f32 / 100f32;
        let end = self.end_intensity as f32 / 100f32;
        match self.curve {
            CurveType::Asymptotic if self.hyper_parameter.unwrap_or_default() != 0 => {
                //if the parameter does not exist or is zero, use the linear case
                let param = self.hyper_parameter.unwrap_or_default() as f32;
                let denom = 1.0 - (-param * 0.4).exp();
                if denom.abs() < f32::EPSILON {
                    (start + (end - start) * progress).clamp(0.0, 1.0)
                } else {
                    (start + (end - start) * (1.0 - (-progress * param * 0.4).exp()) / denom)
                        .clamp(0.0, 1.0)
                }
            }
            CurveType::SCurve if self.hyper_parameter.is_some() => {
                let param = self.hyper_parameter.unwrap_or_default() as f32;
                let denom = ((0.5 * param).exp() - 1.0) * (1.0 + (param * (progress - 0.5)).exp());
                if denom.abs() < f32::EPSILON {
                    (start + (end - start) * progress).clamp(0.0, 1.0)
                } else {
                    (start + (end - start) * ((param * progress).exp() - 1.0) / denom)
                        .clamp(0.0, 1.0)
                }
            }
            CurveType::Custom if !self.control_points.as_ref().is_none_or(|v| v.is_empty()) => {
                let progress_perc = progress * 100.0;

                let cp_f = ControlPoint {
                    x: 0.0,
                    y: self.start_intensity as f32,
                };
                let cp_l = ControlPoint {
                    x: 100.0,
                    y: self.end_intensity as f32,
                };

                let cps: Vec<&ControlPoint, 7> = once(&cp_f)
                    .chain(self.control_points.as_ref().unwrap().iter())
                    .chain(once(&cp_l))
                    .collect();

                let n = cps.len();
                let seg_idx = cps
                    .windows(2)
                    .position(|w| progress_perc >= w[0].x && progress_perc <= w[1].x)
                    .unwrap_or(n - 2);

                let p1 = cps[seg_idx];
                let p2 = cps[seg_idx + 1];
                let p0 = if seg_idx > 0 { cps[seg_idx - 1] } else { p1 };
                let p3 = if seg_idx + 2 < n {
                    cps[seg_idx + 2]
                } else {
                    p2
                };

                let dx = p2.x - p1.x;
                if dx.abs() < f32::EPSILON {
                    return (p1.y / 100.0).clamp(0.0, 1.0);
                }

                let t = (progress_perc - p1.x) / dx;
                let t2 = t * t;
                let t3 = t2 * t;

                let y = 0.5
                    * (2.0 * p1.y
                        + (-p0.y + p2.y) * t
                        + (2.0 * p0.y - 5.0 * p1.y + 4.0 * p2.y - p3.y) * t2
                        + (-p0.y + 3.0 * p1.y - 3.0 * p2.y + p3.y) * t3);

                (y / 100.0).clamp(0.0, 1.0)
            }
            _ => (start + (end - start) * progress).clamp(0.0, 1.0), //default and fallback to linear
        }
    }
}

pub enum AlarmError {
    InvalidAlarm,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Alarm {
    pub id: String<36>, //UUID
    // #[serde(with = "time::serde::iso8601")]
    // pub created_at: OffsetDateTime, //don't care about these fields
    // #[serde(with = "time::serde::iso8601")]
    // pub updated_at: OffsetDateTime,
    pub name: String<32>,
    #[serde(deserialize_with = "deserialize_time")]
    pub time: Time,
    #[serde(deserialize_with = "deserialize_hex")]
    pub color: RGB8, // Hex color code
    pub length: u8,
    pub is_active: bool,
    pub intensity_curve: IntensityCurve,
    pub recurrence: RecurrencePattern,
    #[serde(default = "default_play_melody")]
    pub play_melody: bool,
}

fn default_play_melody() -> bool {
    true
}

impl Alarm {
    /// Seconds into the alarm's current occurrence, or None if it isn't running. An alarm runs
    /// from its trigger for `length` minutes.
    pub fn elapsed_in_alarm(&self, now: OffsetDateTime) -> Option<u32> {
        if !self.is_active {
            return None;
        }
        // The only trigger whose window can still contain `now`
        let trigger = self
            .next_trigger(now - Duration::minutes(self.length as i64))
            .ok()?;
        u32::try_from((now - trigger).whole_seconds()).ok()
    }

    /// Seconds until the alarm next triggers, rounded up
    pub fn time_to_next_trigger(&self, now: OffsetDateTime) -> Result<u32, AlarmError> {
        Ok(secs_until(self.next_trigger(now)? - now))
    }

    /// When the alarm next triggers after `now`: its time minus its length. Errors if the alarm
    /// is invalid.
    pub fn next_trigger(&self, now: OffsetDateTime) -> Result<OffsetDateTime, AlarmError> {
        match self.recurrence.recurrence_type {
            RecurrenceType::Daily => {
                let mut next_time =
                    now.replace_time(self.time) - Duration::minutes(self.length as i64);
                // Twice when the trigger falls the day before its alarm
                while next_time <= now {
                    next_time += Duration::days(1);
                }
                Ok(next_time)
            }
            RecurrenceType::Weekly => {
                let today = now.weekday().number_from_monday() - 1; // 0 = Monday, 6 = Sunday
                let mut next_time =
                    now.replace_time(self.time) - Duration::minutes(self.length as i64);
                if let Some(mut days) = self.recurrence.days {
                    days.rotate_left(today as usize);
                    // 9 days: a trigger can fall the day before its alarm, so tomorrow's weekday
                    // next week may be the first still ahead
                    for &day in days.iter().cycle().take(9) {
                        if day && next_time > now {
                            return Ok(next_time);
                        }
                        next_time += time::Duration::days(1);
                    }
                }
                Err(AlarmError::InvalidAlarm) // No valid weekly recurrence found. Should never happen if data is valid
            }
            RecurrenceType::Custom => {
                if let Some(dates) = &self.recurrence.custom_dates {
                    // Sorted on deserialize
                    for date in dates {
                        let next_time = date.with_time(self.time).assume_offset(UtcOffset::UTC)
                            - Duration::minutes(self.length as i64);
                        if next_time > now {
                            return Ok(next_time);
                        }
                    }
                }
                Err(AlarmError::InvalidAlarm)
            }
        }
    }
}

/// Rounds up, so the scheduler never wakes before the trigger and sees it still ahead
fn secs_until(duration: Duration) -> u32 {
    let secs = duration.whole_seconds();
    (if duration.subsec_nanoseconds() > 0 {
        secs + 1
    } else {
        secs
    }) as u32
}
