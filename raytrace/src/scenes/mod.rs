//!
//! Scenes that show what the tracer can do, built in code without files: a garden in the sun, a pergola in the haze,
//! an aquarium, glass with caustics and downlights with a measured profile. Each comes as a [DemoScene] with a camera
//! and an exposure that fit it, for example to show them in an application and render them for print with
//! `FinalRender`.
//!

use crate::*;

pub mod aquarium;
pub mod caustics;
pub mod downlights;
pub mod garden;
pub mod pergola;

///
/// A scene with a camera and an exposure that fit it.
///
#[derive(Clone)]
pub struct DemoScene {
    /// Short name without spaces, for example for files
    pub name: &'static str,
    /// Name to show
    pub title: &'static str,
    /// One or two sentences about what it shows
    pub description: &'static str,
    /// The scene itself
    pub scene: RtScene,
    /// Where the camera is, Y up
    pub eye: Vec3,
    /// What the camera looks at
    pub target: Vec3,
    /// Vertical field of view
    pub field_of_view: Degrees,
    /// Nearest distance the camera sees
    pub z_near: f32,
    /// Farthest distance the camera sees
    pub z_far: f32,
    /// Exposure that fits the light of the scene
    pub tone_mapping: ToneMapping,
    /// Bounces and caustic photons the scene needs; samples per call, seed and view region are left at their defaults
    pub settings: TraceSettings,
}

impl DemoScene {
    /// The camera for an image of this size.
    pub fn camera(&self, width: u32, height: u32) -> Camera {
        Camera::new_perspective(
            Viewport::new_at_origo(width, height),
            self.eye,
            self.target,
            vec3(0.0, 1.0, 0.0),
            self.field_of_view,
            self.z_near,
            self.z_far,
        )
    }

    ///
    /// The settings of a preview changed for this scene: exposure, bounces and caustic photons (on the CPU a tenth of
    /// the GPU's).
    ///
    #[cfg(feature = "three-d")]
    pub fn preview_settings(&self, settings: PreviewSettings) -> PreviewSettings {
        PreviewSettings {
            tone_mapping: self.tone_mapping,
            max_bounces: self.settings.max_bounces,
            gpu_caustic_photons: self.settings.caustic_photons,
            cpu_caustic_photons: self.settings.caustic_photons / 10,
            ..settings
        }
    }
}

/// All the scenes, in the order to show them.
pub fn all() -> Vec<DemoScene> {
    vec![
        garden::demo(),
        pergola::demo(),
        aquarium::demo(),
        caustics::demo(),
        downlights::demo(),
    ]
}
