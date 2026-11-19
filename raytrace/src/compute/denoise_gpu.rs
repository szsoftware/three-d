//!
//! The à-trous denoiser on the GPU, see denoise.wgsl.
//!

use super::{copy_rows_to_texture, WORKGROUP};
use crate::{AtrousDenoiser, GpuDevice};
use three_d::wgpu as w;

/// Bytes of the parameters of one pass.
pub(super) const PARAMS_BYTES: u64 = 48;

/// The entry points of the passes with `read_only` of every storage buffer after the parameters. The browser compiles
/// the same pipelines ahead, see browser.rs.
pub(super) const PASSES: [(&str, &[bool]); 4] = [
    ("prepare", &[true, false, false, false]),
    ("guide", &[true, true, false, false]),
    ("atrous", &[true, true, true, false]),
    ("finish", &[true, true, false]),
];

/// Pipelines and intermediate buffers of the GPU denoiser for one image size.
#[derive(Debug)]
pub(super) struct GpuDenoise {
    width: u32,
    height: u32,
    pitch: u32,
    prepare: Pass,
    guide: Pass,
    atrous: Pass,
    finish: Pass,
    /// One entry of parameters per pass, at offsets aligned for dynamic offsets.
    params: w::Buffer,
    stride: u64,
    light: [w::Buffer; 2],
    normal_distance: w::Buffer,
    albedo_gradient: w::Buffer,
    averages: w::Buffer,
}

/// A pipeline with its layout: parameters with a dynamic offset, then storage buffers.
#[derive(Debug)]
struct Pass {
    layout: w::BindGroupLayout,
    pipeline: w::ComputePipeline,
}

/// Most passes of one run: prepare and guide share the first entry, then the atrous passes, then finish.
const MAX_ITERATIONS: u32 = 16;

impl GpuDenoise {
    pub(super) fn new(device: &w::Device, width: u32, height: u32) -> Self {
        let module = device.create_shader_module(w::ShaderModuleDescriptor {
            label: Some("denoise"),
            source: w::ShaderSource::Wgsl(include_str!("denoise.wgsl").into()),
        });
        // `read_only` of every storage buffer after the parameters
        let pipeline = |entry: &str, read_only: &[bool]| {
            let mut entries = vec![w::BindGroupLayoutEntry {
                binding: 0,
                visibility: w::ShaderStages::COMPUTE,
                ty: w::BindingType::Buffer {
                    ty: w::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: w::BufferSize::new(PARAMS_BYTES),
                },
                count: None,
            }];
            for (i, &read_only) in read_only.iter().enumerate() {
                entries.push(w::BindGroupLayoutEntry {
                    binding: i as u32 + 1,
                    visibility: w::ShaderStages::COMPUTE,
                    ty: w::BindingType::Buffer {
                        ty: w::BufferBindingType::Storage { read_only },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                });
            }
            let layout = device.create_bind_group_layout(&w::BindGroupLayoutDescriptor {
                label: Some("denoise"),
                entries: &entries,
            });
            let pipeline_layout = device.create_pipeline_layout(&w::PipelineLayoutDescriptor {
                label: Some("denoise"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_compute_pipeline(&w::ComputePipelineDescriptor {
                label: Some("denoise"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            });
            Pass { layout, pipeline }
        };
        let pixels = width.max(1) as u64 * height.max(1) as u64;
        let storage = |label| {
            device.create_buffer(&w::BufferDescriptor {
                label: Some(label),
                size: pixels * 16,
                usage: w::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        let pitch = width.div_ceil(16) * 16;
        let stride = PARAMS_BYTES.max(device.limits().min_uniform_buffer_offset_alignment as u64);
        Self {
            width,
            height,
            pitch,
            prepare: pipeline(PASSES[0].0, PASSES[0].1),
            guide: pipeline(PASSES[1].0, PASSES[1].1),
            atrous: pipeline(PASSES[2].0, PASSES[2].1),
            finish: pipeline(PASSES[3].0, PASSES[3].1),
            params: device.create_buffer(&w::BufferDescriptor {
                label: Some("denoise parameters"),
                size: stride * (MAX_ITERATIONS as u64 + 1),
                usage: w::BufferUsages::UNIFORM | w::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            stride,
            light: [storage("denoise light"), storage("denoise light")],
            normal_distance: storage("denoise normals"),
            albedo_gradient: storage("denoise albedo"),
            averages: device.create_buffer(&w::BufferDescriptor {
                label: Some("denoised"),
                size: pitch as u64 * height.max(1) as u64 * 16,
                usage: w::BufferUsages::STORAGE | w::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
        }
    }

    /// Denoises the sums (three vec4 per pixel) and writes the result into `target`.
    pub(super) fn run(
        &self,
        gpu: &GpuDevice,
        sums: &w::Buffer,
        samples: u32,
        denoiser: &AtrousDenoiser,
        target: &w::Texture,
    ) {
        let iterations = denoiser.iterations.min(MAX_ITERATIONS);
        // Entry 0 for prepare, guide and finish, entry 1 + i for atrous pass i
        let mut data = vec![0u8; self.params.size() as usize];
        for entry in 0..=iterations {
            let step = if entry == 0 { 0 } else { 1u32 << (entry - 1) };
            let words: [u32; 12] = [
                self.width,
                self.height,
                self.pitch,
                0,
                step,
                samples,
                u32::from(samples < 4),
                0,
                denoiser.sigma_luminance.to_bits(),
                denoiser.sigma_normal.to_bits(),
                denoiser.sigma_distance.to_bits(),
                0,
            ];
            let at = (entry as u64 * self.stride) as usize;
            for (i, word) in words.iter().enumerate() {
                data[at + 4 * i..at + 4 * i + 4].copy_from_slice(&word.to_le_bytes());
            }
        }
        gpu.queue.write_buffer(&self.params, 0, &data);

        let device = &gpu.device;
        let params = w::BindingResource::Buffer(w::BufferBinding {
            buffer: &self.params,
            offset: 0,
            size: w::BufferSize::new(PARAMS_BYTES),
        });
        let bind = |pass: &Pass, buffers: &[&w::Buffer]| {
            let mut entries = vec![w::BindGroupEntry {
                binding: 0,
                resource: params.clone(),
            }];
            for (i, b) in buffers.iter().enumerate() {
                entries.push(w::BindGroupEntry {
                    binding: i as u32 + 1,
                    resource: b.as_entire_binding(),
                });
            }
            device.create_bind_group(&w::BindGroupDescriptor {
                label: Some("denoise"),
                layout: &pass.layout,
                entries: &entries,
            })
        };
        let [light_a, light_b] = &self.light;
        let prepare = bind(
            &self.prepare,
            &[sums, light_a, &self.normal_distance, &self.albedo_gradient],
        );
        let guide = bind(
            &self.guide,
            &[
                light_a,
                &self.normal_distance,
                &self.albedo_gradient,
                light_b,
            ],
        );
        // The guided light is in b; the passes go b → a → b ...
        let atrous = [
            bind(
                &self.atrous,
                &[
                    light_b,
                    &self.normal_distance,
                    &self.albedo_gradient,
                    light_a,
                ],
            ),
            bind(
                &self.atrous,
                &[
                    light_a,
                    &self.normal_distance,
                    &self.albedo_gradient,
                    light_b,
                ],
            ),
        ];
        let result = if iterations % 2 == 1 {
            light_a
        } else {
            light_b
        };
        let finish = bind(
            &self.finish,
            &[result, &self.albedo_gradient, &self.averages],
        );

        let mut encoder = device.create_command_encoder(&w::CommandEncoderDescriptor {
            label: Some("denoise"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&w::ComputePassDescriptor {
                label: Some("denoise"),
                timestamp_writes: None,
            });
            let groups = (
                self.width.div_ceil(WORKGROUP),
                self.height.div_ceil(WORKGROUP),
            );
            let mut run = |p: &Pass, group: &w::BindGroup, entry: u32| {
                pass.set_pipeline(&p.pipeline);
                pass.set_bind_group(0, group, &[(entry as u64 * self.stride) as u32]);
                pass.dispatch_workgroups(groups.0, groups.1, 1);
            };
            run(&self.prepare, &prepare, 0);
            run(&self.guide, &guide, 0);
            for i in 0..iterations {
                run(&self.atrous, &atrous[(i % 2) as usize], i + 1);
            }
            run(&self.finish, &finish, 0);
        }
        copy_rows_to_texture(
            &mut encoder,
            &self.averages,
            self.pitch,
            self.width,
            self.height,
            target,
        );
        gpu.queue.submit([encoder.finish()]);
    }
}
