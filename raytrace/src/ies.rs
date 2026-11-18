//!
//! Light distributions of luminaires from IES files (ANSI/IES LM-63), as manufacturers publish them.
//!

use crate::RtError;
use std::f32::consts::PI;
use three_d_asset::prelude::*;

/// Vertical angles of the table: every degree from straight down (0°) to straight up (180°).
pub(crate) const VERTICAL_STEPS: usize = 181;
/// Horizontal angles of the table when the light is not the same all around: every 5° from 0° to 360°.
pub(crate) const HORIZONTAL_STEPS: usize = 73;

///
/// How bright a luminaire shines in each direction: its luminous intensity in cd over the vertical angle (0° straight
/// down, 180° straight up) and the horizontal angle (C-planes, 0° towards the front of the luminaire, 90° to its left
/// seen from above), read from an IES file and resampled to a regular table (1° vertically, 5° horizontally).
///
/// Only photometry of type C is read, the one of almost all architectural luminaires, without tilt data
/// (`TILT=NONE`).
///
#[derive(Clone, Debug, PartialEq)]
pub struct IesProfile {
    /// Intensities in cd: rows of [VERTICAL_STEPS] values, one row (same all around) or [HORIZONTAL_STEPS] rows.
    table: Vec<f32>,
    horizontal_steps: usize,
    /// Lumen of the lamps as the file states, `None` for absolute photometry (LED luminaires).
    lamp_flux: Option<f32>,
}

impl IesProfile {
    ///
    /// Reads the text of an IES file of the versions LM-63-1986, 1991, 1995, 2002 and 2019.
    ///
    pub fn parse(text: &str) -> Result<Self, RtError> {
        let error = |why: &str| RtError::Ies(why.to_owned());
        let mut lines = text.lines();
        // Keywords up to the line with TILT
        let tilt = loop {
            let line = lines.next().ok_or_else(|| error("no TILT line"))?.trim();
            if let Some(tilt) = line.strip_prefix("TILT=") {
                break tilt.trim().to_owned();
            }
        };
        if tilt != "NONE" {
            return Err(error("only TILT=NONE is supported"));
        }
        let rest: Vec<&str> = lines.collect();
        let mut numbers = rest
            .iter()
            .flat_map(|line| line.split(|c: char| c.is_whitespace() || c == ','))
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<f32>()
                    .map_err(|_| error(&format!("not a number: {s}")))
            });
        let mut next = || {
            numbers
                .next()
                .unwrap_or_else(|| Err(error("the file ends early")))
        };
        let lamps = next()?;
        let lumens_per_lamp = next()?;
        let multiplier = next()?;
        let vertical_count = next()? as usize;
        let horizontal_count = next()? as usize;
        let photometric_type = next()? as u32;
        let _units = next()?;
        let _width = next()?;
        let _length = next()?;
        let _height = next()?;
        let ballast_factor = next()?;
        let _future = next()?;
        let _watts = next()?;
        if photometric_type != 1 {
            return Err(error("only type C photometry is supported"));
        }
        if vertical_count < 2 || horizontal_count < 1 {
            return Err(error("too few angles"));
        }
        let vertical: Vec<f32> = (0..vertical_count)
            .map(|_| next())
            .collect::<Result<_, _>>()?;
        let horizontal: Vec<f32> = (0..horizontal_count)
            .map(|_| next())
            .collect::<Result<_, _>>()?;
        let mut candela = Vec::with_capacity(vertical_count * horizontal_count);
        for _ in 0..vertical_count * horizontal_count {
            candela.push(next()? * multiplier * ballast_factor);
        }
        if vertical.windows(2).any(|w| w[1] <= w[0]) || horizontal.windows(2).any(|w| w[1] <= w[0])
        {
            return Err(error("the angles do not increase"));
        }

        // Intensity at a vertical angle in one C-plane of the file, zero outside the measured range
        let in_plane = |h: usize, v: f32| -> f32 {
            let row = &candela[h * vertical_count..(h + 1) * vertical_count];
            if v < vertical[0] || v > vertical[vertical_count - 1] {
                return 0.0;
            }
            let i = vertical
                .partition_point(|a| *a <= v)
                .clamp(1, vertical_count - 1);
            let t = (v - vertical[i - 1]) / (vertical[i] - vertical[i - 1]);
            row[i - 1] + (row[i] - row[i - 1]) * t.clamp(0.0, 1.0)
        };
        // Horizontal angle folded into the measured range by the symmetry the file implies
        let first = horizontal[0];
        let last = horizontal[horizontal_count - 1];
        let fold = |phi: f32| -> f32 {
            let phi = phi.rem_euclid(360.0);
            if last <= 90.0 && first >= 0.0 && horizontal_count > 1 {
                // Quadrants
                let p = phi % 180.0;
                if p > 90.0 {
                    180.0 - p
                } else {
                    p
                }
            } else if first >= 90.0 && last <= 270.0 {
                // Symmetric to the plane 90°–270°
                if phi < 90.0 {
                    180.0 - phi
                } else if phi > 270.0 {
                    540.0 - phi
                } else {
                    phi
                }
            } else if last <= 180.0 {
                // Symmetric to the plane 0°–180°
                if phi > 180.0 {
                    360.0 - phi
                } else {
                    phi
                }
            } else {
                phi
            }
        };
        let at = |phi: f32, v: f32| -> f32 {
            if horizontal_count == 1 {
                return in_plane(0, v);
            }
            let phi = fold(phi).clamp(first, last);
            let i = horizontal
                .partition_point(|a| *a <= phi)
                .clamp(1, horizontal_count - 1);
            let t =
                ((phi - horizontal[i - 1]) / (horizontal[i] - horizontal[i - 1])).clamp(0.0, 1.0);
            in_plane(i - 1, v) * (1.0 - t) + in_plane(i, v) * t
        };
        let horizontal_steps = if horizontal_count == 1 {
            1
        } else {
            HORIZONTAL_STEPS
        };
        let mut table = Vec::with_capacity(horizontal_steps * VERTICAL_STEPS);
        for h in 0..horizontal_steps {
            let phi = 360.0 * h as f32 / (HORIZONTAL_STEPS - 1) as f32;
            for v in 0..VERTICAL_STEPS {
                table.push(at(phi, v as f32).max(0.0));
            }
        }
        Ok(Self {
            table,
            horizontal_steps,
            lamp_flux: (lumens_per_lamp > 0.0).then_some(lamps * lumens_per_lamp),
        })
    }

    ///
    /// A profile that is the same all around, from intensities in cd at evenly spaced vertical angles from 0° (down) to
    /// 180° (up). Mostly for tests and simple spots.
    ///
    pub fn from_vertical(intensities: &[f32]) -> Self {
        let n = intensities.len().max(2);
        let table = (0..VERTICAL_STEPS)
            .map(|v| {
                let x = v as f32 / 180.0 * (n - 1) as f32;
                let i = (x.floor() as usize).min(n - 2);
                let t = x - i as f32;
                let at = |i: usize| intensities.get(i).copied().unwrap_or(0.0);
                at(i) * (1.0 - t) + at(i + 1) * t
            })
            .collect();
        Self {
            table,
            horizontal_steps: 1,
            lamp_flux: None,
        }
    }

    ///
    /// Luminous intensity in cd at a vertical angle (radians, 0 straight down) and horizontal angle (radians, 0 towards
    /// the front, counterclockwise seen from above).
    ///
    pub fn intensity(&self, vertical: f32, horizontal: f32) -> f32 {
        let v = (vertical.to_degrees()).clamp(0.0, 180.0);
        let v0 = (v.floor() as usize).min(VERTICAL_STEPS - 2);
        let tv = v - v0 as f32;
        let row = |h: usize| {
            let r = &self.table[h * VERTICAL_STEPS..];
            r[v0] * (1.0 - tv) + r[v0 + 1] * tv
        };
        if self.horizontal_steps == 1 {
            return row(0);
        }
        let h = horizontal.to_degrees().rem_euclid(360.0) / 5.0;
        let h0 = (h.floor() as usize).min(HORIZONTAL_STEPS - 2);
        let th = h - h0 as f32;
        row(h0) * (1.0 - th) + row(h0 + 1) * th
    }

    /// The luminous flux in lumen the distribution sends out, integrated over all directions.
    pub fn flux(&self) -> f32 {
        let steps_v = 360;
        let steps_h = if self.horizontal_steps == 1 { 1 } else { 144 };
        let dv = PI / steps_v as f32;
        let dh = 2.0 * PI / steps_h as f32;
        let mut sum = 0.0;
        for i in 0..steps_v {
            let v = (i as f32 + 0.5) * dv;
            for j in 0..steps_h {
                let h = (j as f32 + 0.5) * dh;
                sum += self.intensity(v, h) * v.sin() * dv * dh;
            }
        }
        sum
    }

    /// Lumen of the lamps as the file states, `None` for absolute photometry.
    pub fn lamp_flux(&self) -> Option<f32> {
        self.lamp_flux
    }

    /// The table, rows of [VERTICAL_STEPS] values, and the number of rows.
    pub(crate) fn table(&self) -> (&[f32], usize) {
        (&self.table, self.horizontal_steps)
    }
}

///
/// The angles of a direction from a luminaire: vertical from `down`, horizontal around it from `front` towards the
/// left seen from above.
///
pub(crate) fn luminaire_angles(direction: Vec3, down: Vec3, front: Vec3) -> (f32, f32) {
    let up = -down;
    let left = up.cross(front);
    let vertical = direction.dot(down).clamp(-1.0, 1.0).acos();
    let horizontal = direction.dot(left).atan2(direction.dot(front));
    (vertical, horizontal)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A downlight measured in two C-planes, bilateral: brighter towards the front than to the side.
    const DOWNLIGHT: &str = "IESNA:LM-63-2002
[TEST] test
[MANUFAC] none
TILT=NONE
1 1000 1.0 5 3 1 2 0.1 0.1 0.0
1.0 1.0 20
0 22.5 45 67.5 90
0 90 180
400 380 300 100 0
400 300 200 50 0
400 380 300 100 0
";

    #[test]
    fn reads_and_interpolates() {
        let p = IesProfile::parse(DOWNLIGHT).unwrap();
        assert_eq!(p.lamp_flux(), Some(1000.0));
        assert!((p.intensity(0.0, 0.0) - 400.0).abs() < 1e-3);
        // Halfway between 0° and 22.5° in the front plane
        let v = 11.25f32.to_radians();
        assert!(
            (p.intensity(v, 0.0) - 390.0).abs() < 0.5,
            "{}",
            p.intensity(v, 0.0)
        );
        // The side plane, and the other side by the symmetry to the plane 0°–180°
        let v = 45f32.to_radians();
        assert!((p.intensity(v, 90f32.to_radians()) - 200.0).abs() < 1e-3);
        assert!((p.intensity(v, 270f32.to_radians()) - 200.0).abs() < 1e-3);
        // Nothing above the measured range
        assert_eq!(p.intensity(120f32.to_radians(), 0.0), 0.0);
    }

    #[test]
    fn a_uniform_profile_sends_four_pi_times_its_intensity() {
        let p = IesProfile::from_vertical(&[100.0, 100.0]);
        assert!((p.flux() - 4.0 * PI * 100.0).abs() < 1.0, "{}", p.flux());
    }

    #[test]
    fn angles_of_a_luminaire() {
        let down = vec3(0.0, -1.0, 0.0);
        let front = vec3(1.0, 0.0, 0.0);
        let (v, h) = luminaire_angles(vec3(0.0, -1.0, 0.0), down, front);
        assert!(v.abs() < 1e-6 && h.abs() < 1e-6);
        // Left seen from above (Y up): from +X counterclockwise to -Z
        let (v, h) = luminaire_angles(vec3(0.0, 0.0, -1.0), down, front);
        assert!(
            (v - PI / 2.0).abs() < 1e-6 && (h - PI / 2.0).abs() < 1e-6,
            "{v} {h}"
        );
    }

    #[test]
    fn rejects_what_it_can_not_read() {
        assert!(IesProfile::parse("TILT=INCLUDE\n1").is_err());
        assert!(IesProfile::parse(&DOWNLIGHT.replace("1 2 0.1", "2 2 0.1")).is_err());
        assert!(IesProfile::parse(&DOWNLIGHT[..DOWNLIGHT.len() - 20]).is_err());
    }
}
