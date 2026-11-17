use crate::core::backend::{PrimitiveType, UniformType, UniformValue};
use crate::core::*;

pub trait PrimitiveDataType: DataType + Copy + Default {
    fn uniform_value_with_type(data: &[Self], type_: UniformType) -> UniformValue;
}

impl PrimitiveDataType for u8 {
    fn uniform_value_with_type(data: &[Self], type_: UniformType) -> UniformValue {
        let data = data.iter().map(|v| *v as u32).collect::<Vec<_>>();
        u32::uniform_value_with_type(&data, type_)
    }
}
impl PrimitiveDataType for u16 {
    fn uniform_value_with_type(data: &[Self], type_: UniformType) -> UniformValue {
        let data = data.iter().map(|v| *v as u32).collect::<Vec<_>>();
        u32::uniform_value_with_type(&data, type_)
    }
}
impl PrimitiveDataType for u32 {
    fn uniform_value_with_type(data: &[Self], type_: UniformType) -> UniformValue {
        UniformValue::U32(type_, data.to_vec())
    }
}
impl PrimitiveDataType for i8 {
    fn uniform_value_with_type(data: &[Self], type_: UniformType) -> UniformValue {
        let data = data.iter().map(|v| *v as i32).collect::<Vec<_>>();
        i32::uniform_value_with_type(&data, type_)
    }
}
impl PrimitiveDataType for i16 {
    fn uniform_value_with_type(data: &[Self], type_: UniformType) -> UniformValue {
        let data = data.iter().map(|v| *v as i32).collect::<Vec<_>>();
        i32::uniform_value_with_type(&data, type_)
    }
}
impl PrimitiveDataType for i32 {
    fn uniform_value_with_type(data: &[Self], type_: UniformType) -> UniformValue {
        UniformValue::I32(type_, data.to_vec())
    }
}
impl PrimitiveDataType for f16 {
    fn uniform_value_with_type(data: &[Self], type_: UniformType) -> UniformValue {
        let data = data.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
        f32::uniform_value_with_type(&data, type_)
    }
}
impl PrimitiveDataType for f32 {
    fn uniform_value_with_type(data: &[Self], type_: UniformType) -> UniformValue {
        UniformValue::F32(type_, data.to_vec())
    }
}

pub trait DataType: std::fmt::Debug + Clone {
    fn primitive_type() -> PrimitiveType;
    fn size() -> u32;
    fn normalized() -> bool {
        false
    }
    fn uniform_value(data: &[Self]) -> UniformValue;
}

impl<T: DataType> DataType for &T {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }
    fn size() -> u32 {
        T::size()
    }
    fn normalized() -> bool {
        T::normalized()
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        T::uniform_value(&data.iter().map(|v| (*v).clone()).collect::<Vec<_>>())
    }
}

impl DataType for u8 {
    fn primitive_type() -> PrimitiveType {
        PrimitiveType::U8
    }

    fn size() -> u32 {
        1
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        Self::uniform_value_with_type(data, UniformType::Value)
    }
}

impl DataType for u16 {
    fn primitive_type() -> PrimitiveType {
        PrimitiveType::U16
    }

    fn size() -> u32 {
        1
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        Self::uniform_value_with_type(data, UniformType::Value)
    }
}

impl DataType for u32 {
    fn primitive_type() -> PrimitiveType {
        PrimitiveType::U32
    }

    fn size() -> u32 {
        1
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        Self::uniform_value_with_type(data, UniformType::Value)
    }
}

impl DataType for i8 {
    fn primitive_type() -> PrimitiveType {
        PrimitiveType::I8
    }

    fn size() -> u32 {
        1
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        Self::uniform_value_with_type(data, UniformType::Value)
    }
}

impl DataType for i16 {
    fn primitive_type() -> PrimitiveType {
        PrimitiveType::I16
    }

    fn size() -> u32 {
        1
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        Self::uniform_value_with_type(data, UniformType::Value)
    }
}

impl DataType for i32 {
    fn primitive_type() -> PrimitiveType {
        PrimitiveType::I32
    }

    fn size() -> u32 {
        1
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        Self::uniform_value_with_type(data, UniformType::Value)
    }
}

impl DataType for f16 {
    fn primitive_type() -> PrimitiveType {
        PrimitiveType::F16
    }

    fn size() -> u32 {
        1
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        Self::uniform_value_with_type(data, UniformType::Value)
    }
}

impl DataType for f32 {
    fn primitive_type() -> PrimitiveType {
        PrimitiveType::F32
    }

    fn size() -> u32 {
        1
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        Self::uniform_value_with_type(data, UniformType::Value)
    }
}

impl<T: PrimitiveDataType> DataType for Vector2<T> {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        2
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data.iter().flat_map(|v| [v.x, v.y]).collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Vec2)
    }
}

impl<T: PrimitiveDataType> DataType for [T; 2] {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        2
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data.iter().flatten().copied().collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Vec2)
    }
}

impl<T: PrimitiveDataType> DataType for Vector3<T> {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        3
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data
            .iter()
            .flat_map(|v| [v.x, v.y, v.z])
            .collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Vec3)
    }
}

impl<T: PrimitiveDataType> DataType for [T; 3] {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        3
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data.iter().flatten().copied().collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Vec3)
    }
}

impl<T: PrimitiveDataType> DataType for Vector4<T> {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        4
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data
            .iter()
            .flat_map(|v| [v.x, v.y, v.z, v.w])
            .collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Vec4)
    }
}

impl<T: PrimitiveDataType> DataType for [T; 4] {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        4
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data.iter().flatten().copied().collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Vec4)
    }
}

impl<T: PrimitiveDataType> DataType for Quaternion<T> {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        4
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data
            .iter()
            .flat_map(|v| [v.v.x, v.v.y, v.v.z, v.s])
            .collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Vec4)
    }
}

impl<T: PrimitiveDataType> DataType for Matrix2<T> {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        4
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data
            .iter()
            .flat_map(|v| [v.x.x, v.x.y, v.y.x, v.y.y])
            .collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Mat2)
    }
}

impl<T: PrimitiveDataType> DataType for Matrix3<T> {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        9
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data
            .iter()
            .flat_map(|v| {
                [
                    v.x.x, v.x.y, v.x.z, v.y.x, v.y.y, v.y.z, v.z.x, v.z.y, v.z.z,
                ]
            })
            .collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Mat3)
    }
}

impl<T: PrimitiveDataType> DataType for Matrix4<T> {
    fn primitive_type() -> PrimitiveType {
        T::primitive_type()
    }

    fn size() -> u32 {
        16
    }

    fn uniform_value(data: &[Self]) -> UniformValue {
        let data = data
            .iter()
            .flat_map(|v| {
                [
                    v.x.x, v.x.y, v.x.z, v.x.w, v.y.x, v.y.y, v.y.z, v.y.w, v.z.x, v.z.y, v.z.z,
                    v.z.w, v.w.x, v.w.y, v.w.z, v.w.w,
                ]
            })
            .collect::<Vec<_>>();
        T::uniform_value_with_type(&data, UniformType::Mat4)
    }
}

pub trait DepthDataType {
    fn depth_format() -> crate::core::backend::DepthFormat;
}

impl DepthDataType for f16 {
    fn depth_format() -> crate::core::backend::DepthFormat {
        crate::core::backend::DepthFormat::Depth16
    }
}
impl DepthDataType for f24 {
    fn depth_format() -> crate::core::backend::DepthFormat {
        crate::core::backend::DepthFormat::Depth24
    }
}
impl DepthDataType for f32 {
    fn depth_format() -> crate::core::backend::DepthFormat {
        crate::core::backend::DepthFormat::Depth32F
    }
}
