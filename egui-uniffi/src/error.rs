/// What can go wrong behind the uniffi surface.
#[derive(Debug, Clone, PartialEq, uniffi::Error)]
pub enum UiError {
    /// A tree that cannot be drawn: an id twice, a parent that is not there or holds no nodes, a panel inside a node,
    /// values that do not fit (a selected option that is not there, min above max).
    InvalidTree {
        /// Why.
        why: String,
    },
    /// Painting failed on the graphics card.
    Gpu {
        /// Why.
        why: String,
    },
    /// The overlay was painted on another thread than the first time: egui's painter belongs to the thread of its context.
    WrongThread,
    /// The overlay is painted already, called again from inside its own painting.
    Busy,
}

impl std::fmt::Display for UiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTree { why } => write!(f, "invalid tree: {why}"),
            Self::Gpu { why } => write!(f, "GPU: {why}"),
            Self::WrongThread => write!(
                f,
                "painted on another thread than the one that painted it first"
            ),
            Self::Busy => write!(f, "the overlay is painted already"),
        }
    }
}

impl std::error::Error for UiError {}

impl From<cargo_uniffi_support::ThreadBoundError> for UiError {
    fn from(e: cargo_uniffi_support::ThreadBoundError) -> Self {
        match e {
            cargo_uniffi_support::ThreadBoundError::WrongThread { .. } => Self::WrongThread,
            cargo_uniffi_support::ThreadBoundError::Busy => Self::Busy,
        }
    }
}

impl From<three_d::CoreError> for UiError {
    fn from(e: three_d::CoreError) -> Self {
        Self::Gpu { why: e.to_string() }
    }
}
