//!
//! Depth of field: rays from a lens of finite size instead of a pinhole.
//!

use crate::sampling::concentric_disc;
use three_d_asset::prelude::*;
use three_d_asset::{Camera, ProjectionType};

/// Height of the sensor the field of view of a camera is taken for: 24 mm, a full frame camera in landscape.
const SENSOR_HEIGHT: f32 = 0.024;

///
/// The lens of a photo camera, for depth of field of perspective cameras: points at the focus distance are sharp, the
/// blur grows in front of and behind them like in a photo with the same field of view, f-number and a full frame
/// sensor. Needs the scene in meters. Orthographic cameras stay sharp everywhere.
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthOfField {
    /// Focal length divided by the diameter of the aperture, like f/2.8 on a lens. Smaller is blurrier.
    pub f_number: f32,
    /// Distance from the camera to the sharp plane along the view direction, in meters.
    pub focus_distance: f32,
}

impl DepthOfField {
    /// Focused on the plane through `point`, perpendicular to the camera's view direction.
    pub fn focused_on(camera: &Camera, point: Vec3, f_number: f32) -> Self {
        Self {
            f_number,
            focus_distance: (point - camera.position()).dot(camera.view_direction()),
        }
    }

    ///
    /// Radius of the aperture in meters for this camera: the focal length of a full frame sensor with the camera's
    /// vertical field of view, divided by twice the f-number. 0 for orthographic cameras.
    ///
    pub fn aperture_radius(&self, camera: &Camera) -> f32 {
        match camera.projection_type() {
            ProjectionType::Perspective { field_of_view_y } if self.f_number > 0.0 => {
                let half_angle: f32 = 0.5 * field_of_view_y.0;
                let focal_length = 0.5 * SENSOR_HEIGHT / half_angle.tan();
                focal_length / (2.0 * self.f_number)
            }
            _ => 0.0,
        }
    }
}

/// The lens of one camera, in world space.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lens {
    /// Right and up of the camera, as long as the aperture's radius.
    pub u: Vec3,
    pub v: Vec3,
    pub view: Vec3,
    pub focus_distance: f32,
}

impl Lens {
    /// The lens if it blurs anything.
    pub(crate) fn of(camera: &Camera, depth_of_field: Option<DepthOfField>) -> Option<Self> {
        let dof = depth_of_field?;
        let radius = dof.aperture_radius(camera);
        (radius > 0.0 && dof.focus_distance > 0.0).then(|| Self {
            u: camera.right_direction().normalize() * radius,
            v: camera.up_orthogonal().normalize() * radius,
            view: camera.view_direction().normalize(),
            focus_distance: dof.focus_distance,
        })
    }

    /// The ray through the point of the lens at `u1`, `u2` towards where the pinhole ray meets the focus plane.
    pub(crate) fn ray(&self, origin: Vec3, direction: Vec3, u1: f32, u2: f32) -> (Vec3, Vec3) {
        let focus = origin + direction * (self.focus_distance / direction.dot(self.view));
        let (x, y) = concentric_disc(u1, u2);
        let start = origin + self.u * x + self.v * y;
        (start, (focus - start).normalize())
    }
}
