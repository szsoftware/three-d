//!
//! Different types of buffers used for sending data (primarily geometry data) to the GPU.
//!
mod element_buffer;
use std::marker::PhantomData;

#[doc(inline)]
pub use element_buffer::*;

mod vertex_buffer;
#[doc(inline)]
pub use vertex_buffer::*;

mod instance_buffer;
#[doc(inline)]
pub use instance_buffer::*;

mod uniform_buffer;
#[doc(inline)]
pub use uniform_buffer::*;

use crate::core::*;
use data_type::*;

/// The basic data type used for each element in a [VertexBuffer] or [InstanceBuffer].
pub trait BufferDataType: DataType {}
impl BufferDataType for u8 {}
impl BufferDataType for u16 {}
impl BufferDataType for u32 {}
impl BufferDataType for i8 {}
impl BufferDataType for i16 {}
impl BufferDataType for i32 {}
impl BufferDataType for f16 {}
impl BufferDataType for f32 {}

impl<T: BufferDataType + PrimitiveDataType> BufferDataType for Vector2<T> {}
impl<T: BufferDataType + PrimitiveDataType> BufferDataType for Vector3<T> {}
impl<T: BufferDataType + PrimitiveDataType> BufferDataType for Vector4<T> {}
impl<T: BufferDataType + PrimitiveDataType> BufferDataType for [T; 2] {}
impl<T: BufferDataType + PrimitiveDataType> BufferDataType for [T; 3] {}
impl<T: BufferDataType + PrimitiveDataType> BufferDataType for [T; 4] {}

impl BufferDataType for Quat {}

struct Buffer<T: BufferDataType> {
    context: Context,
    id: backend::BufferId,
    attribute_count: u32,
    _d: PhantomData<T>,
}

impl<T: BufferDataType> Buffer<T> {
    pub fn new(context: &Context) -> Self {
        Self {
            context: context.clone(),
            id: context.backend().create_buffer(),
            attribute_count: 0,
            _d: PhantomData,
        }
    }

    pub fn new_with_data(context: &Context, data: &[T]) -> Self {
        let mut buffer = Self::new(context);
        if !data.is_empty() {
            buffer.fill(data);
        }
        buffer
    }

    pub fn fill(&mut self, data: &[T]) {
        self.context.backend().write_buffer(
            self.id,
            backend::BufferKind::Vertex(backend::VertexFormat::of::<T>()),
            to_byte_slice(data),
            if self.attribute_count > 0 {
                backend::BufferUsage::Dynamic
            } else {
                backend::BufferUsage::Static
            },
        );
        self.attribute_count = data.len() as u32;
    }

    pub fn fill_subset(&mut self, offset: u32, data: &[T]) {
        self.context.backend().write_buffer_subset(
            self.id,
            backend::BufferKind::Vertex(backend::VertexFormat::of::<T>()),
            offset,
            to_byte_slice(data),
        );
        self.attribute_count = (offset + data.len() as u32).max(self.attribute_count);
    }

    pub fn attribute_count(&self) -> u32 {
        self.attribute_count
    }

    pub fn id(&self) -> backend::BufferId {
        self.id
    }
}

impl<T: BufferDataType> Drop for Buffer<T> {
    fn drop(&mut self) {
        self.context.backend().delete_buffer(self.id);
    }
}
