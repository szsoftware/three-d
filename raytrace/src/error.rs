///
/// Errors of the ray tracing crate.
///
#[derive(Debug, Clone, PartialEq)]
pub enum RtError {
    /// The mesh can not be traced, the text says why.
    InvalidMesh(String),
    /// The id does not belong to a mesh of this scene.
    UnknownMesh(u32),
    /// The id does not belong to a material of this scene.
    UnknownMaterial(u32),
    /// The id does not belong to an instance of this scene.
    UnknownInstance(u32),
    /// The render image has no pixels.
    EmptyImage,
    /// Writing the image failed, the text says why.
    Export(String),
    /// The GPU can not do what was asked, the text says why.
    Gpu(String),
    /// The render image has no features for the denoiser, see `RenderImage::with_features`.
    NoFeatures,
}

impl std::fmt::Display for RtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RtError::InvalidMesh(why) => write!(f, "invalid mesh: {why}"),
            RtError::UnknownMesh(id) => write!(f, "unknown mesh id {id}"),
            RtError::UnknownMaterial(id) => write!(f, "unknown material id {id}"),
            RtError::UnknownInstance(id) => write!(f, "unknown instance id {id}"),
            RtError::EmptyImage => write!(f, "the render image has no pixels"),
            RtError::Export(why) => write!(f, "export failed: {why}"),
            RtError::Gpu(why) => write!(f, "GPU: {why}"),
            RtError::NoFeatures => write!(f, "the render image collects no features for denoising"),
        }
    }
}

impl std::error::Error for RtError {}
