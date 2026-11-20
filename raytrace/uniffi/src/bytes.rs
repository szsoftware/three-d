use crate::{invalid, RtFfiError};
use std::sync::{Arc, Mutex};

///
/// Bytes that Rust owns, for Kotlin without a copy: the common API of the cargo-uniffi plugin knows this record by its
/// name and fields and gives it `asByteBuffer()` (JVM, Android), `asUint8Array()` (web), `toByteArray()` and `copyInto()`.
/// The bytes handed out by [Upload] and [MeshUpload] may be written too: they stay where they are until the upload is
/// added to a scene.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct NativeBytes {
    /// Address of the first byte in the memory of the process (of the WebAssembly module on the web).
    pub address: u64,
    /// Number of bytes.
    pub length: u64,
}

impl NativeBytes {
    /// The bytes of a slice; valid as long as the slice stays where it is.
    pub fn of<T>(values: &[T]) -> Self {
        Self {
            address: values.as_ptr() as u64,
            length: std::mem::size_of_val(values) as u64,
        }
    }
}

///
/// Bytes of a fixed size that Kotlin writes into: an image file, an OBJ or glTF file, a texture, the transforms of many
/// instances (16 `f32` each, column major). They are allocated once and never move, so the [NativeBytes] of
/// [Upload::bytes] stay valid until the upload is handed to the scene, which takes the bytes.
///
#[derive(uniffi::Object)]
pub struct Upload {
    bytes: Mutex<Option<Vec<u8>>>,
    address: NativeBytes,
}

#[uniffi::export]
impl Upload {
    /// Space for `length` bytes, all zero.
    #[uniffi::constructor]
    pub fn new(length: u64) -> Arc<Self> {
        let bytes = vec![0u8; length as usize];
        let address = NativeBytes::of(&bytes);
        Arc::new(Self {
            bytes: Mutex::new(Some(bytes)),
            address,
        })
    }

    /// Where Kotlin writes the bytes: `asByteBuffer()` on the JVM and Android, `asUint8Array()` on the web.
    pub fn bytes(&self) -> NativeBytes {
        self.address
    }

    /// The number of bytes.
    pub fn length(&self) -> u64 {
        self.address.length
    }
}

impl Upload {
    /// Takes the bytes out, the upload is empty afterwards.
    pub(crate) fn take(&self) -> Result<Vec<u8>, RtFfiError> {
        self.bytes
            .lock()
            .unwrap()
            .take()
            .ok_or(RtFfiError::Consumed)
    }

    /// The bytes as they are, for reading without taking them (an IES file, a texture used twice).
    pub(crate) fn read<R>(&self, f: impl FnOnce(&[u8]) -> R) -> Result<R, RtFfiError> {
        self.bytes
            .lock()
            .unwrap()
            .as_deref()
            .map(f)
            .ok_or(RtFfiError::Consumed)
    }

    /// The bytes as `f32` values in the byte order of the platform.
    pub(crate) fn take_f32(&self) -> Result<Vec<f32>, RtFfiError> {
        let bytes = self.take()?;
        if bytes.len() % 4 != 0 {
            return Err(invalid(format!(
                "{} bytes are not a whole number of f32 values",
                bytes.len()
            )));
        }
        Ok(bytes
            .chunks_exact(4)
            .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    }
}

///
/// A mesh that Kotlin writes straight into the memory of Rust: positions (3 `f32` per vertex), optional normals (3 `f32`),
/// optional texture coordinates (2 `f32`) and indices (3 `u32` per triangle), each in the byte order of the platform.
/// Hand it to [Scene::add_uploaded_mesh](crate::Scene::add_uploaded_mesh) when it is written.
///
#[derive(uniffi::Object)]
pub struct MeshUpload {
    data: Mutex<Option<MeshData>>,
    positions: NativeBytes,
    normals: Option<NativeBytes>,
    uvs: Option<NativeBytes>,
    indices: NativeBytes,
    vertices: u32,
    triangles: u32,
}

pub(crate) struct MeshData {
    pub positions: Vec<f32>,
    pub normals: Option<Vec<f32>>,
    pub uvs: Option<Vec<f32>>,
    pub indices: Vec<u32>,
}

#[uniffi::export]
impl MeshUpload {
    /// Space for a mesh of `vertices` vertices and `triangles` triangles, with normals and texture coordinates if asked.
    #[uniffi::constructor]
    pub fn new(vertices: u32, triangles: u32, normals: bool, uvs: bool) -> Arc<Self> {
        let n = vertices as usize;
        let data = MeshData {
            positions: vec![0.0; 3 * n],
            normals: normals.then(|| vec![0.0; 3 * n]),
            uvs: uvs.then(|| vec![0.0; 2 * n]),
            indices: vec![0; 3 * triangles as usize],
        };
        Arc::new(Self {
            positions: NativeBytes::of(&data.positions),
            normals: data.normals.as_deref().map(NativeBytes::of),
            uvs: data.uvs.as_deref().map(NativeBytes::of),
            indices: NativeBytes::of(&data.indices),
            data: Mutex::new(Some(data)),
            vertices,
            triangles,
        })
    }

    /// The number of vertices.
    pub fn vertex_count(&self) -> u32 {
        self.vertices
    }

    /// The number of triangles.
    pub fn triangle_count(&self) -> u32 {
        self.triangles
    }

    /// x, y, z per vertex as `f32`.
    pub fn positions(&self) -> NativeBytes {
        self.positions
    }

    /// x, y, z per vertex as `f32`, if the upload has normals.
    pub fn normals(&self) -> Option<NativeBytes> {
        self.normals
    }

    /// u, v per vertex as `f32`, if the upload has texture coordinates.
    pub fn uvs(&self) -> Option<NativeBytes> {
        self.uvs
    }

    /// a, b, c per triangle as `u32`, counter clockwise seen from the front.
    pub fn indices(&self) -> NativeBytes {
        self.indices
    }
}

impl MeshUpload {
    pub(crate) fn take(&self) -> Result<MeshData, RtFfiError> {
        self.data.lock().unwrap().take().ok_or(RtFfiError::Consumed)
    }
}
