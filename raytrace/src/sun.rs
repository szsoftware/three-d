//!
//! The position of the sun for a place on earth, a date and a time.
//!

use three_d_asset::prelude::*;

///
/// A place on earth in degrees.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoLocation {
    /// Latitude in degrees, positive to the north.
    pub latitude: f64,
    /// Longitude in degrees, positive to the east.
    pub longitude: f64,
}

impl GeoLocation {
    /// A place by its latitude and longitude in degrees (north and east positive).
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
        }
    }
}

///
/// A date and a local clock time with the offset of the time zone to UTC, including daylight saving time.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalDateTime {
    /// Year, for example 2026.
    pub year: i32,
    /// Month from 1 to 12.
    pub month: u32,
    /// Day of the month from 1.
    pub day: u32,
    /// Hour from 0 to 23.
    pub hour: u32,
    /// Minute from 0 to 59.
    pub minute: u32,
    /// Seconds.
    pub second: f64,
    /// Hours the local time is ahead of UTC, 1 for central European winter time and 2 for summer time.
    pub utc_offset_hours: f64,
}

impl LocalDateTime {
    /// A local date and time to the minute.
    pub fn new(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        utc_offset_hours: f64,
    ) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second: 0.0,
            utc_offset_hours,
        }
    }

    /// Days since 1970-01-01 00:00 UTC, with the time of day as fraction.
    fn days_since_epoch_utc(&self) -> f64 {
        days_from_civil(self.year, self.month, self.day) as f64
            + (self.hour as f64 + self.minute as f64 / 60.0 + self.second / 3600.0
                - self.utc_offset_hours)
                / 24.0
    }
}

///
/// How the scene lies on earth: which way is up and which way is north.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneOrientation {
    /// Up in scene coordinates.
    pub up: Vec3,
    /// North in scene coordinates, perpendicular to up.
    pub north: Vec3,
}

impl Default for SceneOrientation {
    /// Y up and north along -Z, so east is +X, as in glTF and most three-d scenes.
    fn default() -> Self {
        Self {
            up: vec3(0.0, 1.0, 0.0),
            north: vec3(0.0, 0.0, -1.0),
        }
    }
}

impl SceneOrientation {
    /// East in scene coordinates.
    pub fn east(&self) -> Vec3 {
        self.north.cross(self.up).normalize()
    }

    /// The direction for an azimuth (degrees clockwise from north) and an elevation (degrees above the horizon).
    pub fn direction(&self, azimuth_degrees: f64, elevation_degrees: f64) -> Vec3 {
        let (az, el) = (azimuth_degrees.to_radians(), elevation_degrees.to_radians());
        let horizontal = self.north.normalize() * az.cos() as f32 + self.east() * az.sin() as f32;
        (self.up.normalize() * el.sin() as f32 + horizontal * el.cos() as f32).normalize()
    }
}

///
/// Where the sun stands in the sky, seen from a place on earth.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunPosition {
    /// Degrees clockwise from north: 90 is east, 180 south, 270 west.
    pub azimuth: f64,
    /// Degrees above the horizon, negative at night. Atmospheric refraction is not included.
    pub elevation: f64,
}

impl SunPosition {
    ///
    /// The position of the sun with the algorithm of the NOAA Global Monitoring Division, accurate to a few arc minutes
    /// between 1950 and 2050, which is far below the size of the sun disc.
    ///
    pub fn at(location: GeoLocation, time: LocalDateTime) -> Self {
        let days = time.days_since_epoch_utc();
        let day_floor = days.floor();
        let (year, _, _) = civil_from_days(day_floor as i64);
        let day_of_year = day_floor - days_from_civil(year, 1, 1) as f64; // 0 on January 1st
        let hour_utc = (days - day_floor) * 24.0;
        let days_in_year = if is_leap_year(year) { 366.0 } else { 365.0 };

        // Fractional year in radians
        let g =
            2.0 * std::f64::consts::PI / days_in_year * (day_of_year + (hour_utc - 12.0) / 24.0);
        let eq_time = 229.18
            * (0.000075 + 0.001868 * g.cos()
                - 0.032077 * g.sin()
                - 0.014615 * (2.0 * g).cos()
                - 0.040849 * (2.0 * g).sin());
        let declination = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin()
            - 0.006758 * (2.0 * g).cos()
            + 0.000907 * (2.0 * g).sin()
            - 0.002697 * (3.0 * g).cos()
            + 0.00148 * (3.0 * g).sin();

        // True solar time in minutes and hour angle
        let solar_minutes = hour_utc * 60.0 + eq_time + 4.0 * location.longitude;
        let hour_angle = (solar_minutes / 4.0 - 180.0).to_radians();
        let lat = location.latitude.to_radians();

        let cos_zenith = (lat.sin() * declination.sin()
            + lat.cos() * declination.cos() * hour_angle.cos())
        .clamp(-1.0, 1.0);
        let elevation = 90.0 - cos_zenith.acos().to_degrees();
        let azimuth = (180.0
            + hour_angle
                .sin()
                .atan2(hour_angle.cos() * lat.sin() - declination.tan() * lat.cos())
                .to_degrees())
        .rem_euclid(360.0);
        Self { azimuth, elevation }
    }

    /// Unit vector from the scene towards the sun.
    pub fn direction(&self, orientation: &SceneOrientation) -> Vec3 {
        orientation.direction(self.azimuth, self.elevation)
    }
}

fn is_leap_year(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Days since 1970-01-01 for a date of the proleptic Gregorian calendar (H. Hinnant's algorithm).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date for days since 1970-01-01.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BERLIN: GeoLocation = GeoLocation {
        latitude: 52.52,
        longitude: 13.405,
    };

    /// The highest position of the day, searched minute by minute.
    fn noon(
        location: GeoLocation,
        year: i32,
        month: u32,
        day: u32,
        offset: f64,
    ) -> (SunPosition, f64) {
        let mut best = (
            SunPosition::at(location, LocalDateTime::new(year, month, day, 0, 0, offset)),
            0.0,
        );
        for minute in 0..24 * 60 {
            let p = SunPosition::at(
                location,
                LocalDateTime::new(year, month, day, minute / 60, minute % 60, offset),
            );
            if p.elevation > best.0.elevation {
                best = (p, minute as f64 / 60.0);
            }
        }
        best
    }

    #[test]
    fn calendar_round_trip() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for z in [-1000, 0, 11_017, 20_000, 20_774] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
    }

    #[test]
    fn midsummer_noon_in_berlin() {
        // At solar noon the elevation is 90° - latitude + declination (23.44° at the solstice), due south,
        // at about 13:07 summer time in Berlin
        let (p, hour) = noon(BERLIN, 2026, 6, 21, 2.0);
        assert!((p.elevation - (90.0 - 52.52 + 23.44)).abs() < 0.2, "{p:?}");
        assert!((p.azimuth - 180.0).abs() < 1.0, "{p:?}");
        assert!((hour - 13.1).abs() < 0.1, "{hour}");
    }

    #[test]
    fn equinox_and_winter_in_berlin() {
        let (p, _) = noon(BERLIN, 2026, 3, 20, 1.0);
        assert!((p.elevation - (90.0 - 52.52)).abs() < 0.5, "{p:?}");
        let (p, _) = noon(BERLIN, 2026, 12, 21, 1.0);
        assert!((p.elevation - (90.0 - 52.52 - 23.44)).abs() < 0.2, "{p:?}");
    }

    #[test]
    fn morning_east_evening_west_night_below() {
        let morning = SunPosition::at(BERLIN, LocalDateTime::new(2026, 6, 21, 8, 0, 2.0));
        let evening = SunPosition::at(BERLIN, LocalDateTime::new(2026, 6, 21, 19, 0, 2.0));
        let night = SunPosition::at(BERLIN, LocalDateTime::new(2026, 12, 21, 23, 0, 1.0));
        assert!(
            morning.azimuth > 45.0 && morning.azimuth < 135.0,
            "{morning:?}"
        );
        assert!(
            evening.azimuth > 225.0 && evening.azimuth < 315.0,
            "{evening:?}"
        );
        assert!(night.elevation < -30.0, "{night:?}");
    }

    #[test]
    fn southern_hemisphere_sees_the_sun_in_the_north() {
        let sydney = GeoLocation::new(-33.87, 151.21);
        let (p, _) = noon(sydney, 2026, 12, 21, 11.0);
        assert!(
            p.azimuth < 2.0 || p.azimuth > 358.0 || p.elevation > 89.0,
            "{p:?}"
        );
        assert!((p.elevation - (90.0 - 33.87 + 23.44)).abs() < 0.3, "{p:?}");
    }

    #[test]
    fn local_time_across_midnight() {
        // 00:30 on January 1st at UTC+2 is still December 31st in UTC
        let a = SunPosition::at(BERLIN, LocalDateTime::new(2027, 1, 1, 0, 30, 2.0));
        let b = SunPosition::at(BERLIN, LocalDateTime::new(2026, 12, 31, 22, 30, 0.0));
        assert!((a.elevation - b.elevation).abs() < 1e-9 && (a.azimuth - b.azimuth).abs() < 1e-9);
    }

    #[test]
    fn directions_in_the_scene() {
        let o = SceneOrientation::default();
        assert!((o.east() - vec3(1.0, 0.0, 0.0)).magnitude() < 1e-6);
        assert!((o.direction(180.0, 0.0) - vec3(0.0, 0.0, 1.0)).magnitude() < 1e-6);
        assert!((o.direction(0.0, 90.0) - vec3(0.0, 1.0, 0.0)).magnitude() < 1e-6);
    }
}
