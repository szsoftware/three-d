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
        }
    }
}

impl std::error::Error for RtError {}
