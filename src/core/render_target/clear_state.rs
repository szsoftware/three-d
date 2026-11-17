///
/// Defines which channels (red, green, blue, alpha and depth) to clear when starting to write to a [RenderTarget].
/// If `None` then the channel is not cleared and if `Some(value)` the channel is cleared to that value (the value must be between 0 and 1).
///
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ClearState {
    /// Defines the clear value for the red channel.
    pub red: Option<f32>,
    /// Defines the clear value for the green channel.
    pub green: Option<f32>,
    /// Defines the clear value for the blue channel.
    pub blue: Option<f32>,
    /// Defines the clear value for the alpha channel.
    pub alpha: Option<f32>,
    /// Defines the clear value for the depth channel. A value of 1 means a depth value equal to the far plane and 0 means a depth value equal to the near plane.
    pub depth: Option<f32>,
}

impl ClearState {
    ///
    /// Nothing will be cleared.
    ///
    pub const fn none() -> Self {
        Self {
            red: None,
            green: None,
            blue: None,
            alpha: None,
            depth: None,
        }
    }

    ///
    /// The depth will be cleared to the given value.
    ///
    pub const fn depth(depth: f32) -> Self {
        Self {
            red: None,
            green: None,
            blue: None,
            alpha: None,
            depth: Some(depth),
        }
    }

    ///
    /// The color channels (red, green, blue and alpha) will be cleared to the given values.
    ///
    pub const fn color(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self {
            red: Some(red),
            green: Some(green),
            blue: Some(blue),
            alpha: Some(alpha),
            depth: None,
        }
    }

    ///
    /// Both the color channels (red, green, blue and alpha) and depth will be cleared to the given values.
    ///
    pub const fn color_and_depth(red: f32, green: f32, blue: f32, alpha: f32, depth: f32) -> Self {
        Self {
            red: Some(red),
            green: Some(green),
            blue: Some(blue),
            alpha: Some(alpha),
            depth: Some(depth),
        }
    }
}

impl Default for ClearState {
    fn default() -> Self {
        Self::color_and_depth(0.0, 0.0, 0.0, 1.0, 1.0)
    }
}
