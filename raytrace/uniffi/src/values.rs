use crate::{invalid, RtFfiError};
use three_d_raytrace::*;

/// A vector or a linear color.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct V3 {
    /// x, or red
    pub x: f32,
    /// y, or green
    pub y: f32,
    /// z, or blue
    pub z: f32,
}

impl From<V3> for Vec3 {
    fn from(v: V3) -> Self {
        vec3(v.x, v.y, v.z)
    }
}

impl From<Vec3> for V3 {
    fn from(v: Vec3) -> Self {
        V3 {
            x: v.x,
            y: v.y,
            z: v.z,
        }
    }
}

/// 16 values, column major like `Mat4` of three-d, into a matrix.
pub(crate) fn mat4(values: &[f32]) -> Result<Mat4, RtFfiError> {
    if values.len() != 16 {
        return Err(invalid(format!(
            "a transform has 16 values, not {}",
            values.len()
        )));
    }
    let c = |i: usize| {
        vec4(
            values[4 * i],
            values[4 * i + 1],
            values[4 * i + 2],
            values[4 * i + 3],
        )
    };
    Ok(Mat4::from_cols(c(0), c(1), c(2), c(3)))
}

/// A participating medium inside a closed mesh or the haze of the scene.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Enum)]
pub enum Medium {
    /// Clear and colored: keeps `color` of the light after `distance` meters (water, tinted glass).
    Absorbing {
        /// The part of the light left after `distance`, per channel.
        color: V3,
        /// Meters.
        distance: f32,
    },
    /// Fog or smoke: half the light is left after `half_distance` meters, `albedo` of what is lost is scattered,
    /// `anisotropy` above 0 scatters forward.
    Fog {
        /// Meters.
        half_distance: f32,
        /// From 0 to 1.
        albedo: f32,
        /// From -1 to 1.
        anisotropy: f32,
    },
    /// The coefficients themselves, per meter.
    Coefficients {
        /// Absorption per meter.
        absorption: V3,
        /// Scattering per meter.
        scattering: V3,
        /// From -1 to 1.
        anisotropy: f32,
    },
}

impl From<Medium> for RtMedium {
    fn from(m: Medium) -> Self {
        match m {
            Medium::Absorbing { color, distance } => RtMedium::absorbing(color.into(), distance),
            Medium::Fog {
                half_distance,
                albedo,
                anisotropy,
            } => RtMedium::fog(half_distance, albedo, anisotropy),
            Medium::Coefficients {
                absorption,
                scattering,
                anisotropy,
            } => RtMedium {
                absorption: absorption.into(),
                scattering: scattering.into(),
                anisotropy,
            },
        }
    }
}

/// A physically based material, see `RtMaterial`; textures are ids from `Scene::add_texture`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Material {
    /// Linear base color.
    pub base_color: V3,
    /// Opacity, used with `alpha_cutout`.
    #[uniffi(default = 1.0)]
    pub alpha: f32,
    /// 0 for non-metals, 1 for metals.
    #[uniffi(default = 0.0)]
    pub metallic: f32,
    /// 0 is a mirror, 1 matte.
    #[uniffi(default = 0.5)]
    pub roughness: f32,
    /// Strength of the specular reflection of non-metals, 1 is the usual 4 %, 0 purely diffuse.
    #[uniffi(default = 1.0)]
    pub specular: f32,
    /// Emitted luminance in cd/m² per channel.
    pub emission: V3,
    /// Part of the light that passes through, from 0 to 1.
    #[uniffi(default = 0.0)]
    pub transmission: f32,
    /// Index of refraction, 1.5 for glass.
    #[uniffi(default = 1.5)]
    pub index_of_refraction: f32,
    /// Cut out where the base color texture's alpha is below this.
    #[uniffi(default = None)]
    pub alpha_cutout: Option<f32>,
    /// A thin pane instead of a body with an inside.
    #[uniffi(default = true)]
    pub thin_walled: bool,
    /// The medium inside a closed mesh that is not thin walled.
    #[uniffi(default = None)]
    pub medium: Option<Medium>,
    /// Which medium wins where two overlap, higher wins.
    #[uniffi(default = 0)]
    pub medium_priority: u32,
    /// Base color texture (sRGB), multiplies the base color.
    #[uniffi(default = None)]
    pub base_color_texture: Option<u32>,
    /// Roughness in green and metallic in blue, like glTF.
    #[uniffi(default = None)]
    pub metallic_roughness_texture: Option<u32>,
    /// Tangent space normal map.
    #[uniffi(default = None)]
    pub normal_texture: Option<u32>,
    /// Emission texture (sRGB), multiplies the emission.
    #[uniffi(default = None)]
    pub emission_texture: Option<u32>,
}

impl Material {
    pub(crate) fn to_rt(&self, textures: u32) -> Result<RtMaterial, RtFfiError> {
        let texture = |id: Option<u32>| match id {
            Some(id) if id >= textures => Err(RtFfiError::UnknownId {
                what: format!("texture {id}"),
            }),
            Some(id) => Ok(Some(TextureId(id))),
            None => Ok(None),
        };
        Ok(RtMaterial {
            base_color: self.base_color.into(),
            alpha: self.alpha,
            metallic: self.metallic,
            roughness: self.roughness,
            specular: self.specular,
            emission: self.emission.into(),
            transmission: self.transmission,
            index_of_refraction: self.index_of_refraction,
            alpha_cutout: self.alpha_cutout,
            thin_walled: self.thin_walled,
            medium: self.medium.map(Into::into),
            medium_priority: self.medium_priority,
            base_color_texture: texture(self.base_color_texture)?,
            metallic_roughness_texture: texture(self.metallic_roughness_texture)?,
            normal_texture: texture(self.normal_texture)?,
            emission_texture: texture(self.emission_texture)?,
            ..Default::default()
        })
    }

    pub(crate) fn from_rt(m: &RtMaterial) -> Self {
        Self {
            base_color: m.base_color.into(),
            alpha: m.alpha,
            metallic: m.metallic,
            roughness: m.roughness,
            specular: m.specular,
            emission: m.emission.into(),
            transmission: m.transmission,
            index_of_refraction: m.index_of_refraction,
            alpha_cutout: m.alpha_cutout,
            thin_walled: m.thin_walled,
            medium: m.medium.map(|m| Medium::Coefficients {
                absorption: m.absorption.into(),
                scattering: m.scattering.into(),
                anisotropy: m.anisotropy,
            }),
            medium_priority: m.medium_priority,
            base_color_texture: m.base_color_texture.map(|t| t.0),
            metallic_roughness_texture: m.metallic_roughness_texture.map(|t| t.0),
            normal_texture: m.normal_texture.map(|t| t.0),
            emission_texture: m.emission_texture.map(|t| t.0),
        }
    }
}

/// A light source besides the sky and glowing surfaces, see `RtLight`.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum Light {
    /// Parallel light, hard shadows: `direction` is where the light travels.
    Directional {
        /// Where the light goes.
        direction: V3,
        /// Linear color.
        color: V3,
        /// Lux on a surface facing the light.
        illuminance: f32,
    },
    /// A point that sends out `flux` lumen in all directions.
    Point {
        /// World position.
        position: V3,
        /// Linear color.
        color: V3,
        /// Lumen.
        flux: f32,
    },
    /// A glowing ball of `radius` that sends out `flux` lumen, soft shadows.
    Sphere {
        /// World position.
        center: V3,
        /// Meters.
        radius: f32,
        /// Linear color.
        color: V3,
        /// Lumen.
        flux: f32,
    },
    /// A glowing parallelogram with a luminance in cd/m², shining to `edge_u × edge_v`.
    Rect {
        /// One corner.
        corner: V3,
        /// First edge from the corner.
        edge_u: V3,
        /// Second edge from the corner.
        edge_v: V3,
        /// Linear color.
        color: V3,
        /// cd/m².
        luminance: f32,
        /// Shines from both sides.
        two_sided: bool,
    },
    /// A luminaire with a measured distribution, `flux` lumen or the profile's own intensities if `None`.
    Luminaire {
        /// The profile, from `parse_ies`.
        profile: std::sync::Arc<crate::Ies>,
        /// World position.
        position: V3,
        /// Direction of the profile's 0°.
        down: V3,
        /// Direction of C0.
        front: V3,
        /// Linear color.
        color: V3,
        /// Lumen, `None` for the profile as measured.
        flux: Option<f32>,
    },
}

impl From<Light> for RtLight {
    fn from(l: Light) -> Self {
        match l {
            Light::Directional {
                direction,
                color,
                illuminance,
            } => RtLight::Directional {
                direction: Vec3::from(direction).normalize(),
                color: color.into(),
                illuminance,
            },
            Light::Point {
                position,
                color,
                flux,
            } => RtLight::Point {
                position: position.into(),
                color: color.into(),
                // A point that sends `flux` lumen evenly: flux / 4π candela
                intensity: flux / (4.0 * std::f32::consts::PI),
            },
            Light::Sphere {
                center,
                radius,
                color,
                flux,
            } => RtLight::sphere_with_flux(center.into(), radius, color.into(), flux),
            Light::Rect {
                corner,
                edge_u,
                edge_v,
                color,
                luminance,
                two_sided,
            } => RtLight::Rect {
                corner: corner.into(),
                edge_u: edge_u.into(),
                edge_v: edge_v.into(),
                color: color.into(),
                luminance,
                two_sided,
            },
            Light::Luminaire {
                profile,
                position,
                down,
                front,
                color,
                flux,
            } => {
                let light = match flux {
                    Some(flux) => RtLight::luminaire_with_flux(
                        position.into(),
                        down.into(),
                        front.into(),
                        profile.profile.clone(),
                        flux,
                    ),
                    None => RtLight::luminaire(
                        position.into(),
                        down.into(),
                        front.into(),
                        profile.profile.clone(),
                    ),
                };
                match light {
                    RtLight::Luminaire {
                        position,
                        down,
                        front,
                        profile,
                        scale,
                        ..
                    } => RtLight::Luminaire {
                        position,
                        down,
                        front,
                        color: color.into(),
                        profile,
                        scale,
                    },
                    other => other,
                }
            }
        }
    }
}

/// A daylight sky with the sun for a place and a local time.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SkySpec {
    /// Degrees north.
    pub latitude: f64,
    /// Degrees east.
    pub longitude: f64,
    /// Year.
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    /// 1 to 31.
    pub day: u32,
    /// 0 to 23.
    pub hour: u32,
    /// 0 to 59.
    #[uniffi(default = 0)]
    pub minute: u32,
    /// The time zone of the local time, 2 for summer time in Germany.
    #[uniffi(default = 0.0)]
    pub utc_offset_hours: f64,
    /// Where north is in the scene, -Z by default.
    pub north: V3,
    /// Haze of the air: 2 clear, 3 normal, 6 hazy.
    #[uniffi(default = 3.0)]
    pub turbidity: f32,
    /// Linear color of the ground below the horizon.
    pub ground_albedo: V3,
}

impl From<&SkySpec> for Sky {
    fn from(s: &SkySpec) -> Self {
        let orientation = SceneOrientation {
            north: Vec3::from(s.north).normalize(),
            ..Default::default()
        };
        Sky::new(
            GeoLocation::new(s.latitude, s.longitude),
            LocalDateTime::new(s.year, s.month, s.day, s.hour, s.minute, s.utc_offset_hours),
            orientation,
        )
        .with_turbidity(s.turbidity)
        .with_ground_albedo(s.ground_albedo.into())
    }
}

/// Haze over the whole scene, thinning out with height.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Fog {
    /// The medium at `height`.
    pub medium: Medium,
    /// Meters, y up.
    pub height: f32,
    /// Meters over which the density falls by e upwards.
    pub scale_height: f32,
}

impl From<Fog> for RtFog {
    fn from(f: Fog) -> Self {
        RtFog {
            medium: f.medium.into(),
            height: f.height,
            scale_height: f.scale_height,
        }
    }
}

/// A perspective camera, Y up, in meters.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct CameraSpec {
    /// Where the camera is.
    pub eye: V3,
    /// What it looks at.
    pub target: V3,
    /// Up, usually 0, 1, 0.
    pub up: V3,
    /// Vertical field of view in degrees.
    pub fov_degrees: f32,
    /// Nearest distance seen.
    #[uniffi(default = 0.05)]
    pub near: f32,
    /// Farthest distance seen.
    #[uniffi(default = 500.0)]
    pub far: f32,
    /// Aperture for depth of field, `None` for everything sharp.
    #[uniffi(default = None)]
    pub f_number: Option<f32>,
    /// Distance of the sharp plane, the target's distance if `None`.
    #[uniffi(default = None)]
    pub focus_distance: Option<f32>,
}

impl CameraSpec {
    pub(crate) fn camera(&self, width: u32, height: u32) -> Camera {
        Camera::new_perspective(
            Viewport::new_at_origo(width.max(1), height.max(1)),
            self.eye.into(),
            self.target.into(),
            self.up.into(),
            degrees(self.fov_degrees),
            self.near,
            self.far,
        )
    }

    pub(crate) fn depth_of_field(&self) -> Option<DepthOfField> {
        let f_number = self.f_number?;
        let focus_distance = self
            .focus_distance
            .unwrap_or_else(|| (Vec3::from(self.target) - Vec3::from(self.eye)).magnitude());
        Some(DepthOfField {
            f_number,
            focus_distance,
        })
    }

    pub(crate) fn from_camera(camera: &Camera) -> Self {
        let fov = match camera.projection_type() {
            three_d_asset::ProjectionType::Perspective { field_of_view_y } => {
                Deg::from(*field_of_view_y).0
            }
            _ => 50.0,
        };
        Self {
            eye: camera.position().into(),
            target: camera.target().into(),
            up: camera.up().into(),
            fov_degrees: fov,
            near: camera.z_near(),
            far: camera.z_far(),
            f_number: None,
            focus_distance: None,
        }
    }
}

/// The curve that compresses bright values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ToneCurve {
    /// The filmic curve of ACES.
    Aces,
    /// x / (1 + x).
    Reinhard,
    /// Cut off at one.
    Clamp,
}

/// How an image of the scene is exposed and traced.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct Look {
    /// EV100: about 15 for a sunny day, 11 overcast, 7 a lit room.
    pub exposure_ev100: f32,
    /// The tone curve.
    pub tone: ToneCurve,
    /// Most bounces of a path.
    #[uniffi(default = 8)]
    pub max_bounces: u32,
    /// Photons per pass for caustics under solid glass and water, 0 without caustics.
    #[uniffi(default = 0)]
    pub caustic_photons: u32,
    /// Brightest value a sample may add after the first bounce, against fireflies.
    #[uniffi(default = None)]
    pub indirect_clamp: Option<f32>,
    /// Changes the random numbers.
    #[uniffi(default = 0)]
    pub seed: u32,
    /// Denoises the final image.
    #[uniffi(default = true)]
    pub denoise: bool,
}

impl Look {
    pub(crate) fn tone_mapping(&self) -> ToneMapping {
        ToneMapping {
            exposure_ev100: self.exposure_ev100,
            operator: match self.tone {
                ToneCurve::Aces => ToneOperator::Aces,
                ToneCurve::Reinhard => ToneOperator::Reinhard,
                ToneCurve::Clamp => ToneOperator::Clamp,
            },
        }
    }

    pub(crate) fn settings(&self, camera: &CameraSpec) -> TraceSettings {
        TraceSettings {
            max_bounces: self.max_bounces,
            caustic_photons: self.caustic_photons,
            indirect_clamp: self.indirect_clamp,
            seed: self.seed,
            depth_of_field: camera.depth_of_field(),
            ..Default::default()
        }
    }

    pub(crate) fn from_demo(tone: &ToneMapping, settings: &TraceSettings) -> Self {
        Self {
            exposure_ev100: tone.exposure_ev100,
            tone: match tone.operator {
                ToneOperator::Aces => ToneCurve::Aces,
                ToneOperator::Reinhard => ToneCurve::Reinhard,
                ToneOperator::Clamp => ToneCurve::Clamp,
            },
            max_bounces: settings.max_bounces,
            caustic_photons: settings.caustic_photons,
            indirect_clamp: settings.indirect_clamp,
            seed: settings.seed,
            denoise: true,
        }
    }
}

/// The meshes every scene has at hand, in three-d's sizes: cube and sphere from -1 to 1, square from -1 to 1 in x and
/// y facing +z, cylinder along x from 0 to 1 with radius 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Builtin {
    /// `CpuMesh::cube()`
    Cube,
    /// `CpuMesh::sphere(subdivisions)`
    Sphere,
    /// `CpuMesh::square()`
    Square,
    /// `CpuMesh::cylinder(subdivisions)`
    Cylinder,
}
