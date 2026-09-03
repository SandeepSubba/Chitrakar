//! A GPU render backend, built on wgpu.
//!
//! The CPU renderer in `chitrakar-render` stays the correctness
//! reference: this draws what it can and declines the rest, and its
//! tests compare it against that reference pixel by pixel (a software
//! Vulkan driver — llvmpipe — makes that comparison runnable in CI; see
//! docs/spikes/gpu-rendering.md).
//!
//! What it draws today: solid fills — rectangles (rounded too),
//! ellipses and paths, compound ones included — nested through group
//! transforms, in painter's order, with per-layer opacity, composited
//! premultiplied in linear light on a four-sample `Rgba16Float` target.
//! Rectangles and ellipses find their coverage from their own signed
//! distance; a path is filled the way a stencil buffer fills one — a fan
//! over its rings flips the stencil, so even-odd falls out of the parity
//! and a hole is a hole however the ring is wound — and the multisampling
//! is what softens its edges. A placed image is a textured quad, its
//! texels premultiplied into linear light before they are uploaded so
//! the filtering happens where the compositor works — magnified or at
//! its own size; shrunk, where the CPU box-filters the texels a pixel
//! covers, the page goes back. A gradient fill — linear or radial, on
//! any of those shapes — is a ramp baked into a row of texels and
//! sampled across the shape's own normalized box, so it follows the
//! shape the way the CPU's does. Everything else — strokes, text,
//! masks, effects, filters, adjustments, blend modes, a group that has
//! to be composited on its own, ink authored for a press — is declined,
//! and the caller falls back to the CPU.

use chitrakar_color::LinearRgba;
use chitrakar_doc::{BlendMode, Document, NodeId, NodeKind, Transform, VectorShape};
use chitrakar_render::Surface;
use wgpu::util::DeviceExt;

/// One vertex of a shape's quad: where it lands on the page, where that
/// is in the shape's own space, the shape's parameters (size, corner
/// radius, and which kind it is) and its premultiplied linear colour.
///
/// A gradient-filled shape reads two of these differently: its paint is
/// a ramp texture rather than a colour, so `color` carries only which
/// gradient it is (in `r`) and the layer's alpha (in `a`), and `grad`
/// carries the gradient's geometry in the shape's normalized box — the
/// two ends of a linear ramp, or a radial one's centre and radius.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    doc: [f32; 2],
    local: [f32; 2],
    params: [f32; 4],
    color: [f32; 4],
    grad: [f32; 4],
}

/// Samples per pixel. A path's edge is as smooth as the stencil is
/// finely sampled, and four is what every adapter offers.
const SAMPLES: u32 = 4;

const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

/// Premultiplied source over destination: the same arithmetic the CPU
/// compositor does.
const PREMULTIPLIED_OVER: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// A depth-stencil state that only ever touches the stencil: `op` on the
/// pixels that pass `compare`, and the same on those that do not, so a
/// cover pass leaves the buffer as clean as it found it.
fn stencil_state(
    op: wgpu::StencilOperation,
    compare: wgpu::CompareFunction,
) -> wgpu::DepthStencilState {
    let face = wgpu::StencilFaceState {
        compare,
        fail_op: op,
        depth_fail_op: op,
        pass_op: op,
    };
    wgpu::DepthStencilState {
        format: STENCIL_FORMAT,
        depth_write_enabled: false,
        depth_compare: wgpu::CompareFunction::Always,
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: 0xff,
        },
        bias: Default::default(),
    }
}

/// One thing to draw, in painter's order: a shape whose fragment finds
/// its own coverage, or a path stencilled and then covered.
enum Draw {
    /// A rectangle or an ellipse. `ramp` names the scene texture its
    /// gradient was baked into, or nothing when it is a flat fill.
    Shape {
        quad: std::ops::Range<u32>,
        ramp: Option<usize>,
    },
    Path {
        stencil: std::ops::Range<u32>,
        cover: std::ops::Range<u32>,
        ramp: Option<usize>,
    },
    /// A placed image: the quad, and which of the scene's textures it
    /// samples.
    Image {
        quad: std::ops::Range<u32>,
        texture: usize,
    },
}

/// A device, a queue and the pipelines that draw every shape.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    stencil: wgpu::RenderPipeline,
    cover: wgpu::RenderPipeline,
    image: wgpu::RenderPipeline,
    /// The same two passes again, painting from a gradient's ramp
    /// instead of a flat colour.
    shape_gradient: wgpu::RenderPipeline,
    cover_gradient: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// What the adapter calls itself, for tests and diagnostics.
    pub adapter: String,
}

impl GpuRenderer {
    /// Bring up a renderer on whatever adapter this machine offers, or
    /// nothing at all when it offers none.
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("chitrakar"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .ok()?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shapes"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("page"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shapes"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image"),
            bind_group_layouts: &[&layout, &texture_layout],
            push_constant_ranges: &[],
        });
        // Bilinear, clamped: the texels are premultiplied linear, so the
        // filtering happens where the compositor works, as it does on the
        // CPU. Off the edge reads as the edge, which the quad never asks
        // for anyway.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let target = wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rgba16Float,
            blend: Some(PREMULTIPLIED_OVER),
            write_mask: wgpu::ColorWrites::ALL,
        };
        let multisample = wgpu::MultisampleState {
            count: SAMPLES,
            ..Default::default()
        };
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4,
                4 => Float32x4
            ],
        };
        // The stencil pass writes no colour and flips the buffer under
        // every triangle of the fan; the cover pass paints where the
        // parity says the fill reached and clears up behind itself.
        let stencil = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("path stencil"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_stencil"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_stencil"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    write_mask: wgpu::ColorWrites::empty(),
                    ..target.clone()
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(stencil_state(
                wgpu::StencilOperation::Invert,
                wgpu::CompareFunction::Always,
            )),
            multisample,
            multiview: None,
            cache: None,
        });
        let cover = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("path cover"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_cover"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_cover"),
                compilation_options: Default::default(),
                targets: &[Some(target.clone())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(stencil_state(
                wgpu::StencilOperation::Zero,
                wgpu::CompareFunction::NotEqual,
            )),
            multisample,
            multiview: None,
            cache: None,
        });
        let image = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image"),
            layout: Some(&image_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_image"),
                compilation_options: Default::default(),
                targets: &[Some(target.clone())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(stencil_state(
                wgpu::StencilOperation::Keep,
                wgpu::CompareFunction::Always,
            )),
            multisample,
            multiview: None,
            cache: None,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shapes"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(target.clone())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            // The same attachment the paths use, left alone: a pass has
            // one set of attachments whatever is drawing into it.
            depth_stencil: Some(stencil_state(
                wgpu::StencilOperation::Keep,
                wgpu::CompareFunction::Always,
            )),
            multisample,
            multiview: None,
            cache: None,
        });
        // The gradient pipelines differ from their flat counterparts only
        // in the fragment they run and the ramp they bind, so they borrow
        // everything else from them.
        let shape_gradient = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shapes (gradient)"),
            layout: Some(&image_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_shape_gradient"),
                compilation_options: Default::default(),
                targets: &[Some(target.clone())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(stencil_state(
                wgpu::StencilOperation::Keep,
                wgpu::CompareFunction::Always,
            )),
            multisample,
            multiview: None,
            cache: None,
        });
        let cover_gradient = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("path cover (gradient)"),
            layout: Some(&image_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_cover"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_cover_gradient"),
                compilation_options: Default::default(),
                targets: &[Some(target)],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(stencil_state(
                wgpu::StencilOperation::Zero,
                wgpu::CompareFunction::NotEqual,
            )),
            multisample,
            multiview: None,
            cache: None,
        });
        Some(Self {
            device,
            queue,
            pipeline,
            stencil,
            cover,
            image,
            shape_gradient,
            cover_gradient,
            layout,
            texture_layout,
            sampler,
            adapter: info.name,
        })
    }

    /// Draw the whole page, or nothing when the document holds something
    /// this backend does not know how to draw.
    pub fn render(&self, doc: &Document) -> Option<Surface> {
        let (width, height) = (doc.meta.width, doc.meta.height);
        let mut scene = Scene::default();
        collect(doc, doc.root(), Transform::default(), 1.0, &mut scene)?;
        Some(self.draw(width, height, &scene))
    }

    /// Whether [`render`](Self::render) would draw this document.
    pub fn can_render(doc: &Document) -> bool {
        let mut scene = Scene::default();
        collect(doc, doc.root(), Transform::default(), 1.0, &mut scene).is_some()
    }

    fn draw(&self, width: u32, height: u32, scene: &Scene) -> Surface {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let make = |label, samples, format, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        // Drawn multisampled, resolved into the texture that is read back.
        let multi = make(
            "page (multisampled)",
            SAMPLES,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let texture = make(
            "page",
            1,
            wgpu::TextureFormat::Rgba16Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let stencil = make(
            "stencil",
            SAMPLES,
            STENCIL_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let multi_view = multi.create_view(&Default::default());
        let stencil_view = stencil.create_view(&Default::default());
        let view = texture.create_view(&Default::default());
        let page = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("page"),
                contents: bytemuck::cast_slice(&[width as f32, height as f32, 0.0, 0.0]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("page"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: page.as_entire_binding(),
            }],
        });
        // A page with nothing on it gets no vertex buffer: wgpu will
        // not hand out a slice of an empty one, and the pass below still
        // clears the target, which is the whole of what such a page is.
        let quads = (!scene.vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("shapes"),
                    contents: bytemuck::cast_slice(&scene.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        // Rows of the readback buffer are aligned, so a narrow page is
        // padded out and unpadded again below.
        let row = (width as usize * 8).div_ceil(256) * 256;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (row * height as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        // A texture per image the page places, premultiplied linear
        // already, so nothing has to be converted per sample.
        let textures: Vec<wgpu::BindGroup> = scene
            .textures
            .iter()
            .map(|img| {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("image"),
                    size: wgpu::Extent3d {
                        width: img.width,
                        height: img.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                self.queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytemuck::cast_slice(&img.texels),
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(img.width * 8),
                        rows_per_image: Some(img.height),
                    },
                    wgpu::Extent3d {
                        width: img.width,
                        height: img.height,
                        depth_or_array_layers: 1,
                    },
                );
                let view = texture.create_view(&Default::default());
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("image"),
                    layout: &self.texture_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                })
            })
            .collect();

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("page"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &multi_view,
                    resolve_target: Some(&view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &stencil_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Discard,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Some(quads) = &quads {
                pass.set_bind_group(0, &bind, &[]);
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.set_stencil_reference(0);
                for draw in &scene.draws {
                    match draw {
                        Draw::Shape { quad, ramp } => {
                            match ramp {
                                Some(ramp) => {
                                    pass.set_pipeline(&self.shape_gradient);
                                    pass.set_bind_group(1, &textures[*ramp], &[]);
                                }
                                None => pass.set_pipeline(&self.pipeline),
                            }
                            pass.draw(quad.clone(), 0..1);
                        }
                        Draw::Path {
                            stencil,
                            cover,
                            ramp,
                        } => {
                            pass.set_pipeline(&self.stencil);
                            pass.draw(stencil.clone(), 0..1);
                            match ramp {
                                Some(ramp) => {
                                    pass.set_pipeline(&self.cover_gradient);
                                    pass.set_bind_group(1, &textures[*ramp], &[]);
                                }
                                None => pass.set_pipeline(&self.cover),
                            }
                            pass.draw(cover.clone(), 0..1);
                        }
                        Draw::Image { quad, texture } => {
                            pass.set_pipeline(&self.image);
                            pass.set_bind_group(1, &textures[*texture], &[]);
                            pass.draw(quad.clone(), 0..1);
                        }
                    }
                }
            }
        }
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(row as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);
        let data = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height) as usize);
        for y in 0..height as usize {
            let line = &data[y * row..y * row + width as usize * 8];
            for x in 0..width as usize {
                let at = x * 8;
                let half =
                    |i: usize| f16_to_f32(u16::from_le_bytes([line[at + i], line[at + i + 1]]));
                pixels.push(LinearRgba {
                    r: half(0),
                    g: half(2),
                    b: half(4),
                    a: half(6),
                });
            }
        }
        drop(data);
        readback.unmap();
        Surface {
            width,
            height,
            pixels,
        }
    }
}

/// The nearest half-precision float to a full-precision one, for the
/// texels an image is uploaded as. Values here are colours in 0..1, so
/// the overflow and subnormal corners are handled plainly.
fn f32_to_f16(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127;
    let mantissa = bits & 0x7f_ffff;
    if exponent > 15 {
        return sign | 0x7c00; // infinity, or as near as this format goes
    }
    if exponent < -24 {
        return sign;
    }
    if exponent < -14 {
        // Subnormal: shift the implicit one down into the mantissa.
        let shift = (-14 - exponent) as u32;
        let m = (mantissa | 0x80_0000) >> (shift + 13);
        return sign | m as u16;
    }
    sign | (((exponent + 15) as u16) << 10) | ((mantissa >> 13) as u16)
}

/// A half-precision float as the full-precision one it stands for. The
/// target is 16-bit because that is the widest format every adapter will
/// blend into; nothing else in the engine speaks it.
fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exponent = ((bits >> 10) & 0x1f) as u32;
    let mantissa = (bits & 0x3ff) as u32;
    let out = match exponent {
        0 if mantissa == 0 => sign << 31,
        // Subnormal: normalize it into a single-precision exponent.
        0 => {
            let shift = mantissa.leading_zeros() - 21;
            (sign << 31) | ((127 - 15 - shift) << 23) | ((mantissa << (shift + 1)) & 0x7f_ffff)
        }
        0x1f => (sign << 31) | 0x7f80_0000 | (mantissa << 13),
        _ => (sign << 31) | ((exponent + 127 - 15) << 23) | (mantissa << 13),
    };
    f32::from_bits(out)
}

/// The vertices and the order to draw them in.
#[derive(Default)]
struct Scene {
    vertices: Vec<Vertex>,
    draws: Vec<Draw>,
    /// Everything the fragment shaders sample, premultiplied in linear
    /// light and half-precision: the pixels behind a placed image, and
    /// the baked ramp behind a gradient.
    textures: Vec<Image>,
    /// Which texture a placed resource went to, so a resource placed by
    /// several layers is uploaded once. Ramps are not shared: they are
    /// small, and two layers rarely carry the same one.
    ids: Vec<(String, usize)>,
}

/// A texture ready to upload: its size and its texels.
struct Image {
    width: u32,
    height: u32,
    texels: Vec<u16>,
}

impl Scene {
    /// Take a run of vertices as the range it occupies.
    fn push(&mut self, verts: Vec<Vertex>) -> std::ops::Range<u32> {
        let start = self.vertices.len() as u32;
        self.vertices.extend(verts);
        start..self.vertices.len() as u32
    }
}

/// Walk the tree in painter's order, turning what can be drawn into
/// quads; `None` the moment something cannot be.
fn collect(
    doc: &Document,
    group: NodeId,
    parent: Transform,
    opacity: f32,
    out: &mut Scene,
) -> Option<()> {
    for &child in doc.children_of(group).ok()? {
        let node = doc.node(child).ok()?;
        if !node.visible || node.opacity <= 0.0 {
            continue;
        }
        // Anything that needs a surface of its own, or reads what is
        // under it, belongs to the CPU for now.
        if node.mask.is_some() || !node.effects.is_empty() || node.blend != BlendMode::Normal {
            return None;
        }
        let t = parent.compose(node.transform);
        match &node.kind {
            NodeKind::Group => {
                // A group at less than full opacity composites as a unit,
                // which needs an isolation pass this backend has not got.
                if node.opacity < 1.0 {
                    return None;
                }
                collect(doc, child, t, opacity, out)?;
            }
            NodeKind::Vector {
                shape,
                fill,
                stroke,
                gradient,
            } => {
                if stroke.is_some() {
                    return None;
                }
                let alpha = node.opacity * opacity;
                // A gradient paints in place of the flat fill, from a
                // ramp baked here once and sampled there per pixel; the
                // layer's own opacity scales it in the fragment, so two
                // layers could share a ramp even at different opacities.
                let (color, grad, ramp) = match gradient {
                    // No stops is nothing to paint, as it is on the CPU
                    // — and the flat fill stays covered up.
                    Some(g) if g.stops().is_empty() => continue,
                    Some(g) => {
                        let (ramp, geom, radial) = bake(g)?;
                        let at = out.textures.len();
                        out.textures.push(ramp);
                        let kind = if radial { 1.0 } else { 0.0 };
                        ([kind, 0.0, 0.0, alpha], geom, Some(at))
                    }
                    None => {
                        let fill = (*fill)?;
                        // Ink authored for a press resolves through the
                        // profile, which is the CPU's business.
                        let chitrakar_color::AuthoredColor::Srgb { .. } = fill else {
                            return None;
                        };
                        let c = chitrakar_color::to_working(fill);
                        (
                            [c.r * alpha, c.g * alpha, c.b * alpha, c.a * alpha],
                            [0.0; 4],
                            None,
                        )
                    }
                };
                let (size, radius, kind) = match shape {
                    VectorShape::Rect {
                        width,
                        height,
                        radius,
                    } => {
                        let r = radius.max(0.0).min(width.min(*height).max(0.0) / 2.0);
                        ([*width, *height], r, 0.0)
                    }
                    VectorShape::Ellipse { rx, ry } => ([rx * 2.0, ry * 2.0], 0.0, 1.0),
                    // A path is stencilled from its rings and covered:
                    // parity gives the even-odd fill the CPU draws,
                    // holes and crossings included.
                    VectorShape::Path { .. } => {
                        let rings: Vec<Vec<[f32; 2]>> = chitrakar_render::shape_rings(shape)
                            .into_iter()
                            .map(|ring| {
                                ring.into_iter()
                                    .map(|p| {
                                        [
                                            t.a * p[0] + t.c * p[1] + t.e,
                                            t.b * p[0] + t.d * p[1] + t.f,
                                        ]
                                    })
                                    .collect()
                            })
                            .filter(|ring: &Vec<[f32; 2]>| ring.len() >= 3)
                            .collect();
                        if rings.is_empty() {
                            continue;
                        }
                        // The stencil pass reads nothing but position.
                        let mut fan = Vec::new();
                        for ring in &rings {
                            for i in 1..ring.len() - 1 {
                                for p in [ring[0], ring[i], ring[i + 1]] {
                                    fan.push(Vertex {
                                        doc: p,
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                        let stencil = out.push(fan);
                        // The cover quad is the layer's own box carried
                        // through its transform — a parallelogram that
                        // holds every point the fill can reach, grown by
                        // a device pixel so it cannot cut the edge short.
                        // Its corners carry the box's normalized
                        // coordinates, which a gradient interpolates
                        // across however the layer is turned.
                        let Some([x0, y0, x1, y1]) =
                            chitrakar_render::local_bounds_of(doc, child).ok()?
                        else {
                            continue;
                        };
                        let scale = (t.a.abs() + t.c.abs()).max(t.b.abs() + t.d.abs()).max(1e-6);
                        let (mu, mv) = (1.0 / scale / (x1 - x0), 1.0 / scale / (y1 - y0));
                        let corner = |u: f32, v: f32| {
                            let (lx, ly) = (x0 + (x1 - x0) * u, y0 + (y1 - y0) * v);
                            Vertex {
                                doc: [t.a * lx + t.c * ly + t.e, t.b * lx + t.d * ly + t.f],
                                local: [u, v],
                                params: [0.0; 4],
                                color,
                                grad,
                            }
                        };
                        let (lo, hi) = ((-mu, -mv), (1.0 + mu, 1.0 + mv));
                        let cover = out.push(vec![
                            corner(lo.0, lo.1),
                            corner(hi.0, lo.1),
                            corner(hi.0, hi.1),
                            corner(lo.0, lo.1),
                            corner(hi.0, hi.1),
                            corner(lo.0, hi.1),
                        ]);
                        out.draws.push(Draw::Path {
                            stencil,
                            cover,
                            ramp,
                        });
                        continue;
                    }
                };
                if !(size[0] > 0.0 && size[1] > 0.0) {
                    continue;
                }
                let range = out.push(quad(
                    t,
                    size,
                    [size[0], size[1], radius, kind],
                    color,
                    grad,
                    1.5,
                ));
                out.draws.push(Draw::Shape { quad: range, ramp });
            }
            NodeKind::Raster(raster) => {
                let Some(res) = doc.resource(&raster.resource_id) else {
                    // A resource whose pixels never came back is drawn by
                    // nobody; the CPU skips it too.
                    continue;
                };
                if res.rgba8.is_empty() {
                    continue;
                }
                // Shrinking is where the two renderers part: the CPU box-
                // filters the texels a pixel covers, and bilinear sampling
                // would alias. Hand the page over rather than draw it
                // differently.
                let scale = (t.a.abs() + t.c.abs()).max(t.b.abs() + t.d.abs());
                if scale < 0.99 {
                    return None;
                }
                let at = match out.ids.iter().find(|(id, _)| *id == raster.resource_id) {
                    Some((_, at)) => *at,
                    None => {
                        let at = out.textures.len();
                        out.textures.push(premultiplied(res));
                        out.ids.push((raster.resource_id.clone(), at));
                        at
                    }
                };
                let alpha = node.opacity * opacity;
                let size = [res.width as f32, res.height as f32];
                // The quad is the image's own box; its local coordinates
                // are the texture's, so the vertex shader passes them
                // straight through as texture coordinates.
                let mut verts = quad(t, size, [0.0; 4], [0.0, 0.0, 0.0, alpha], [0.0; 4], 0.0);
                for v in &mut verts {
                    v.local = [v.local[0] / size[0], v.local[1] / size[1]];
                }
                let quad = out.push(verts);
                out.draws.push(Draw::Image { quad, texture: at });
            }
            _ => return None,
        }
    }
    Some(())
}

/// How many texels a gradient's ramp is baked into. Its stops are
/// resolved here and the sampler interpolates between them, so the only
/// error is at a stop landing between two texels: the ramp bends a
/// five-hundredth of its length early or late, and nowhere else.
const RAMP: u32 = 512;

/// A gradient as the shader wants it: its ramp baked into a row of
/// premultiplied linear texels, its geometry in the shape's normalized
/// box, and whether that geometry is a radial one.
///
/// `None` declines the page: a stop authored for a press resolves
/// through the document's profile, which is the CPU's business. The
/// caller has already ruled out a gradient with no stops.
fn bake(g: &chitrakar_doc::Gradient) -> Option<(Image, [f32; 4], bool)> {
    let mut stops = Vec::with_capacity(g.stops().len());
    for stop in g.stops() {
        let chitrakar_color::AuthoredColor::Srgb { .. } = stop.color else {
            return None;
        };
        stops.push((stop.offset, chitrakar_color::to_working(stop.color)));
    }
    stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut texels = Vec::with_capacity(RAMP as usize * 4);
    for i in 0..RAMP {
        let c = ramp_at(&stops, i as f32 / (RAMP - 1) as f32);
        texels.extend_from_slice(&[
            f32_to_f16(c.r),
            f32_to_f16(c.g),
            f32_to_f16(c.b),
            f32_to_f16(c.a),
        ]);
    }
    let (geom, radial) = match g {
        chitrakar_doc::Gradient::Linear { from, to, .. } => {
            ([from[0], from[1], to[0], to[1]], false)
        }
        chitrakar_doc::Gradient::Radial { center, radius, .. } => {
            ([center[0], center[1], *radius, 0.0], true)
        }
    };
    Some((
        Image {
            width: RAMP,
            height: 1,
            texels,
        },
        geom,
        radial,
    ))
}

/// Colour at `t` along a sorted ramp, clamped past either end: the same
/// walk the CPU renderer does per pixel, done once per texel here.
fn ramp_at(stops: &[(f32, LinearRgba)], t: f32) -> LinearRgba {
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    if t <= first.0 {
        return first.1;
    }
    if t >= last.0 {
        return last.1;
    }
    for w in stops.windows(2) {
        if t <= w[1].0 {
            let span = w[1].0 - w[0].0;
            let k = if span.abs() < 1e-6 {
                0.0
            } else {
                (t - w[0].0) / span
            };
            let mix = |a: f32, b: f32| a + (b - a) * k;
            return LinearRgba {
                r: mix(w[0].1.r, w[1].1.r),
                g: mix(w[0].1.g, w[1].1.g),
                b: mix(w[0].1.b, w[1].1.b),
                a: mix(w[0].1.a, w[1].1.a),
            };
        }
    }
    last.1
}

/// A resource's pixels as the compositor wants them: linear light,
/// premultiplied, half-precision.
fn premultiplied(res: &chitrakar_doc::Resource) -> Image {
    let mut texels = Vec::with_capacity(res.rgba8.len());
    for px in res.rgba8.chunks_exact(4) {
        let a = px[3] as f32 / 255.0;
        let c = |v: u8| f32_to_f16(chitrakar_color::srgb_to_linear(v as f32 / 255.0) * a);
        texels.extend_from_slice(&[c(px[0]), c(px[1]), c(px[2]), f32_to_f16(a)]);
    }
    Image {
        width: res.width,
        height: res.height,
        texels,
    }
}

/// The six vertices of a shape's quad, in document space, grown by
/// `grow` device pixels so an antialiased edge has somewhere to land.
/// A shape wants a pixel and a half of that; an image wants none — its
/// texture ends where the box does, and a margin would sample past it.
fn quad(
    t: Transform,
    size: [f32; 2],
    params: [f32; 4],
    color: [f32; 4],
    grad: [f32; 4],
    grow: f32,
) -> Vec<Vertex> {
    let scale = (t.a.abs() + t.c.abs()).max(t.b.abs() + t.d.abs()).max(1e-6);
    let m = grow / scale;
    let corners = [
        [-m, -m],
        [size[0] + m, -m],
        [size[0] + m, size[1] + m],
        [-m, size[1] + m],
    ];
    let place = |p: [f32; 2]| Vertex {
        doc: [t.a * p[0] + t.c * p[1] + t.e, t.b * p[0] + t.d * p[1] + t.f],
        local: p,
        params,
        color,
        grad,
    };
    let [tl, tr, br, bl] = corners;
    vec![
        place(tl),
        place(tr),
        place(br),
        place(tl),
        place(br),
        place(bl),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chitrakar_color::{AuthoredColor, ColorMode};
    use chitrakar_doc::{Command, Node};

    /// A renderer, or nothing where the machine has no adapter — except
    /// in CI, which installs a software driver on purpose: a skip there
    /// would quietly stop checking the thing the job exists to check.
    fn gpu_or_skip() -> Option<GpuRenderer> {
        match GpuRenderer::new() {
            Some(gpu) => Some(gpu),
            None if std::env::var("CI").is_ok() => {
                panic!("no GPU adapter in CI: mesa-vulkan-drivers should have been installed")
            }
            None => {
                eprintln!("skipped: no GPU adapter");
                None
            }
        }
    }

    const RED: AuthoredColor = AuthoredColor::Srgb {
        r: 1.0,
        g: 0.2,
        b: 0.1,
        a: 1.0,
    };
    const BLUE: AuthoredColor = AuthoredColor::Srgb {
        r: 0.1,
        g: 0.3,
        b: 0.9,
        a: 1.0,
    };
    const WHITE: AuthoredColor = AuthoredColor::Srgb {
        r: 0.95,
        g: 0.95,
        b: 0.9,
        a: 1.0,
    };

    fn filled(name: &str, shape: VectorShape, color: AuthoredColor) -> Box<Node> {
        let mut node = Node::vector(name, shape);
        if let NodeKind::Vector { fill, .. } = &mut node.kind {
            *fill = Some(color);
        }
        Box::new(node)
    }

    fn add(doc: &mut Document, node: Box<Node>, at: Transform) -> NodeId {
        let root = doc.root();
        let index = doc.children_of(root).unwrap().len();
        doc.apply(Command::AddNode {
            parent: root,
            index,
            node,
        })
        .unwrap();
        let id = doc.children_of(root).unwrap()[index];
        doc.apply(Command::SetTransform { id, transform: at })
            .unwrap();
        id
    }

    /// A page of everything this backend claims to draw.
    fn page() -> Document {
        let mut doc = Document::new(120, 80, ColorMode::Rgb);
        add(
            &mut doc,
            filled(
                "rect",
                VectorShape::Rect {
                    width: 40.0,
                    height: 30.0,
                    radius: 0.0,
                },
                RED,
            ),
            Transform::translation(10.0, 10.0),
        );
        add(
            &mut doc,
            filled(
                "round",
                VectorShape::Rect {
                    width: 30.0,
                    height: 30.0,
                    radius: 8.0,
                },
                BLUE,
            ),
            Transform::translation(60.0, 8.0),
        );
        add(
            &mut doc,
            filled("ellipse", VectorShape::Ellipse { rx: 20.0, ry: 12.0 }, BLUE),
            Transform::translation(15.0, 45.0),
        );
        // Turned and scaled, to check the quad and the edge follow the
        // transform rather than the axes.
        let turned = add(
            &mut doc,
            filled(
                "turned",
                VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 0.0,
                },
                RED,
            ),
            Transform::default(),
        );
        let (sin, cos) = 0.4f32.sin_cos();
        doc.apply(Command::SetTransform {
            id: turned,
            transform: Transform {
                a: 1.4 * cos,
                b: 1.4 * sin,
                c: -1.4 * sin,
                d: 1.4 * cos,
                e: 75.0,
                f: 45.0,
            },
        })
        .unwrap();
        doc
    }

    /// How far apart two renders are, per channel, over the whole page.
    fn difference(a: &Surface, b: &Surface) -> (f64, f64) {
        assert_eq!((a.width, a.height), (b.width, b.height));
        let (mut total, mut worst) = (0.0f64, 0.0f64);
        for (p, q) in a.pixels.iter().zip(&b.pixels) {
            for (u, v) in [(p.r, q.r), (p.g, q.g), (p.b, q.b), (p.a, q.a)] {
                let d = (u - v).abs() as f64;
                total += d;
                worst = worst.max(d);
            }
        }
        (total / (a.pixels.len() * 4) as f64, worst)
    }

    #[test]
    fn the_gpu_draws_the_page_the_cpu_draws() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        eprintln!("adapter: {}", gpu.adapter);
        let doc = page();
        assert!(GpuRenderer::can_render(&doc));
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        assert!(
            mean < 0.004,
            "mean channel difference {mean:.5} (worst {worst:.3})"
        );
        // Inside and outside are not approximate: they are the fill and
        // the bare page, to the precision the target holds.
        let at = |s: &Surface, x: u32, y: u32| s.get(x, y);
        for (x, y) in [(30, 25), (75, 20), (35, 57), (80, 55), (5, 5), (110, 75)] {
            let (g, c) = (at(&drawn, x, y), at(&reference, x, y));
            assert!(
                (g.r - c.r).abs() < 0.01
                    && (g.g - c.g).abs() < 0.01
                    && (g.b - c.b).abs() < 0.01
                    && (g.a - c.a).abs() < 0.01,
                "at ({x}, {y}): {g:?} vs {c:?}"
            );
        }
        // The edges are antialiased rather than stepped, and the softness
        // is the reference's: down through the ellipse's rim, coverage
        // rises through partial values and tracks the CPU's row for row.
        let scan = |s: &Surface| (43..49).map(|y| s.get(35, y).a).collect::<Vec<f32>>();
        let (rim, want) = (scan(&drawn), scan(&reference));
        assert!(
            rim.iter().any(|a| *a > 0.001 && *a < 0.999),
            "a soft rim, not a hard one: {rim:?}"
        );
        assert!(
            rim.iter().zip(&want).all(|(a, b)| (a - b).abs() < 0.06),
            "the same rim the CPU draws: {rim:?} vs {want:?}"
        );
    }

    #[test]
    fn paths_are_filled_the_way_the_cpu_fills_them() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(90, 90, ColorMode::Rgb);
        // A square with a square hole, and a curved petal beside it: the
        // hole tests the parity, the curve tests the flattening.
        add(
            &mut doc,
            filled(
                "ring",
                VectorShape::Path {
                    points: vec![[0.0, 0.0], [40.0, 0.0], [40.0, 40.0], [0.0, 40.0]],
                    closed: true,
                    smooth: false,
                    handles: Vec::new(),
                    subpaths: vec![vec![[10.0, 10.0], [30.0, 10.0], [30.0, 30.0], [10.0, 30.0]]],
                },
                RED,
            ),
            Transform::translation(5.0, 5.0),
        );
        add(
            &mut doc,
            filled(
                "petal",
                VectorShape::Path {
                    points: vec![[0.0, 0.0], [30.0, 0.0], [30.0, 30.0]],
                    closed: true,
                    smooth: false,
                    handles: vec![[0.0, 0.0, 6.0, -14.0], [-6.0, -14.0, 0.0, 0.0], [0.0; 4]],
                    subpaths: Vec::new(),
                },
                BLUE,
            ),
            Transform::translation(52.0, 50.0),
        );
        assert!(GpuRenderer::can_render(&doc));
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        // Wider than the analytic shapes: a stencil's edge is as fine as
        // the sampling, not exact.
        assert!(
            mean < 0.012,
            "mean channel difference {mean:.5} (worst {worst:.3})"
        );
        assert_eq!(drawn.get(8, 8).a, 1.0, "inside the ring");
        assert_eq!(drawn.get(25, 25).a, 0.0, "and the hole is a hole");
        assert!(drawn.get(60, 55).a > 0.9, "the petal is filled");
        assert_eq!(drawn.get(85, 10).a, 0.0, "bare page stays bare");
        // A slanted edge is soft, and as soft as the CPU draws it: across
        // the petal's diagonal, coverage falls through partial values.
        let scan = |s: &Surface| (68..76).map(|x| s.get(x, 70).a).collect::<Vec<f32>>();
        let (edge, want) = (scan(&drawn), scan(&reference));
        assert!(
            edge.iter().any(|a| *a > 0.05 && *a < 0.95),
            "a soft edge: {edge:?}"
        );
        assert!(
            edge.iter().zip(&want).all(|(a, b)| (a - b).abs() < 0.3),
            "close to the CPU's edge: {edge:?} vs {want:?}"
        );
    }

    #[test]
    fn opacity_and_groups_composite_as_they_do_on_the_cpu() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(60, 60, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::group("g")),
        })
        .unwrap();
        let group = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id: group,
            transform: Transform::translation(10.0, 10.0),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: group,
            index: 0,
            node: filled(
                "under",
                VectorShape::Rect {
                    width: 30.0,
                    height: 30.0,
                    radius: 0.0,
                },
                RED,
            ),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: group,
            index: 1,
            node: filled(
                "over",
                VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 0.0,
                },
                BLUE,
            ),
        })
        .unwrap();
        let over = doc.children_of(group).unwrap()[1];
        doc.apply(Command::SetOpacity {
            id: over,
            opacity: 0.5,
        })
        .unwrap();
        let (mean, worst) = difference(
            &gpu.render(&doc).unwrap(),
            &chitrakar_render::render(&doc).unwrap(),
        );
        assert!(mean < 0.004, "mean {mean:.5}, worst {worst:.3}");

        // A group at less than full opacity composites as a unit, which
        // this backend declines rather than getting wrong.
        doc.apply(Command::SetOpacity {
            id: group,
            opacity: 0.5,
        })
        .unwrap();
        assert!(!GpuRenderer::can_render(&doc));
        assert!(gpu.render(&doc).is_none());
    }

    #[test]
    fn a_placed_image_is_sampled_the_way_the_cpu_samples_it() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(64, 40, ColorMode::Rgb);
        // Four texels: red, green, blue and a clear one, so the corners
        // and the alpha all have something to say.
        let rgba = vec![
            255, 40, 30, 255, 30, 220, 60, 255, //
            40, 60, 240, 255, 0, 0, 0, 0,
        ];
        let id = doc.add_resource(2, 2, rgba);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::raster(
                "img",
                chitrakar_doc::RasterRef {
                    resource_id: id.clone(),
                    width: 2,
                    height: 2,
                },
            )),
        })
        .unwrap();
        let img = doc.children_of(root).unwrap()[0];
        // Magnified ten times and moved: bilinear both sides.
        doc.apply(Command::SetTransform {
            id: img,
            transform: Transform {
                a: 10.0,
                d: 10.0,
                e: 5.0,
                f: 5.0,
                ..Default::default()
            },
        })
        .unwrap();
        assert!(GpuRenderer::can_render(&doc));
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        assert!(
            mean < 0.006,
            "mean channel difference {mean:.5} (worst {worst:.3})"
        );
        // The texel centres are the colours they were given, and the
        // clear corner stays clear.
        let red = drawn.get(10, 10).to_srgb8();
        assert!(red[0] > 240 && red[1] < 70, "the red texel: {red:?}");
        // Halfway between the blue texel's centre and the clear one,
        // coverage is halfway too — and it is the CPU's halfway.
        let (mid, want) = (drawn.get(15, 20).a, reference.get(15, 20).a);
        assert!(
            mid > 0.3 && mid < 0.7 && (mid - want).abs() < 0.05,
            "{mid} vs {want}"
        );
        assert!(
            (drawn.get(20, 20).a - reference.get(20, 20).a).abs() < 0.05,
            "the clear corner"
        );
        assert_eq!(drawn.get(2, 2).a, 0.0, "bare page beside it");
        // The same image twice shares one texture.
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::raster(
                "again",
                chitrakar_doc::RasterRef {
                    resource_id: id,
                    width: 2,
                    height: 2,
                },
            )),
        })
        .unwrap();
        let mut scene = Scene::default();
        collect(&doc, doc.root(), Transform::default(), 1.0, &mut scene).unwrap();
        assert_eq!(scene.textures.len(), 1, "one texture for two placements");
        assert_eq!(scene.draws.len(), 2);

        // Shrunk, the CPU box-filters the texels a pixel covers; rather
        // than draw that differently, the page goes back.
        doc.apply(Command::SetTransform {
            id: img,
            transform: Transform {
                a: 0.25,
                d: 0.25,
                ..Default::default()
            },
        })
        .unwrap();
        assert!(!GpuRenderer::can_render(&doc));
    }

    #[test]
    fn half_precision_survives_the_trip_out_and_back() {
        for v in [0.0, 0.25, 0.5, 1.0, 1.0 / 3.0, 0.001] {
            let back = f16_to_f32(f32_to_f16(v));
            assert!((back - v).abs() < 1e-3, "{v} came back as {back}");
        }
        assert_eq!(f32_to_f16(0.0), 0);
        assert_eq!(f16_to_f32(f32_to_f16(1.0)), 1.0);
        // Below what the format can hold, it says nothing rather than
        // something wrong.
        assert_eq!(f32_to_f16(1e-9), 0);
    }

    fn ramp(offsets: &[(f32, AuthoredColor)]) -> Vec<chitrakar_doc::GradientStop> {
        offsets
            .iter()
            .map(|(offset, color)| chitrakar_doc::GradientStop {
                offset: *offset,
                color: *color,
            })
            .collect()
    }

    fn gradient_filled(name: &str, shape: VectorShape, g: chitrakar_doc::Gradient) -> Box<Node> {
        let mut node = Node::vector(name, shape);
        if let NodeKind::Vector { fill, gradient, .. } = &mut node.kind {
            // A fill underneath, which the gradient paints in place of:
            // if the two ever swapped the difference would be loud.
            *fill = Some(RED);
            *gradient = Some(g);
        }
        Box::new(node)
    }

    /// A gradient is a ramp baked into a row of texels here and a flat
    /// colour interpolated per pixel there — the same paint either way,
    /// on every shape that can carry it and through a transform.
    #[test]
    fn gradients_ramp_the_way_the_cpu_ramps_them() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(120, 80, ColorMode::Rgb);
        // Corner to corner, through a middle stop that is not halfway.
        add(
            &mut doc,
            gradient_filled(
                "rect",
                VectorShape::Rect {
                    width: 40.0,
                    height: 30.0,
                    radius: 6.0,
                },
                chitrakar_doc::Gradient::Linear {
                    from: [0.0, 0.0],
                    to: [1.0, 1.0],
                    stops: ramp(&[(0.0, RED), (0.3, BLUE), (1.0, WHITE)]),
                },
            ),
            Transform::translation(8.0, 8.0),
        );
        // Radial, on an ellipse, turned: the box it ramps across turns
        // with it, so the ramp does too.
        add(
            &mut doc,
            gradient_filled(
                "ellipse",
                VectorShape::Ellipse { rx: 20.0, ry: 14.0 },
                chitrakar_doc::Gradient::Radial {
                    center: [0.4, 0.45],
                    radius: 0.8,
                    stops: ramp(&[(0.0, WHITE), (1.0, BLUE)]),
                },
            ),
            Transform {
                a: 0.9,
                b: 0.44,
                c: -0.44,
                d: 0.9,
                e: 78.0,
                f: 14.0,
            },
        );
        // And on a path, where the stencil says where the fill reached
        // and the cover quad says what colour it is.
        add(
            &mut doc,
            gradient_filled(
                "path",
                VectorShape::Path {
                    points: vec![[0.0, 0.0], [44.0, 6.0], [38.0, 30.0], [6.0, 24.0]],
                    closed: true,
                    smooth: false,
                    handles: Vec::new(),
                    subpaths: Vec::new(),
                },
                chitrakar_doc::Gradient::Linear {
                    from: [0.0, 1.0],
                    to: [0.0, 0.0],
                    stops: ramp(&[(0.0, RED), (1.0, BLUE)]),
                },
            ),
            Transform::translation(12.0, 44.0),
        );

        assert!(GpuRenderer::can_render(&doc));
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        assert!(
            mean < 0.006,
            "mean channel difference {mean:.5} (worst {worst:.3})"
        );
        // Well inside each shape the colour is the reference's, not
        // merely close on average.
        for (x, y) in [(20, 20), (30, 20), (40, 20), (90, 35), (81, 31), (34, 59)] {
            let (g, c) = (drawn.get(x, y), reference.get(x, y));
            assert!(
                (g.r - c.r).abs() < 0.02
                    && (g.g - c.g).abs() < 0.02
                    && (g.b - c.b).abs() < 0.02
                    && (g.a - c.a).abs() < 0.02,
                "at ({x}, {y}): {g:?} vs {c:?}"
            );
        }
        // It really ramps: across the rect the blue channel climbs, and
        // it climbs the way the reference's does.
        let across = |s: &Surface| {
            (12..44)
                .step_by(4)
                .map(|x| s.get(x, 20).b)
                .collect::<Vec<f32>>()
        };
        let (got, want) = (across(&drawn), across(&reference));
        assert!(
            got.windows(2).any(|w| w[1] > w[0] + 0.02),
            "a ramp, not a flat fill: {got:?}"
        );
        assert!(
            got.iter().zip(&want).all(|(a, b)| (a - b).abs() < 0.03),
            "the ramp the CPU draws: {got:?} vs {want:?}"
        );
    }

    /// A gradient with no stops paints nothing at all — and does not
    /// fall back to the flat fill underneath it, which is what the CPU
    /// does with one.
    #[test]
    fn a_gradient_without_stops_paints_nothing() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        add(
            &mut doc,
            gradient_filled(
                "empty",
                VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 0.0,
                },
                chitrakar_doc::Gradient::Linear {
                    from: [0.0, 0.0],
                    to: [1.0, 0.0],
                    stops: Vec::new(),
                },
            ),
            Transform::translation(10.0, 10.0),
        );
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        assert_eq!(drawn.get(20, 20).a, 0.0, "a bare page");
        assert_eq!(reference.get(20, 20).a, 0.0, "which is what the CPU draws");
    }

    #[test]
    fn what_it_cannot_draw_it_declines() {
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        let rect = VectorShape::Rect {
            width: 20.0,
            height: 20.0,
            radius: 0.0,
        };
        let id = add(
            &mut doc,
            filled("r", rect.clone(), RED),
            Transform::default(),
        );
        assert!(GpuRenderer::can_render(&doc));

        // A stroke, text, a blend mode: each on its own is enough to
        // hand the page back.
        let mut with_stroke = doc.clone();
        with_stroke
            .apply(Command::SetKind {
                id,
                kind: Box::new(NodeKind::Vector {
                    shape: rect.clone(),
                    fill: Some(RED),
                    stroke: Some(chitrakar_doc::Stroke {
                        color: BLUE,
                        width: 2.0,
                        widths: Vec::new(),
                    }),
                    gradient: None,
                }),
            })
            .unwrap();
        assert!(!GpuRenderer::can_render(&with_stroke));

        let mut blended = doc.clone();
        blended
            .apply(Command::SetBlendMode {
                id,
                blend: BlendMode::Multiply,
            })
            .unwrap();
        assert!(!GpuRenderer::can_render(&blended));

        let mut with_text = doc.clone();
        let root = with_text.root();
        with_text
            .apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::text(
                    "t",
                    chitrakar_doc::TextSpec::new("hi", 12.0, RED),
                )),
            })
            .unwrap();
        assert!(!GpuRenderer::can_render(&with_text));

        // Ink authored for a press resolves through the document's
        // profile, so a gradient with a CMYK stop goes back too.
        let mut pressed = doc.clone();
        pressed
            .apply(Command::SetKind {
                id,
                kind: Box::new(NodeKind::Vector {
                    shape: rect.clone(),
                    fill: None,
                    stroke: None,
                    gradient: Some(chitrakar_doc::Gradient::Linear {
                        from: [0.0, 0.0],
                        to: [1.0, 0.0],
                        stops: ramp(&[
                            (0.0, RED),
                            (
                                1.0,
                                AuthoredColor::Cmyk {
                                    c: 0.1,
                                    m: 0.8,
                                    y: 0.2,
                                    k: 0.0,
                                    a: 1.0,
                                },
                            ),
                        ]),
                    }),
                }),
            })
            .unwrap();
        assert!(!GpuRenderer::can_render(&pressed));

        // A hidden layer it cannot draw is no obstacle: it is not drawn.
        let hidden = with_text.children_of(root).unwrap()[1];
        with_text
            .apply(Command::SetVisible {
                id: hidden,
                visible: false,
            })
            .unwrap();
        assert!(GpuRenderer::can_render(&with_text));
    }

    /// What the two backends cost on the same page. Not an assertion:
    /// llvmpipe is a CPU driver, so this measures the plumbing, not a
    /// graphics card.
    #[test]
    #[ignore = "timing probe, not an assertion"]
    fn gpu_timing_probe() {
        let Some(gpu) = GpuRenderer::new() else {
            return;
        };
        let mut doc = page();
        doc.meta.width = 1280;
        doc.meta.height = 720;
        for _ in 0..3 {
            let t = std::time::Instant::now();
            let _ = gpu.render(&doc).unwrap();
            let gpu_ms = t.elapsed();
            let t = std::time::Instant::now();
            let _ = chitrakar_render::render(&doc).unwrap();
            eprintln!("{} — gpu {gpu_ms:?}, cpu {:?}", gpu.adapter, t.elapsed());
        }
    }

    #[test]
    fn half_precision_decodes_the_way_the_target_encodes() {
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0x3800), 0.5);
        assert_eq!(f16_to_f32(0xbc00), -1.0);
        assert!((f16_to_f32(0x3555) - 1.0 / 3.0).abs() < 1e-3);
        assert!(
            f16_to_f32(0x0001) > 0.0 && f16_to_f32(0x0001) < 1e-6,
            "subnormal"
        );
        assert!(f16_to_f32(0x7c00).is_infinite());
    }
}
