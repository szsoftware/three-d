use three_d_raytrace::RtError;

/// What can go wrong behind the uniffi surface.
#[derive(Debug, Clone, PartialEq, uniffi::Error)]
pub enum RtFfiError {
    /// A mesh, material, texture or instance id that the scene does not have.
    UnknownId {
        /// What kind of id and which.
        what: String,
    },
    /// Data that does not fit: a mesh with indices out of range, a transform without 16 values, a buffer too small.
    InvalidData {
        /// Why.
        why: String,
    },
    /// A file that could not be read: an OBJ, glTF, PNG or IES file.
    Import {
        /// Why.
        why: String,
    },
    /// The GPU failed, for example lost after a driver reset.
    Gpu {
        /// Why.
        why: String,
    },
    /// An object that holds a three-d context was called from another thread than the one that created the renderer.
    WrongThread,
    /// An upload that was already handed to the scene and is empty now.
    Consumed,
}

impl std::fmt::Display for RtFfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownId { what } => write!(f, "unknown {what}"),
            Self::InvalidData { why } => write!(f, "invalid data: {why}"),
            Self::Import { why } => write!(f, "import failed: {why}"),
            Self::Gpu { why } => write!(f, "GPU: {why}"),
            Self::WrongThread => write!(
                f,
                "called from another thread than the one that created the renderer"
            ),
            Self::Consumed => write!(f, "the upload was already added to a scene"),
        }
    }
}

impl std::error::Error for RtFfiError {}

impl From<RtError> for RtFfiError {
    fn from(e: RtError) -> Self {
        match e {
            RtError::UnknownMesh(id) => Self::UnknownId {
                what: format!("mesh {id}"),
            },
            RtError::UnknownMaterial(id) => Self::UnknownId {
                what: format!("material {id}"),
            },
            RtError::UnknownInstance(id) => Self::UnknownId {
                what: format!("instance {id}"),
            },
            RtError::InvalidMesh(why) => Self::InvalidData { why },
            RtError::Ies(why) | RtError::Export(why) => Self::Import { why },
            RtError::Gpu(why) => Self::Gpu { why },
            other => Self::InvalidData {
                why: other.to_string(),
            },
        }
    }
}

pub(crate) fn invalid(why: impl Into<String>) -> RtFfiError {
    RtFfiError::InvalidData { why: why.into() }
}
