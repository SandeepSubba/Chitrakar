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
//! shape the way the CPU's does. A stroke is an inner band on a
//! rectangle or an ellipse, measured from the two rims so that stroking
//! one never grows its bounds; on a path it is the union of the
//! round-capped segments the CPU tests a sample against, laid down as
//! geometry — a trapezoid per segment and a disc at every point — and
//! unioned in the stencil, so joins, caps and a width that swells and
//! tapers all come out of the one region. Text is the whole block
//! rasterized to coverage at the size it is seen at — by the renderer
//! that owns that decision, so the bitmap is the one the CPU would have
//! sampled — read off a quad over the block's own box. A mask is the
//! coverage the CPU compositor reads, rasterized once into a texture
//! and multiplied into the fragments of everything the layer drew. A
//! blend mode, a group that is less than opaque, one carrying a mask
//! and one holding something that reads what is under it each
//! composite on a surface of their own, and the surface is what lands.
//! An adjustment layer, and a filter that is a function of one pixel,
//! rewrite what is composited below them from a copy taken aside; a
//! blur and a sharpen are the same three box passes each way the CPU
//! makes them of.
//!
//! A layer held to the one under it shows only where that layer's own
//! alpha does, which is that layer drawn aside — so it arrives the way
//! a mask does, as a coverage in a texture, and a layer that is both
//! held and masked is held back by one coverage, since two of them
//! multiplied are one. A frame is a group with a size of its own: its
//! ground is the rectangle it is, filled, and everything under it is
//! held to that rectangle — whole pixels, so that holding rides the
//! same coverage too. A copy of another layer draws what that layer
//! draws, where the copy is: the original's own placement is undone
//! first, so moving the original moves only the original, and where the
//! copy stands in for the original's children with layers of its own,
//! those are what it draws.
//!
//! What is declined, and falls back to the CPU: a live effect; a layer
//! held to a base whose own alpha is not a plain question — an
//! adjustment, or one that is faded, blended or masked; a frame that is
//! turned, composited as a whole, or whose box does not land on whole
//! pixels, each of which the CPU draws another way; a copy of another
//! layer that is faded, blended or masked, for the same reason; a paint
//! layer;
//! pixelate, which reads a neighbourhood but not along an axis; ink
//! authored for a press; and anything needing a texture larger than the
//! device was asked for. Declining is always a safe answer — the page
//! comes out right, more slowly. Drawing the wrong thing never is,
//! which is what the audit over every command is there to catch.

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
    /// The box a mask's coverage was rasterized over, in page pixels
    /// (x, y, width, height), so the fragment can turn where it is on
    /// the page into where it is in that raster. A width of zero says
    /// the layer has no mask, which is most of them.
    mask: [f32; 4],
}

/// What a layer with no mask carries: a box of no width, which the
/// fragment reads as "let everything through".
const NO_MASK: [f32; 4] = [0.0; 4];

/// The largest texture asked for, which is what `downlevel_defaults`
/// guarantees on every adapter. A page that would need a bigger one —
/// a page larger than this, a placed image larger than this, or a text
/// block rasterized this finely — goes back to the CPU rather than
/// overrunning what the device was asked for.
const MAX_TEXTURE: u32 = 2048;

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
    /// A path's stroke: the pieces that make up the region it covers,
    /// which the stencil takes the union of, and the quad that paints
    /// the union.
    Stroke {
        union: std::ops::Range<u32>,
        cover: std::ops::Range<u32>,
    },
    /// A text block: the quad over the block's box, and the coverage
    /// raster it reads.
    Text {
        quad: std::ops::Range<u32>,
        texture: usize,
    },
    /// A placed image: the quad, and which of the scene's textures it
    /// samples.
    Image {
        quad: std::ops::Range<u32>,
        texture: usize,
    },
    /// An adjustment layer: the quad over the page whose fragment reads
    /// what is under it — from the copy a blend reads too — and writes
    /// the adjusted answer back over it.
    Adjust {
        quad: std::ops::Range<u32>,
        /// The scene texture it is read off, for the ones stated by a
        /// table rather than by a handful of numbers.
        table: Option<usize>,
    },
    /// A blur layer: what is under it, run through six box passes on a
    /// pair of scratch textures, coming back down weighed by the
    /// layer's opacity and its mask. `steps` is two quads — one along
    /// each axis, each carrying the radius — that the six passes
    /// alternate between; `quad` is the one that lays the result down.
    Blur {
        steps: std::ops::Range<u32>,
        quad: std::ops::Range<u32>,
    },
    /// A motion blur: what is under it, run through one pass that
    /// averages along a line, coming back down the way a blur's does.
    /// `along` is the quad carrying how many taps and how far apart.
    Smear {
        along: std::ops::Range<u32>,
        quad: std::ops::Range<u32>,
    },
    /// A pixelate: two passes on the same scratch pair, one along each
    /// axis, coming back down the way a blur's does. `steps` is the two
    /// quads, each carrying the grid along its own axis.
    Blocks {
        steps: std::ops::Range<u32>,
        quad: std::ops::Range<u32>,
    },
    /// Everything between this and its `Close` is drawn on a surface of
    /// its own, because the group it belongs to composites as a unit
    /// before it meets what is under it.
    Open,
    /// That surface laid over the one under it, at the layer's own
    /// opacity, held to its mask — which is the item's, so the one quad
    /// carries it rather than each of the children — and brought down
    /// by its blend mode.
    Close {
        quad: std::ops::Range<u32>,
        blend: BlendMode,
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
    /// A layer's own surface brought down onto what is under it by a
    /// blend mode, which the fragment works out in full and writes over
    /// what was there.
    blend: wgpu::RenderPipeline,
    /// An adjustment layer, rewriting what is composited below it.
    adjust: wgpu::RenderPipeline,
    /// One box-blur pass, off one texture onto another: no multisampling
    /// and no stencil, since it draws one quad over the whole page.
    box_blur: wgpu::RenderPipeline,
    smear: wgpu::RenderPipeline,
    blocks: wgpu::RenderPipeline,
    /// The blurred copy coming back down onto what it was taken from.
    blur_down: wgpu::RenderPipeline,
    /// The same two passes again, painting from a gradient's ramp
    /// instead of a flat colour.
    shape_gradient: wgpu::RenderPipeline,
    cover_gradient: wgpu::RenderPipeline,
    /// Sets the stencil rather than flipping it, so overlapping pieces
    /// of one stroke union instead of cancelling.
    union: wgpu::RenderPipeline,
    text: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The stand-in bound wherever a pipeline declares a texture nothing
    /// is reading: one white texel, which as a mask lets everything
    /// through.
    open: wgpu::BindGroup,
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
                // The fragment stage wants it too: a vignette is measured
                // from the middle of the page out to its corner, so the
                // page's own size is part of the reading.
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
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
        // One layout for every pipeline: the page, whatever texture the
        // fragment paints from, and the mask it is held to. A pipeline
        // that reads neither still declares them, so a draw never has to
        // ask which groups the pipeline it is about to use expects.
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("page"),
            bind_group_layouts: &[
                &layout,
                &texture_layout,
                &texture_layout,
                // What a layer with a blend mode reads: a copy of what
                // is already on the surface it is coming down onto.
                &texture_layout,
            ],
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
        // What stands in for a texture nothing is reading: one white
        // texel. As a mask it lets everything through, and as the image
        // of a shape that paints from its own colour it is never
        // sampled — but the pipelines all declare both, so both are
        // always bound.
        let open = {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("no mask"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&[f32_to_f16(1.0)]),
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(2),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            let view = texture.create_view(&Default::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("no mask"),
                layout: &texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            })
        };

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
                4 => Float32x4, 5 => Float32x4
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
            layout: Some(&pipeline_layout),
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
        // The same quad as an image, brought down by a blend mode
        // rather than laid over: the fragment works out the whole
        // answer, backdrop included, so it replaces what is there
        // instead of blending into it.
        let blend = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blend"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_blend"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    blend: Some(wgpu::BlendState::REPLACE),
                    ..target.clone()
                })],
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
        // An adjustment layer: the same quad again, its fragment reading
        // what is under it and writing the answer back over it.
        let adjust = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("adjust"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_adjust"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    blend: Some(wgpu::BlendState::REPLACE),
                    ..target.clone()
                })],
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
        // Six of these make a blur: three box passes each way, ping-
        // ponging between two page-sized textures. Nothing else is on
        // them, so there is no multisampling to do and no stencil to
        // read.
        let box_blur = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("box blur"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_box"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    blend: Some(wgpu::BlendState::REPLACE),
                    ..target.clone()
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // One of these makes a motion blur: a single pass along the line
        // it was given, onto the first of the same pair.
        let smear = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("smear"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_smear"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    blend: Some(wgpu::BlendState::REPLACE),
                    ..target.clone()
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // Two of these make a pixelate: one pass along each axis, on the
        // same pair.
        let blocks = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pixelate"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_block"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    blend: Some(wgpu::BlendState::REPLACE),
                    ..target.clone()
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // And the blurred page coming back down, weighed by the layer's
        // opacity and its mask, over what it was taken from.
        let blur_down = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blur down"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_blur_down"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    blend: Some(wgpu::BlendState::REPLACE),
                    ..target.clone()
                })],
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
        let union = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stroke union"),
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
                wgpu::StencilOperation::Replace,
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
            layout: Some(&pipeline_layout),
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
            layout: Some(&pipeline_layout),
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
        let text = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("text"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_cover"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_text"),
                compilation_options: Default::default(),
                targets: &[Some(target)],
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
        Some(Self {
            device,
            queue,
            pipeline,
            stencil,
            cover,
            image,
            blend,
            adjust,
            box_blur,
            smear,
            blocks,
            blur_down,
            shape_gradient,
            cover_gradient,
            union,
            text,
            layout,
            texture_layout,
            sampler,
            open,
            adapter: info.name,
        })
    }

    /// Draw the whole page, or nothing when the document holds something
    /// this backend does not know how to draw.
    pub fn render(&self, doc: &Document) -> Option<Surface> {
        let (width, height) = (doc.meta.width, doc.meta.height);
        let mut scene = Scene::default();
        gather(doc, &mut scene)?;
        Some(self.draw(width, height, &scene))
    }

    /// Whether [`render`](Self::render) would draw this document.
    pub fn can_render(doc: &Document) -> bool {
        gather(doc, &mut Scene::default()).is_some()
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
                    format: if img.channels == 1 {
                        wgpu::TextureFormat::R16Float
                    } else {
                        wgpu::TextureFormat::Rgba16Float
                    },
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
                        bytes_per_row: Some(img.width * 2 * img.channels),
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

        // The passes to run, and the surfaces they run on. A group that
        // composites as a unit is drawn on one of its own and laid down
        // afterwards, which means ending the pass at every `Open` and
        // every `Close` — a pass has one set of attachments, and this is
        // where they change.
        let passes = plan(&scene.draws);
        let deep = passes.iter().map(|p| p.target).max().unwrap_or(0);
        // One surface per depth of isolation, reused by every group at
        // that depth: a group's surface is laid down the moment its
        // `Close` comes up, so the next one along can have it back.
        let mut surfaces = Vec::new();
        for _ in 0..deep {
            let multi = make(
                "group (multisampled)",
                SAMPLES,
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let flat = make(
                "group",
                1,
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
            );
            let flat_view = flat.create_view(&Default::default());
            let read = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("group"),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&flat_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            surfaces.push((
                multi.create_view(&Default::default()),
                flat_view,
                read,
                flat,
            ));
        }
        let attachment = |at: usize| -> (&wgpu::TextureView, &wgpu::TextureView) {
            match at {
                0 => (&multi_view, &view),
                n => (&surfaces[n - 1].0, &surfaces[n - 1].1),
            }
        };
        let resolved = |at: usize| -> &wgpu::Texture {
            match at {
                0 => &texture,
                n => &surfaces[n - 1].3,
            }
        };
        // A blend has to read what is already on the surface it is
        // coming down onto, and a pass cannot sample what it is drawing
        // into — so what is there is copied aside first. One is enough:
        // the passes run in order, and the copy is spent before the next
        // begins.
        let blending = passes
            .iter()
            .any(|p| p.lay.as_ref().is_some_and(Opening::reads_under));
        let under = blending.then(|| {
            let backdrop = make(
                "backdrop",
                1,
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let backdrop_view = backdrop.create_view(&Default::default());
            let read = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("backdrop"),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&backdrop_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            (backdrop, read)
        });
        // A blur is six box passes, three along each axis, ping-ponging
        // between two page-sized textures: the copy taken above goes in,
        // and the blurred page comes out of the second one. Neither is
        // multisampled — one quad covers the whole of each, so there are
        // no edges on them to sample.
        let blurring = passes.iter().any(|p| {
            matches!(
                p.lay,
                Some(Opening::Blur { .. })
                    | Some(Opening::Smear { .. })
                    | Some(Opening::Blocks { .. })
            )
        });
        let scratch: Vec<_> = if !blurring {
            Vec::new()
        } else {
            (0..2)
                .map(|_| {
                    let texture = make(
                        "blur",
                        1,
                        wgpu::TextureFormat::Rgba16Float,
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                    );
                    let view = texture.create_view(&Default::default());
                    let read = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("blur"),
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
                    });
                    (view, read)
                })
                .collect::<Vec<_>>()
        };

        let mut encoder = self.device.create_command_encoder(&Default::default());
        for (n, step) in passes.iter().enumerate() {
            let (colour, resolve) = attachment(step.target);
            // The multisampled attachment is only worth keeping when
            // this surface is drawn on again — which is what happens to
            // the page while a group is taken off to its own surface and
            // brought back. Every other pass throws it away, as the one
            // pass a page without groups needs always did.
            let again = passes[n + 1..].iter().any(|p| p.target == step.target);
            // Take the copy before the pass starts, for what will read
            // what is already there.
            if let (Some(laid), Some((backdrop, _))) = (&step.lay, &under) {
                if laid.reads_under() {
                    encoder.copy_texture_to_texture(
                        wgpu::ImageCopyTexture {
                            texture: resolved(step.target),
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::ImageCopyTexture {
                            texture: backdrop,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        size,
                    );
                }
            }
            // The box passes, before the pass that lays their result
            // down: the copy of what is under the layer goes in, and six
            // averagings later — horizontal, vertical, three times over,
            // which is the CPU renderer's Gaussian — the second scratch
            // texture holds the blurred page.
            if let (Some(Opening::Blur { steps, .. }), Some((_, backdrop)), Some(quads)) =
                (&step.lay, &under, &quads)
            {
                let along =
                    |axis: usize| steps.start + 6 * axis as u32..steps.start + 6 * axis as u32 + 6;
                for round in 0..6 {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("box blur"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &scratch[round % 2].0,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_pipeline(&self.box_blur);
                    pass.set_bind_group(0, &bind, &[]);
                    // The first round reads the copy; every one after it
                    // reads what the one before wrote.
                    pass.set_bind_group(
                        1,
                        if round == 0 {
                            backdrop
                        } else {
                            &scratch[(round + 1) % 2].1
                        },
                        &[],
                    );
                    pass.set_bind_group(2, &self.open, &[]);
                    pass.set_bind_group(3, &self.open, &[]);
                    pass.set_vertex_buffer(0, quads.slice(..));
                    pass.draw(along(round % 2), 0..1);
                }
            }
            // The two block passes, before the pass that lays their
            // result down: the copy of what is under the layer goes in,
            // one averaging along each axis, and the second scratch
            // texture holds the grid.
            if let (Some(Opening::Blocks { steps, .. }), Some((_, backdrop)), Some(quads)) =
                (&step.lay, &under, &quads)
            {
                let along =
                    |axis: usize| steps.start + 6 * axis as u32..steps.start + 6 * axis as u32 + 6;
                for round in 0..2 {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("pixelate"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &scratch[round].0,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    pass.set_pipeline(&self.blocks);
                    pass.set_bind_group(0, &bind, &[]);
                    // The first reads the copy; the second reads what the
                    // first wrote.
                    pass.set_bind_group(1, if round == 0 { backdrop } else { &scratch[0].1 }, &[]);
                    pass.set_bind_group(2, &self.open, &[]);
                    pass.set_bind_group(3, &self.open, &[]);
                    pass.set_vertex_buffer(0, quads.slice(..));
                    pass.draw(along(round), 0..1);
                }
            }
            // The one smearing pass, before the pass that lays its result
            // down: the copy of what is under the layer goes in, and the
            // averaging along the line leaves its answer on the first
            // scratch texture.
            if let (Some(Opening::Smear { along, .. }), Some((_, backdrop)), Some(quads)) =
                (&step.lay, &under, &quads)
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("smear"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &scratch[0].0,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.smear);
                pass.set_bind_group(0, &bind, &[]);
                pass.set_bind_group(1, backdrop, &[]);
                pass.set_bind_group(2, &self.open, &[]);
                pass.set_bind_group(3, &self.open, &[]);
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.draw(along.clone(), 0..1);
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("page"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: colour,
                    resolve_target: Some(resolve),
                    ops: wgpu::Operations {
                        load: if step.clear {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: if again {
                            wgpu::StoreOp::Store
                        } else {
                            wgpu::StoreOp::Discard
                        },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &stencil_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    // Cleared for every pass: a cover pass leaves the
                    // stencil as it found it, so there is never anything
                    // to carry across one.
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
                pass.set_bind_group(1, &self.open, &[]);
                pass.set_bind_group(2, &self.open, &[]);
                pass.set_bind_group(3, &self.open, &[]);
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.set_stencil_reference(0);
                // A group's surface comes down first, over whatever was
                // already on this one.
                if let Some(laid) = &step.lay {
                    if let Some((_, backdrop)) = &under {
                        pass.set_bind_group(3, backdrop, &[]);
                    }
                    let (quad, mask) = match laid {
                        Opening::Lay {
                            from,
                            quad,
                            mask,
                            blend,
                        } => {
                            if *blend == BlendMode::Normal || under.is_none() {
                                pass.set_pipeline(&self.image);
                            } else {
                                pass.set_pipeline(&self.blend);
                            }
                            pass.set_bind_group(1, &surfaces[from - 1].2, &[]);
                            (quad, mask)
                        }
                        Opening::Blur { quad, mask, .. } => {
                            pass.set_pipeline(&self.blur_down);
                            // Six rounds end on the second of the pair.
                            pass.set_bind_group(1, &scratch[1].1, &[]);
                            (quad, mask)
                        }
                        Opening::Smear { quad, mask, .. } => {
                            // Laid down the same way; the one pass left
                            // its answer on the first of the pair.
                            pass.set_pipeline(&self.blur_down);
                            pass.set_bind_group(1, &scratch[0].1, &[]);
                            (quad, mask)
                        }
                        Opening::Blocks { quad, mask, .. } => {
                            // Two passes end on the second of the pair.
                            pass.set_pipeline(&self.blur_down);
                            pass.set_bind_group(1, &scratch[1].1, &[]);
                            (quad, mask)
                        }
                        Opening::Adjust { quad, mask, table } => {
                            pass.set_pipeline(&self.adjust);
                            if let Some(at) = table {
                                pass.set_bind_group(1, &textures[*at], &[]);
                            }
                            (quad, mask)
                        }
                    };
                    pass.set_bind_group(2, mask.map_or(&self.open, |at| &textures[at]), &[]);
                    pass.draw(quad.clone(), 0..1);
                }
                for item in &scene.draws[step.items.clone()] {
                    // A masked layer holds its fragments to the coverage
                    // its mask lets through; one with none reads the
                    // white texel that stands in for a mask.
                    pass.set_bind_group(2, item.mask.map_or(&self.open, |at| &textures[at]), &[]);
                    match &item.draw {
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
                        Draw::Stroke { union, cover } => {
                            // The pieces set the stencil to one wherever
                            // any of them reaches, so overlapping ones
                            // union; the cover pass paints that and
                            // clears up behind itself.
                            pass.set_stencil_reference(1);
                            pass.set_pipeline(&self.union);
                            pass.draw(union.clone(), 0..1);
                            pass.set_stencil_reference(0);
                            pass.set_pipeline(&self.cover);
                            pass.draw(cover.clone(), 0..1);
                        }
                        Draw::Text { quad, texture } => {
                            pass.set_pipeline(&self.text);
                            pass.set_bind_group(1, &textures[*texture], &[]);
                            pass.draw(quad.clone(), 0..1);
                        }
                        Draw::Image { quad, texture } => {
                            pass.set_pipeline(&self.image);
                            pass.set_bind_group(1, &textures[*texture], &[]);
                            pass.draw(quad.clone(), 0..1);
                        }
                        // The four that cut a pass are drawn as its
                        // opening, above; nothing of them is left here.
                        Draw::Open
                        | Draw::Close { .. }
                        | Draw::Adjust { .. }
                        | Draw::Blur { .. }
                        | Draw::Smear { .. }
                        | Draw::Blocks { .. } => {}
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

/// One entry in painter's order: what to draw, and the mask its
/// fragments are held to — the scene texture the layer's coverage was
/// rasterized into, or nothing for the layers that have none.
struct Item {
    draw: Draw,
    mask: Option<usize>,
}

impl Item {
    /// A draw with no mask on it, which is what every builder makes;
    /// `collect` puts the mask on afterwards, once for everything the
    /// layer turned into.
    fn of(draw: Draw) -> Item {
        Item { draw, mask: None }
    }
}

/// The vertices and the order to draw them in.
#[derive(Default)]
struct Scene {
    vertices: Vec<Vertex>,
    draws: Vec<Item>,
    /// Everything the fragment shaders sample, premultiplied in linear
    /// light and half-precision: the pixels behind a placed image, and
    /// the baked ramp behind a gradient.
    textures: Vec<Image>,
    /// Which texture a placed resource went to, so a resource placed by
    /// several layers is uploaded once. Ramps are not shared: they are
    /// small, and two layers rarely carry the same one.
    ids: Vec<(String, usize)>,
}

/// A texture ready to upload: its size, how many channels each texel
/// has — four for a colour, one for a text block's coverage — and its
/// texels, half-precision either way.
struct Image {
    width: u32,
    height: u32,
    channels: u32,
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

/// Everything the page needs drawn, or `None` when some of it cannot be.
fn gather(doc: &Document, out: &mut Scene) -> Option<()> {
    if doc.meta.width > MAX_TEXTURE || doc.meta.height > MAX_TEXTURE {
        return None;
    }
    collect(doc, doc.root(), Transform::default(), 1.0, None, out)
}

/// Walk the tree in painter's order, turning what can be drawn into
/// quads; `None` the moment something cannot be.
fn collect(
    doc: &Document,
    group: NodeId,
    parent: Transform,
    opacity: f32,
    // The page rectangle everything drawn here is held inside, from a
    // frame somewhere above. Whole pixels, so holding a layer to it
    // takes either all of a pixel or none of it — which is why it can
    // ride the same coverage a mask does, without the double counting
    // that makes a soft mask on a group a surface of its own.
    bound: Option<chitrakar_render::ClipRect>,
    out: &mut Scene,
) -> Option<()> {
    for &child in doc.children_of(group).ok()? {
        one(doc, child, parent, opacity, bound, out)?;
    }
    Some(())
}

/// One layer, where its parent puts it, turned into quads.
///
/// Pulled out of the walk so a caller with a single layer in mind can
/// ask for it: a copy of another layer draws that layer where the copy
/// is, which is this same work with a different space and no parent to
/// have walked down from.
fn one(
    doc: &Document,
    child: NodeId,
    parent: Transform,
    opacity: f32,
    bound: Option<chitrakar_render::ClipRect>,
    out: &mut Scene,
) -> Option<()> {
    let node = doc.node(child).ok()?;
    if !node.visible || node.opacity <= 0.0 {
        return Some(());
    }
    // A live effect still belongs to the CPU.
    if !node.effects.is_empty() {
        return None;
    }
    // A layer held to the one under it shows only where that
    // layer's own alpha does. That alpha is the layer drawn aside,
    // which is what the CPU renderer does to it — so it arrives
    // here the way a mask does, as the same coverage in the same
    // texture, and the two renderers cannot come to disagree about
    // what clipping means either.
    //
    // Only where "its alpha" is a plain question, though: a base
    // that draws a picture of its own, at full strength, with
    // nothing over it. An adjustment has no picture and lets
    // everything through; a base that is faded, blended or masked
    // has an alpha that depends on how it was composited, and this
    // reading is of the layer alone. Any of those and the page goes
    // back to the CPU, which is always a safe answer.
    let held_to = if node.clipped {
        match chitrakar_render::clip_base(doc, child).ok()? {
            // Nothing under it to be held to — the first of a run
            // is what the rest are held to — so it draws whole.
            None => None,
            Some(base) => {
                let b = doc.node(base).ok()?;
                let draws = matches!(
                    b.kind,
                    NodeKind::Vector { .. } | NodeKind::Raster(_) | NodeKind::Text(_)
                );
                if !draws
                    || b.opacity < 1.0
                    || b.blend != BlendMode::Normal
                    || b.mask.is_some()
                    || !b.effects.is_empty()
                {
                    return None;
                }
                Some(base)
            }
        }
    } else {
        None
    };
    let t = parent.compose(node.transform);
    // What composites as a unit before it meets what is under it: a
    // layer with a blend mode, which has to see the whole of what it
    // is coming down onto; a group that is less than opaque, whose
    // contents meet each other at full strength before the result is
    // taken down together; a group with a mask, since holding each
    // child to it would take the coverage twice where two of them
    // overlap. Each of those is drawn on a surface of its own, and
    // the surface is what lands.
    //
    // A masked *leaf* is not one of them: its mask is a coverage its
    // own fragments can be multiplied by, exactly.
    // A group holding something that reads what is under it — an
    // adjustment, another blend — is isolated too, because that is
    // what decides what "under it" means: the CPU renderer asks the
    // same question, so both give the adjustment the same page to
    // work on.
    // A layer that rewrites what is under it has nothing left over
    // to blend against it, and the CPU renderer reads it that way:
    // it hands an adjustment and a filter their opacity and their
    // mask and never looks at the blend mode. A surface of its own
    // would be a different picture, so hand the page over instead.
    if matches!(node.kind, NodeKind::Adjustment(_) | NodeKind::Filter(_))
        && node.blend != BlendMode::Normal
    {
        return None;
    }
    let alone = node.blend != BlendMode::Normal
        || (matches!(node.kind, NodeKind::Group)
            && (node.opacity < 1.0
                || node.mask.is_some()
                || chitrakar_render::reads_backdrop(doc, child).ok()?));
    if alone {
        out.draws.push(Item::of(Draw::Open));
    }
    // What the layer itself is drawn at: its own opacity, unless it
    // is going on a surface of its own, where the opacity belongs to
    // the quad that brings the surface back.
    let alpha = if alone { 1.0 } else { node.opacity * opacity };
    // Where the layer's own drawing starts, so the mask can be put
    // on everything the layer turns into and nothing else.
    let mut mark = (out.vertices.len(), out.draws.len());
    match &node.kind {
        NodeKind::Group => {
            collect(
                doc,
                child,
                t,
                if alone { 1.0 } else { opacity },
                // A group on a surface of its own composites
                // unbounded and is cut when that surface is laid
                // down; one that is not hands the bound to each of
                // its children.
                if alone { None } else { bound },
                out,
            )?;
        }
        NodeKind::Vector {
            shape,
            fill,
            stroke,
            gradient,
        } => vector(
            doc,
            child,
            shape,
            fill.clone(),
            stroke.as_ref(),
            gradient.as_ref(),
            t,
            alpha,
            out,
        )?,
        NodeKind::Raster(raster) => {
            let Some(res) = doc.resource(&raster.resource_id) else {
                // A resource whose pixels never came back is drawn by
                // nobody; the CPU skips it too.
                return Some(());
            };
            if res.rgba8.is_empty() {
                return Some(());
            }
            if res.width > MAX_TEXTURE || res.height > MAX_TEXTURE {
                return None;
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
            let size = [res.width as f32, res.height as f32];
            // The quad is the image's own box; its local coordinates
            // are the texture's, so the vertex shader passes them
            // straight through as texture coordinates.
            let mut verts = quad(t, size, [0.0; 4], [0.0, 0.0, 0.0, alpha], [0.0; 4], 0.0);
            for v in &mut verts {
                v.local = [v.local[0] / size[0], v.local[1] / size[1]];
            }
            let quad = out.push(verts);
            out.draws.push(Item::of(Draw::Image { quad, texture: at }));
        }
        NodeKind::Text(spec) => {
            let color = premultiplied_color(spec.fill.clone(), alpha)?;
            text(spec, t, color, out)?;
        }
        NodeKind::Adjustment(adj) => {
            // An adjustment rewrites what is composited below it,
            // weighted by its own opacity and its mask. It reads
            // that from the copy a blend reads, which means a pass
            // of its own, which `plan` cuts for it.
            let plan = adjustment_of(doc, adj)?;
            let table = plan.table.map(|img| {
                let at = out.textures.len();
                out.textures.push(img);
                at
            });
            let quad = out.push(page_quad(doc, alpha, plan.params, plan.grad, plan.extra));
            out.draws.push(Item::of(Draw::Adjust { quad, table }));
        }
        NodeKind::Filter(filter) => {
            // A filter that is a function of one pixel and of where
            // that pixel sits on the page is an adjustment in every
            // way this backend cares about: it rewrites what is
            // composited below it, from the same copy taken aside,
            // in the same pass. The ones that read a neighbourhood
            // are declined by `filter_of` and stay the CPU's.
            // A filter's radius is written in the space it lives
            // in, so a group that scales stretches it — which is the
            // reading the CPU renderer takes.
            match filter_of(filter, parent)? {
                Filtering::Pointwise(params, grad) => {
                    let quad = out.push(page_quad(doc, alpha, params, grad, [0.0; 3]));
                    out.draws.push(Item::of(Draw::Adjust { quad, table: None }));
                }
                Filtering::Blur { radius, sharpen } => {
                    let axis =
                        |a: f32| page_quad(doc, 1.0, [radius, a, 0.0, 0.0], [0.0; 4], [0.0; 3]);
                    let mut steps = out.push(axis(0.0));
                    steps.end = out.push(axis(1.0)).end;
                    let quad = out.push(page_quad(
                        doc,
                        alpha,
                        [sharpen, 0.0, 0.0, 0.0],
                        [0.0; 4],
                        [0.0; 3],
                    ));
                    out.draws.push(Item::of(Draw::Blur { steps, quad }));
                }
                Filtering::Blocks { across, down } => {
                    let mut steps = out.push(page_quad(doc, 1.0, across, [0.0; 4], [0.0; 3]));
                    steps.end = out.push(page_quad(doc, 1.0, down, [0.0; 4], [0.0; 3])).end;
                    // Laid down the way a blur is: nothing is added back,
                    // so the amount that makes a sharpen is zero.
                    let quad = out.push(page_quad(doc, alpha, [0.0; 4], [0.0; 4], [0.0; 3]));
                    out.draws.push(Item::of(Draw::Blocks { steps, quad }));
                }
                Filtering::Smear { taps, step } => {
                    let along = out.push(page_quad(
                        doc,
                        1.0,
                        [taps, step[0], step[1], 0.0],
                        [0.0; 4],
                        [0.0; 3],
                    ));
                    // Laid down the way a blur is: what was under it,
                    // mixed with the smeared copy by the layer's opacity
                    // and its mask. Nothing is added back, so the amount
                    // that makes a sharpen out of a blur is zero here.
                    let quad = out.push(page_quad(doc, alpha, [0.0; 4], [0.0; 4], [0.0; 3]));
                    out.draws.push(Item::of(Draw::Smear { along, quad }));
                }
                // Nothing asked for is nothing drawn, and nothing
                // drawn wants no mask over it either.
                Filtering::Nothing => return Some(()),
            }
        }
        // A frame is a group with a size of its own: a ground
        // painted inside it and everything under it held to its
        // rectangle. Upright and composited like its contents, it
        // is nothing but a narrower region to paint in — which is
        // how the CPU renderer reads it too, so an adjustment
        // inside a frame sees the page below the frame on both.
        //
        // The rectangle is rounded to whole pixels there, because a
        // frame's edge is a page edge and a page edge is crisp; a
        // frame whose box does not land on whole pixels would want
        // an antialiased edge here and a rounded one there, so it
        // goes back. So does a turned frame, and one composited as
        // a whole — both of which the CPU draws on a surface of its
        // own, which is a different picture from this.
        NodeKind::Artboard {
            width,
            height,
            background,
            ..
        } => {
            let upright = t.b.abs() < 1e-6 && t.c.abs() < 1e-6;
            if !upright || alone || node.opacity < 1.0 || node.mask.is_some() {
                return None;
            }
            let box_ = chitrakar_render::transformed_box(t, [0.0, 0.0, *width, *height]);
            let chitrakar_render::Bounds::Rect(fx0, fy0, fx1, fy1) = box_ else {
                return None;
            };
            if [fx0, fy0, fx1, fy1]
                .iter()
                .any(|v| (v - v.round()).abs() > 1e-3)
            {
                return None;
            }
            let (pw, ph) = (doc.meta.width, doc.meta.height);
            let board = chitrakar_render::ClipRect {
                x0: (fx0.round().max(0.0) as u32).min(pw),
                y0: (fy0.round().max(0.0) as u32).min(ph),
                x1: (fx1.round().max(0.0) as u32).min(pw),
                y1: (fy1.round().max(0.0) as u32).min(ph),
            };
            let inside = match bound {
                Some(outer) => outer.intersect(board),
                None => board,
            };
            if inside.is_empty() {
                return Some(());
            }
            // The ground is the frame's own rectangle, filled. Laid
            // down as the shape it is, so it goes through the same
            // arithmetic every other filled rectangle does.
            if let Some(ground) = background {
                vector(
                    doc,
                    child,
                    &VectorShape::Rect {
                        width: *width,
                        height: *height,
                        radius: 0.0,
                    },
                    Some(ground.clone()),
                    None,
                    None,
                    t,
                    alpha,
                    out,
                )?;
                // The ground is inside the frame like everything
                // else: a frame hanging off the page shows only the
                // part of it that is on the page.
                let (at, quad) = mask_texture(doc, child, None, None, Some(inside), parent, out)?;
                for v in &mut out.vertices[mark.0..] {
                    v.mask = quad;
                }
                for item in &mut out.draws[mark.1..] {
                    item.mask = at;
                }
            }
            collect(doc, child, t, opacity, Some(inside), out)?;
            // Everything inside took the bound as it was collected,
            // and the frame itself has no mask — it was declined
            // above if it had one — so there is nothing left to put
            // over what was drawn.
            return Some(());
        }
        // A copy draws what the original draws, where the copy is: the
        // original's own placement is undone first, so moving the
        // original moves only the original. Where the copy stands in
        // for the original's own children with layers of its own, those
        // are what it draws, in the copy's own space.
        //
        // Composited like the original would be — nothing of its own to
        // apply — it is drawn straight in, which is the reading the CPU
        // renderer takes. A copy that is faded, blended or masked goes
        // on a surface of its own there, and that is a different
        // picture from this, so it goes back.
        NodeKind::Instance { of, .. } => {
            if alone || node.opacity < 1.0 || node.mask.is_some() {
                return None;
            }
            let master = doc.node(*of).ok()?;
            let back = chitrakar_render::invert(master.transform)?;
            let stand_ins = if chitrakar_render::takes_stand_ins(doc, *of) {
                chitrakar_render::copy_children(doc, child).ok()?
            } else {
                Vec::new()
            };
            if stand_ins.is_empty() {
                one(doc, *of, t.compose(back), opacity, bound, out)?;
            } else {
                for part in stand_ins {
                    one(doc, part, t, opacity, bound, out)?;
                }
            }
            return Some(());
        }
        _ => return None,
    }
    if alone {
        // The mask and the opacity go on the quad that lays the
        // surface down, not on what was drawn into it.
        mark = (out.vertices.len(), out.draws.len());
        let quad = out.push(page_quad(
            doc,
            node.opacity * opacity,
            [blend_index(node.blend) as f32, 0.0, 0.0, 0.0],
            [0.0; 4],
            [0.0; 3],
        ));
        out.draws.push(Item::of(Draw::Close {
            quad,
            blend: node.blend,
        }));
    }
    // The mask, once, over everything the layer drew: the CPU
    // renderer rasterizes the coverage and the fragments read it.
    // What a layer is held to rides the same texture — two
    // coverages a layer is held back by are one coverage, and a
    // fragment reads it once.
    if node.mask.is_some() || held_to.is_some() || bound.is_some() {
        let (at, box_) = mask_texture(doc, child, node.mask.as_ref(), held_to, bound, parent, out)?;
        for v in &mut out.vertices[mark.0..] {
            v.mask = box_;
        }
        for item in &mut out.draws[mark.1..] {
            item.mask = at;
        }
    }
    Some(())
}

/// Rasterize a layer's mask into a scene texture, and say where it went
/// and the box of page pixels it was rasterized over.
///
/// The coverage comes from `chitrakar_render::mask_plane_over` — the same
/// reading the CPU compositor does at every pixel of a masked layer — so
/// the two renderers cannot come to disagree about what a mask means.
/// Only the layer's own box is rasterized: outside it there is nothing
/// of the layer for a mask to hold back.
fn mask_texture(
    doc: &Document,
    id: NodeId,
    mask: Option<&chitrakar_doc::Mask>,
    held_to: Option<NodeId>,
    bound: Option<chitrakar_render::ClipRect>,
    parent: Transform,
    out: &mut Scene,
) -> Option<(Option<usize>, [f32; 4])> {
    let page = (doc.meta.width, doc.meta.height);
    // Through the space the layer is being drawn in rather than through
    // the one the document places it in: a copy of a group draws that
    // group's layers somewhere else entirely, and a coverage rasterized
    // over the original's box would hold the copy back by what is
    // happening at the other end of the page.
    let placed = match chitrakar_render::bounds_in_parent_space(doc, id).ok()? {
        chitrakar_render::Bounds::Rect(x0, y0, x1, y1) => {
            chitrakar_render::transformed_box(parent, [x0, y0, x1, y1])
        }
        other => other,
    };
    let (bx0, by0, bx1, by1) = match placed {
        chitrakar_render::Bounds::Rect(x0, y0, x1, y1) => (x0, y0, x1, y1),
        // A layer that reaches nowhere has nothing for a mask to hold
        // back; one that reaches everywhere — an adjustment — wants the
        // whole page.
        chitrakar_render::Bounds::None => return Some((None, NO_MASK)),
        chitrakar_render::Bounds::Everything => (0.0, 0.0, page.0 as f32, page.1 as f32),
    };
    // A pixel of margin: the quads are grown by a device pixel so an
    // edge is not cut short, and the coverage has to reach as far.
    let x0 = (bx0.floor() as i64 - 1).clamp(0, page.0 as i64) as u32;
    let y0 = (by0.floor() as i64 - 1).clamp(0, page.1 as i64) as u32;
    let x1 = (bx1.ceil() as i64 + 1).clamp(0, page.0 as i64) as u32;
    let y1 = (by1.ceil() as i64 + 1).clamp(0, page.1 as i64) as u32;
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w == 0 || h == 0 {
        // Nothing of the layer is on the page; a box of no width says
        // there is no mask, and there is nothing to hold back either.
        return Some((None, NO_MASK));
    }
    // No size check: the box is clamped to the page, and a page bigger
    // than the textures this backend asked for was handed back before
    // any of this.
    let clip = chitrakar_render::ClipRect { x0, y0, x1, y1 };
    let mut cover = match mask {
        Some(mask) => chitrakar_render::mask_plane_over(doc, mask, parent, clip, page),
        None => vec![1.0; (w * h) as usize],
    };
    if let Some(base) = held_to {
        let held = chitrakar_render::layer_coverage_at(doc, base, parent).ok()?;
        for (i, c) in cover.iter_mut().enumerate() {
            let (x, y) = (x0 + i as u32 % w, y0 + i as u32 / w);
            *c *= held[(y * page.0 + x) as usize];
        }
    }
    // A frame somewhere above: everything drawn inside one is held to
    // its rectangle, which is whole pixels, so this takes all of a
    // pixel or none of it.
    if let Some(inside) = bound {
        for (i, c) in cover.iter_mut().enumerate() {
            let (x, y) = (x0 + i as u32 % w, y0 + i as u32 / w);
            if x < inside.x0 || x >= inside.x1 || y < inside.y0 || y >= inside.y1 {
                *c = 0.0;
            }
        }
    }
    let at = out.textures.len();
    out.textures.push(Image {
        width: w,
        height: h,
        channels: 1,
        texels: cover.iter().map(|c| f32_to_f16(*c)).collect(),
    });
    Some((Some(at), [x0 as f32, y0 as f32, w as f32, h as f32]))
}

/// Draw a text block: the whole block rasterized to coverage at the size
/// it is seen at — by the renderer that owns that decision, so the
/// bitmap is the one the CPU would have sampled — and read back off a
/// quad over the block's own box.
fn text(
    spec: &chitrakar_doc::TextSpec,
    t: Transform,
    color: [f32; 4],
    out: &mut Scene,
) -> Option<()> {
    let [bx0, by0, bx1, by1] = chitrakar_render::text::bounds(spec);
    if !(bx1 > bx0 && by1 > by0) {
        return Some(());
    }
    let (raster, scale) = chitrakar_render::text_raster(spec, t);
    if raster.width == 0 || raster.height == 0 {
        return Some(());
    }
    if raster.width + 2 > MAX_TEXTURE || raster.height + 2 > MAX_TEXTURE {
        return None;
    }
    // A transparent row and column around the coverage, so that off the
    // edge the sampler reads no ink rather than smearing the border —
    // which is what the CPU's own sampler does there.
    let (w, h) = (raster.width + 2, raster.height + 2);
    let mut texels = vec![0u16; (w * h) as usize];
    for y in 0..raster.height {
        for x in 0..raster.width {
            texels[((y + 1) * w + x + 1) as usize] = f32_to_f16(raster.sample(x, y));
        }
    }
    let at = out.textures.len();
    out.textures.push(Image {
        width: w,
        height: h,
        channels: 1,
        texels,
    });
    // The quad is the block's box, grown by a device pixel: the CPU
    // walks whole pixels of that box, so the last of them can reach a
    // little past it.
    let (ox, oy) = raster.origin;
    let m = 1.0 / device_scale(t);
    let corner = |x: f32, y: f32| Vertex {
        doc: place(t, [x, y]),
        // The raster's own texel coordinates, which is what the shader
        // needs to read it the way the CPU reads it.
        local: [(x - ox) * scale, (y - oy) * scale],
        params: [0.0; 4],
        color,
        grad: [0.0; 4],
        mask: NO_MASK,
    };
    let (x0, y0, x1, y1) = (bx0 - m, by0 - m, bx1 + m, by1 + m);
    let quad = out.push(vec![
        corner(x0, y0),
        corner(x1, y0),
        corner(x1, y1),
        corner(x0, y0),
        corner(x1, y1),
        corner(x0, y1),
    ]);
    out.draws.push(Item::of(Draw::Text { quad, texture: at }));
    Some(())
}

/// Turn one vector layer into quads: its fill, and then its stroke over
/// it, which is the order the CPU paints them in. `None` declines the
/// page.
#[allow(clippy::too_many_arguments)]
fn vector(
    doc: &Document,
    id: NodeId,
    shape: &VectorShape,
    fill: Option<chitrakar_color::AuthoredColor>,
    stroke: Option<&chitrakar_doc::Stroke>,
    gradient: Option<&chitrakar_doc::Gradient>,
    t: Transform,
    alpha: f32,
    out: &mut Scene,
) -> Option<()> {
    // A gradient paints in place of the flat fill, from a ramp baked
    // here once and sampled there per pixel; the layer's own opacity
    // scales it in the fragment, so two layers could share a ramp even
    // at different opacities.
    let paint = match gradient {
        // No stops is nothing to paint, as it is on the CPU — and the
        // flat fill stays covered up.
        Some(g) if g.stops().is_empty() => None,
        Some(g) => {
            let (ramp, geom, radial) = bake(g)?;
            let at = out.textures.len();
            out.textures.push(ramp);
            let kind = if radial { 1.0 } else { 0.0 };
            Some(([kind, 0.0, 0.0, alpha], geom, Some(at)))
        }
        None => match fill {
            Some(c) => Some((premultiplied_color(c, alpha)?, [0.0; 4], None)),
            None => None,
        },
    };
    let ink = match stroke {
        Some(s) if s.width > 0.0 => Some((premultiplied_color(s.color.clone(), alpha)?, s)),
        _ => None,
    };
    if paint.is_none() && ink.is_none() {
        return Some(());
    }

    // A path is stencilled and covered: parity gives the even-odd fill
    // the CPU draws, holes and crossings included; a stroke is the union
    // of the round-capped segments the CPU tests against, which the
    // stencil takes as geometry.
    if let VectorShape::Path { .. } = shape {
        if let Some((color, grad, ramp)) = paint {
            fill_path(doc, id, shape, t, color, grad, ramp, out);
        }
        if let Some((color, s)) = ink {
            stroke_path(shape, t, color, s, out);
        }
        return Some(());
    }

    let (size, radius) = match shape {
        VectorShape::Rect {
            width,
            height,
            radius,
        } => (
            [*width, *height],
            radius.max(0.0).min(width.min(*height).max(0.0) / 2.0),
        ),
        VectorShape::Ellipse { rx, ry } => ([rx * 2.0, ry * 2.0], 0.0),
        VectorShape::Path { .. } => unreachable!("handled above"),
    };
    if !(size[0] > 0.0 && size[1] > 0.0) {
        return Some(());
    }
    let ellipse = matches!(shape, VectorShape::Ellipse { .. });
    let kind = if ellipse { 1.0 } else { 0.0 };
    if let Some((color, grad, ramp)) = paint {
        let quad = out.push(quad(
            t,
            size,
            [size[0], size[1], radius, kind],
            color,
            grad,
            1.5,
        ));
        out.draws.push(Item::of(Draw::Shape { quad, ramp }));
    }
    if let Some((color, s)) = ink {
        // A band between two outlines: the shape shrunk by one figure
        // and grown by the other. Which side of its edge the band lies
        // on is which of those two carries the width.
        let (shrink, grow) = match chitrakar_doc::stroke_align(shape, s) {
            chitrakar_doc::StrokeAlign::Inside => (s.width, 0.0),
            chitrakar_doc::StrokeAlign::Centre => (s.width / 2.0, s.width / 2.0),
            chitrakar_doc::StrokeAlign::Outside => (0.0, s.width),
        };
        let quad = out.push(quad(
            t,
            size,
            [size[0], size[1], radius, kind + 2.0],
            color,
            [shrink, grow, 0.0, 0.0],
            // The quad has to reach whatever the band grows to, plus the
            // pixel every edge is softened over.
            1.5 + grow * device_scale(t),
        ));
        out.draws.push(Item::of(Draw::Shape { quad, ramp: None }));
    }
    Some(())
}

/// An authored colour premultiplied into linear light and scaled by the
/// layer's opacity. `None` declines the page: ink authored for a press
/// resolves through the document's profile, which is the CPU's business.
fn premultiplied_color(color: chitrakar_color::AuthoredColor, alpha: f32) -> Option<[f32; 4]> {
    // A colour standing for a swatch is whatever that swatch means, which
    // is what decides here: a name for an sRGB is drawable, a name for an
    // ink is the CPU's business exactly as the ink itself is.
    let chitrakar_color::AuthoredColor::Srgb { .. } = color.flat() else {
        return None;
    };
    let c = chitrakar_color::to_working(&color);
    Some([c.r * alpha, c.g * alpha, c.b * alpha, c.a * alpha])
}

/// Stencil a path's rings and cover them.
#[allow(clippy::too_many_arguments)]
fn fill_path(
    doc: &Document,
    id: NodeId,
    shape: &VectorShape,
    t: Transform,
    color: [f32; 4],
    grad: [f32; 4],
    ramp: Option<usize>,
    out: &mut Scene,
) {
    let rings: Vec<Vec<[f32; 2]>> = chitrakar_render::shape_rings(shape)
        .into_iter()
        .map(|ring| ring.into_iter().map(|p| place(t, p)).collect())
        .filter(|ring: &Vec<[f32; 2]>| ring.len() >= 3)
        .collect();
    if rings.is_empty() {
        return;
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
    // The cover quad is the layer's own box carried through its
    // transform — a parallelogram that holds every point the fill can
    // reach, grown by a device pixel so it cannot cut the edge short.
    // Its corners carry the box's normalized coordinates, which a
    // gradient interpolates across however the layer is turned.
    let Ok(Some([x0, y0, x1, y1])) = chitrakar_render::local_bounds_of(doc, id) else {
        return;
    };
    let (mu, mv) = (
        1.0 / device_scale(t) / (x1 - x0),
        1.0 / device_scale(t) / (y1 - y0),
    );
    let corner = |u: f32, v: f32| Vertex {
        doc: place(t, [x0 + (x1 - x0) * u, y0 + (y1 - y0) * v]),
        local: [u, v],
        params: [0.0; 4],
        color,
        grad,
        mask: NO_MASK,
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
    out.draws.push(Item::of(Draw::Path {
        stencil,
        cover,
        ramp,
    }));
}

/// Draw a path's stroke: the very region the CPU tests a sample against,
/// laid down as geometry. `chitrakar_render::stroke_pieces` states that
/// region as a union of convex pieces — a band per segment, a disc where
/// an end or a corner is round, a polygon where one is squared, bevelled
/// or mitred — and each piece is tessellated here. The stencil takes the
/// union (a pixel is in the stroke if any piece covers it, however the
/// pieces overlap), and one quad covers it.
fn stroke_path(
    shape: &VectorShape,
    t: Transform,
    color: [f32; 4],
    stroke: &chitrakar_doc::Stroke,
    out: &mut Scene,
) {
    let scale = device_scale(t);
    let mut tris: Vec<Vertex> = Vec::new();
    let mut box_ = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    let vertex = |p: [f32; 2], box_: &mut [f32; 4]| {
        let doc = place(t, p);
        *box_ = [
            box_[0].min(doc[0]),
            box_[1].min(doc[1]),
            box_[2].max(doc[0]),
            box_[3].max(doc[1]),
        ];
        Vertex {
            doc,
            ..Default::default()
        }
    };
    for piece in chitrakar_render::stroke_pieces(shape, stroke) {
        match piece {
            // The boundary of a segment whose half-width runs linearly
            // from one end to the other is a straight line, so the band
            // between the two rims is a quadrilateral.
            chitrakar_render::StrokePiece::Band { a, b, ha, hb } => {
                let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                let len = (dx * dx + dy * dy).sqrt();
                if len < 1e-9 {
                    continue;
                }
                let (nx, ny) = (-dy / len, dx / len);
                let corners = [
                    [a[0] + nx * ha, a[1] + ny * ha],
                    [b[0] + nx * hb, b[1] + ny * hb],
                    [b[0] - nx * hb, b[1] - ny * hb],
                    [a[0] - nx * ha, a[1] - ny * ha],
                ];
                for k in [0, 1, 2, 0, 2, 3] {
                    tris.push(vertex(corners[k], &mut box_));
                }
            }
            chitrakar_render::StrokePiece::Disc { at, r } => {
                if r <= 0.0 {
                    continue;
                }
                // Enough sides that the fan is smooth at the size it is
                // actually seen at.
                let sides = ((r * scale) as usize + 8).clamp(8, 64);
                for k in 0..sides {
                    let angle = |k: usize| k as f32 / sides as f32 * std::f32::consts::TAU;
                    let (a0, a1) = (angle(k), angle(k + 1));
                    for q in [
                        at,
                        [at[0] + r * a0.cos(), at[1] + r * a0.sin()],
                        [at[0] + r * a1.cos(), at[1] + r * a1.sin()],
                    ] {
                        tris.push(vertex(q, &mut box_));
                    }
                }
            }
            // Convex, so a fan from its first point covers it.
            chitrakar_render::StrokePiece::Corner(pts, n) => {
                for k in 1..n - 1 {
                    for q in [pts[0], pts[k], pts[k + 1]] {
                        tris.push(vertex(q, &mut box_));
                    }
                }
            }
        }
    }
    if tris.is_empty() {
        return;
    }
    let union = out.push(tris);
    // A device-space box around the geometry, grown by a pixel: the
    // stroke carries no gradient, so the cover quad needs no coordinates
    // of its own.
    let corner = |x: f32, y: f32| Vertex {
        doc: [x, y],
        color,
        ..Default::default()
    };
    let (x0, y0, x1, y1) = (box_[0] - 1.0, box_[1] - 1.0, box_[2] + 1.0, box_[3] + 1.0);
    let cover = out.push(vec![
        corner(x0, y0),
        corner(x1, y0),
        corner(x1, y1),
        corner(x0, y0),
        corner(x1, y1),
        corner(x0, y1),
    ]);
    out.draws.push(Item::of(Draw::Stroke { union, cover }));
}

/// A local-space point on the page.
fn place(t: Transform, p: [f32; 2]) -> [f32; 2] {
    [t.a * p[0] + t.c * p[1] + t.e, t.b * p[0] + t.d * p[1] + t.f]
}

/// How many device pixels a unit of the layer's own space spans.
fn device_scale(t: Transform) -> f32 {
    (t.a.abs() + t.c.abs()).max(t.b.abs() + t.d.abs()).max(1e-6)
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
        let chitrakar_color::AuthoredColor::Srgb { .. } = stop.color.flat() else {
            return None;
        };
        stops.push((stop.offset, chitrakar_color::to_working(&stop.color)));
    }
    stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut texels = Vec::with_capacity(RAMP as usize * 4);
    for i in 0..RAMP {
        let c = chitrakar_render::ramp_color(&stops, i as f32 / (RAMP - 1) as f32);
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
            channels: 4,
            texels,
        },
        geom,
        radial,
    ))
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
        channels: 4,
        texels,
    }
}

/// The parameters an adjustment is stated by, laid out the way the
/// shader reads them: which adjustment it is, then up to seven numbers.
/// `None` for the ones this backend has not learnt — a curve and a
/// gradient map are read off tables, and the two that speak in bands of
/// colour want the whole HSL round trip, so they wait.
fn adjustment_of(doc: &Document, adj: &chitrakar_doc::Adjustment) -> Option<Adjusting> {
    use chitrakar_doc::Adjustment as A;
    let plain = |params: [f32; 4], grad: [f32; 4]| Adjusting {
        params,
        grad,
        extra: [0.0; 3],
        table: None,
    };
    Some(match adj {
        A::Exposure { stops } => plain([1.0, *stops, 0.0, 0.0], [0.0; 4]),
        A::BrightnessContrast {
            brightness,
            contrast,
        } => plain([2.0, *brightness, *contrast, 0.0], [0.0; 4]),
        A::HueSaturation {
            hue_degrees,
            saturation,
            lightness,
        } => plain([3.0, *hue_degrees, *saturation, *lightness], [0.0; 4]),
        A::Levels {
            in_black,
            in_white,
            gamma,
            out_black,
            out_white,
        } => plain(
            [4.0, *in_black, *in_white, *gamma],
            [*out_black, *out_white, 0.0, 0.0],
        ),
        A::WhiteBalance { temperature, tint } => plain([5.0, *temperature, *tint, 0.0], [0.0; 4]),
        A::Vibrance { amount } => plain([6.0, *amount, 0.0, 0.0], [0.0; 4]),
        A::BlackAndWhite { red, green, blue } => plain([7.0, *red, *green, *blue], [0.0; 4]),
        A::Invert { amount } => plain([8.0, *amount, 0.0, 0.0], [0.0; 4]),
        A::ShadowsHighlights {
            shadows,
            highlights,
        } => plain([9.0, *shadows, *highlights, 0.0], [0.0; 4]),
        // The three read off tables the CPU renderer builds, so neither
        // renderer can read a table the other did not write.
        A::Curves {
            points,
            red,
            green,
            blue,
        } => {
            let line: Vec<f32> = (0..=256).map(|i| i as f32 / 256.0).collect();
            let of = |pts: &Vec<[f32; 2]>| {
                if pts.len() >= 2 {
                    chitrakar_render::curve_lut(pts)
                } else {
                    line.clone()
                }
            };
            let (master, r, g, b) = (of(points), of(red), of(green), of(blue));
            let mut texels = Vec::with_capacity(master.len() * 4);
            for i in 0..master.len() {
                for v in [master[i], r[i], g[i], b[i]] {
                    texels.push(f32_to_f16(v));
                }
            }
            Adjusting {
                params: [10.0, 0.0, 0.0, 0.0],
                grad: [0.0; 4],
                extra: [0.0; 3],
                table: Some(Image {
                    width: master.len() as u32,
                    height: 1,
                    channels: 4,
                    texels,
                }),
            }
        }
        A::GradientMap { .. } => {
            // The prepared ramp is the one that knows a CMYK document's
            // press profile, which is why it is asked for rather than
            // the stops themselves.
            let Some(chitrakar_render::Prepared::Ramp(ramp)) = chitrakar_render::prepare(doc, adj)
            else {
                // Fewer than two stops is not a ramp: nothing to map
                // through, and the picture is left as it is.
                return Some(plain([0.0; 4], [0.0; 4]));
            };
            let steps = chitrakar_render::RampLut::steps();
            let mut texels = Vec::with_capacity((steps + 1) * 4);
            for i in 0..=steps {
                let c = ramp.at(i as f32 / steps as f32);
                for v in [c.r, c.g, c.b, c.a] {
                    texels.push(f32_to_f16(v));
                }
            }
            Adjusting {
                params: [11.0, 0.0, 0.0, 0.0],
                grad: [0.0; 4],
                extra: [0.0; 3],
                table: Some(Image {
                    width: (steps + 1) as u32,
                    height: 1,
                    channels: 4,
                    texels,
                }),
            }
        }
        A::SelectiveHsl { bands } => {
            let mut texels = Vec::with_capacity(6 * 4);
            for i in 0..6 {
                let band = bands.get(i).copied().unwrap_or([0.0; 3]);
                for v in [band[0], band[1], band[2], 0.0] {
                    texels.push(f32_to_f16(v));
                }
            }
            Adjusting {
                params: [12.0, 0.0, 0.0, 0.0],
                grad: [0.0; 4],
                extra: [0.0; 3],
                table: Some(Image {
                    width: 6,
                    height: 1,
                    channels: 4,
                    texels,
                }),
            }
        }
        A::ColorBalance {
            shadows,
            midtones,
            highlights,
            preserve_luminosity,
        } => Adjusting {
            params: [13.0, shadows[0], shadows[1], shadows[2]],
            grad: [midtones[0], midtones[1], midtones[2], highlights[0]],
            extra: [
                highlights[1],
                highlights[2],
                if *preserve_luminosity { 1.0 } else { 0.0 },
            ],
            table: None,
        },
    })
}

/// What a filter's quad carries, or `None` for one this backend does not
/// draw.
///
/// Blur, sharpen and pixelate each read a *neighbourhood* rather than a
/// pixel: they want the surface under them sampled many times over, at
/// offsets, which is passes of its own rather than the single copy-aside
/// everything here works from. Until that exists they are the CPU's.
fn filter_of(filter: &chitrakar_doc::Filter, view: Transform) -> Option<Filtering> {
    use chitrakar_doc::Filter as F;
    // A filter's radius is written in the space it lives in, so a group
    // that scales stretches it — and a smear, which has a direction as
    // well as a length, goes through the whole of that space rather than
    // through the scale alone.
    let scale = view.a.hypot(view.b).max(view.c.hypot(view.d));
    let point = |params: [f32; 4], grad: [f32; 4]| Filtering::Pointwise(params, grad);
    // The W3C's box size for a Gaussian after three passes each way,
    // read off the CPU renderer so the two blur by the same amount.
    let boxes = |sigma: f32| {
        let sigma = sigma * scale;
        (sigma > 0.01).then(|| {
            let d =
                ((sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0) + 0.5).floor() as i32;
            (d.max(1) / 2).max(1) as f32
        })
    };
    Some(match filter {
        F::Vignette {
            amount,
            radius,
            softness,
        } => point([14.0, *amount, *radius, *softness], [0.0; 4]),
        F::Noise {
            amount,
            grain,
            mono,
            seed,
        } => point(
            [15.0, *amount, *grain, if *mono { 1.0 } else { 0.0 }],
            // A seed is a whole 32 bits and a vertex carries floats, so
            // it travels as two halves that a float holds exactly.
            [(seed & 0xffff) as f32, (seed >> 16) as f32, 0.0, 0.0],
        ),
        F::GaussianBlur { sigma } => match boxes(*sigma) {
            Some(radius) => Filtering::Blur {
                radius,
                sharpen: 0.0,
            },
            // A blur too small to move a pixel does nothing at all, and
            // the CPU renderer returns without touching the page.
            None => Filtering::Nothing,
        },
        F::Sharpen { sigma, amount } => match boxes(*sigma) {
            // A sharpen with no amount is the same nothing, and one is
            // easy enough to author by dragging a slider back to zero.
            Some(radius) if *amount != 0.0 => Filtering::Blur {
                radius,
                sharpen: *amount,
            },
            _ => Filtering::Nothing,
        },
        // A grid of squares, each the average of what it covered. The
        // grid is laid out in the document rather than on the page, so
        // which block a pixel belongs to is decided by where it lands
        // once mapped back out of the space the filter sits in — and
        // where that space is upright, which column a pixel is in
        // depends on x alone and which row on y alone. That makes the
        // block's average separable: two passes, one along each axis,
        // and the second averages the first's row means over the rows of
        // the block, which is the block. Turned, the blocks lie at an
        // angle on the page and neither pass can walk them, so that page
        // stays the CPU's.
        F::Pixelate { size } => {
            // Exactly upright, not nearly: the two renderers work the
            // block out from the same arithmetic, and a hair of shear
            // here would be a term the CPU adds and this does not —
            // which near a block's edge is a whole row in the wrong
            // square.
            let det = view.a * view.d - view.b * view.c;
            if view.b != 0.0 || view.c != 0.0 || det.abs() < 1e-9 {
                return None;
            }
            // The same inverse the CPU renderer builds, from the same
            // determinant, so the two agree to the last bit.
            let (ia, id) = (view.d / det, view.a / det);
            // A block smaller than a device pixel is not a block anyone
            // asked for, which is the floor the CPU renderer puts on it.
            let side = size.max(1.0 / scale.max(1e-6));
            // How far a pass has to walk. A block wider than this is
            // rare, and the walk would be long enough to be worth
            // handing back rather than growing a loop nobody can bound.
            const REACH: f32 = 128.0;
            if side / ia.abs() > REACH || side / id.abs() > REACH {
                return None;
            }
            Filtering::Blocks {
                across: [ia, view.e, side, 0.0],
                down: [id, view.f, side, 1.0],
            }
        }
        // A smear along a line. The box passes cannot walk it — they run
        // along an axis and this one runs at whatever angle it was given
        // — so it takes a pass of its own, with the taps worked out here
        // exactly as the CPU renderer works them out.
        F::MotionBlur { distance, degrees } => {
            let t = degrees.to_radians();
            let (dx, dy) = (t.cos() * distance, t.sin() * distance);
            let (ox, oy) = (view.a * dx + view.c * dy, view.b * dx + view.d * dy);
            let far = ox.hypot(oy);
            // A smear shorter than a pixel does not move one, and the CPU
            // renderer hands the page back untouched.
            if far < 0.5 {
                return Some(Filtering::Nothing);
            }
            // Odd, so the pixel itself is one of the taps.
            const MOST: usize = 257;
            let n = ((far.ceil() as usize).saturating_add(1)).min(MOST) | 1;
            Filtering::Smear {
                taps: n as f32,
                step: [ox / (n as f32 - 1.0), oy / (n as f32 - 1.0)],
            }
        }
    })
}

/// What a filter layer turns into here.
enum Filtering {
    /// A function of one pixel and of where it is: the quad says which
    /// filter and what it was asked for, and the adjustment machinery
    /// does the rest.
    Pointwise([f32; 4], [f32; 4]),
    /// Three box passes each way over what is under it, and how much of
    /// the difference to add back — zero for a plain blur, and an
    /// unsharp amount for a sharpen.
    Blur { radius: f32, sharpen: f32 },
    /// One pass over what is under it, averaging `taps` samples `step`
    /// apart along a line: a smear, which has an angle and so cannot be
    /// separated into a turn along each axis the way a blur is.
    Smear { taps: f32, step: [f32; 2] },
    /// Two passes, one along each axis: a pixelate, whose block average
    /// separates where the grid is upright on the page. Each carries the
    /// inverse scale along its axis, the origin it is measured from, the
    /// block's side, and which axis it is.
    Blocks { across: [f32; 4], down: [f32; 4] },
    /// A filter that was asked for nothing: a blur of no radius, a
    /// sharpen of no amount. The CPU renderer draws nothing for these,
    /// and neither does this.
    Nothing,
}

/// What an adjustment is stated by: the numbers the vertex carries, and
/// the table the fragment reads where there is more to say than seven
/// numbers can hold.
struct Adjusting {
    params: [f32; 4],
    grad: [f32; 4],
    extra: [f32; 3],
    table: Option<Image>,
}

/// Which arm of the shader's `blended` a mode is. The order is the
/// shader's; Normal is zero and never reaches it, since a layer that
/// composites normally is laid down by the plain image pipeline.
fn blend_index(mode: BlendMode) -> u32 {
    match mode {
        BlendMode::Normal => 0,
        BlendMode::Multiply => 1,
        BlendMode::Screen => 2,
        BlendMode::Overlay => 3,
        BlendMode::Darken => 4,
        BlendMode::Lighten => 5,
        BlendMode::ColorDodge => 6,
        BlendMode::ColorBurn => 7,
        BlendMode::HardLight => 8,
        BlendMode::SoftLight => 9,
        BlendMode::Difference => 10,
        BlendMode::Exclusion => 11,
        BlendMode::Hue => 12,
        BlendMode::Saturation => 13,
        BlendMode::Color => 14,
        BlendMode::Luminosity => 15,
    }
}

/// One pass: the surface it draws on, whether that surface starts bare,
/// a group's surface to lay down before anything else, and the run of
/// items to draw.
struct Pass {
    /// 0 is the page; anything else is the isolation surface for that
    /// depth of nesting.
    target: usize,
    clear: bool,
    lay: Option<Opening>,
    items: std::ops::Range<usize>,
}

/// What a pass does before its own items: something that reads the
/// surface it is drawing onto, which is why the pass had to start here
/// at all.
enum Opening {
    /// A finished group's surface, coming back down onto the one under
    /// it. Anything but a `Normal` blend reads what is under it, and a
    /// pass cannot sample what it is drawing into, so that is read from
    /// a copy taken first.
    Lay {
        from: usize,
        quad: std::ops::Range<u32>,
        mask: Option<usize>,
        blend: BlendMode,
    },
    /// An adjustment layer, which reads everything composited below it
    /// and writes the answer back over it — always from the copy — and,
    /// where it is stated by a table rather than by a handful of
    /// numbers, reads that where a picture's pixels would be.
    Adjust {
        quad: std::ops::Range<u32>,
        mask: Option<usize>,
        table: Option<usize>,
    },
    /// A blur layer. The six box passes have already run on the scratch
    /// pair by the time this opens the pass; what is left is the one
    /// quad that reads the blurred copy and the untouched one and mixes
    /// them by the layer's weight.
    Blur {
        steps: std::ops::Range<u32>,
        quad: std::ops::Range<u32>,
        mask: Option<usize>,
    },
    /// A motion blur. The one pass that averages along the line has run
    /// on the first scratch texture by the time this opens the pass;
    /// what is left is the quad that reads it and the untouched copy and
    /// mixes them by the layer's weight — which is a blur's quad, since
    /// there is nothing different to say once the smearing is done.
    Smear {
        along: std::ops::Range<u32>,
        quad: std::ops::Range<u32>,
        mask: Option<usize>,
    },
    /// A pixelate. Its two passes have run on the scratch pair by the
    /// time this opens the pass, ending on the second of them, so what
    /// is left is a blur's quad again.
    Blocks {
        steps: std::ops::Range<u32>,
        quad: std::ops::Range<u32>,
        mask: Option<usize>,
    },
}

impl Opening {
    /// Whether it wants the copy of what is already on the surface.
    fn reads_under(&self) -> bool {
        match self {
            Opening::Lay { blend, .. } => *blend != BlendMode::Normal,
            Opening::Adjust { .. }
            | Opening::Blur { .. }
            | Opening::Smear { .. }
            | Opening::Blocks { .. } => true,
        }
    }
}

/// Cut the items into passes at every `Open` and `Close`. A pass has one
/// set of attachments, so a group that composites as a unit — drawn on a
/// surface of its own and laid down afterwards — is three passes: what
/// came before it, the group itself, and what follows once it is down.
///
/// The surfaces are reused by depth. A group's is laid down the moment
/// its `Close` comes up and nothing reads it afterwards, so the next
/// group at that depth can have it back.
fn plan(draws: &[Item]) -> Vec<Pass> {
    let mut passes = Vec::new();
    let mut stack = vec![0usize];
    let (mut start, mut clear, mut lay) = (0usize, true, None);
    for (i, item) in draws.iter().enumerate() {
        // Only the four that change what the pass is drawing on, or
        // want to read it, cut it.
        match &item.draw {
            Draw::Open
            | Draw::Close { .. }
            | Draw::Adjust { .. }
            | Draw::Blur { .. }
            | Draw::Smear { .. }
            | Draw::Blocks { .. } => {}
            _ => continue,
        }
        passes.push(Pass {
            target: *stack.last().unwrap(),
            clear,
            lay: lay.take(),
            items: start..i,
        });
        match &item.draw {
            Draw::Open => {
                stack.push(stack.len());
                clear = true;
            }
            Draw::Close { quad, blend } => {
                let from = stack.pop().unwrap_or(0);
                clear = false;
                lay = Some(Opening::Lay {
                    from,
                    quad: quad.clone(),
                    mask: item.mask,
                    blend: *blend,
                });
            }
            Draw::Adjust { quad, table } => {
                clear = false;
                lay = Some(Opening::Adjust {
                    quad: quad.clone(),
                    mask: item.mask,
                    table: *table,
                });
            }
            Draw::Blur { steps, quad } => {
                clear = false;
                lay = Some(Opening::Blur {
                    steps: steps.clone(),
                    quad: quad.clone(),
                    mask: item.mask,
                });
            }
            Draw::Smear { along, quad } => {
                clear = false;
                lay = Some(Opening::Smear {
                    along: along.clone(),
                    quad: quad.clone(),
                    mask: item.mask,
                });
            }
            Draw::Blocks { steps, quad } => {
                clear = false;
                lay = Some(Opening::Blocks {
                    steps: steps.clone(),
                    quad: quad.clone(),
                    mask: item.mask,
                });
            }
            _ => unreachable!("only the six above cut a pass"),
        }
        start = i + 1;
    }
    passes.push(Pass {
        target: *stack.last().unwrap(),
        clear,
        lay,
        items: start..draws.len(),
    });
    passes
}

/// The six vertices of a quad over the whole page, reading a texture
/// that covers it: what lays an isolated group's surface back down.
/// `alpha` is the group's own opacity, which the image fragment reads
/// off the colour the way a placed picture's does.
fn page_quad(
    doc: &Document,
    alpha: f32,
    params: [f32; 4],
    grad: [f32; 4],
    extra: [f32; 3],
) -> Vec<Vertex> {
    let (w, h) = (doc.meta.width as f32, doc.meta.height as f32);
    let corner = |u: f32, v: f32| Vertex {
        doc: [u * w, v * h],
        local: [u, v],
        params,
        color: [extra[0], extra[1], extra[2], alpha],
        grad,
        mask: NO_MASK,
    };
    vec![
        corner(0.0, 0.0),
        corner(1.0, 0.0),
        corner(1.0, 1.0),
        corner(0.0, 0.0),
        corner(1.0, 1.0),
        corner(0.0, 1.0),
    ]
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
        mask: NO_MASK,
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
    use chitrakar_doc::{Command, Node, StrokeCap, StrokeJoin};

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

    /// Every command there is, asked of this backend one at a time.
    ///
    /// The CPU renderer is the reference and this one draws what it can,
    /// so the thing worth knowing is not what it declines — declining is
    /// always a safe answer — but whether anything it *accepts* comes
    /// out differently. One command at a time over a document with
    /// something of everything in it finds that where a test written per
    /// feature cannot: the disagreements live in the combinations, and
    /// the fixture is where every new `Command` has to be added anyway.
    #[test]
    fn whatever_the_gpu_agrees_to_draw_it_draws_the_way_the_cpu_does() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut f = chitrakar_doc::fixture::everything();
        // The fixture holds one of every node kind, and this backend
        // does not draw them all yet — the strokes a paint layer and a
        // clone layer hold are still the CPU's. One of those in
        // the document makes the whole page declined, and an audit that
        // is declined every time measures nothing, so they come out
        // first. As the backend learns a kind, its line here goes and
        // the commands that speak to it come into scope by themselves.
        for id in [f.painted, f.borrowed] {
            f.doc.apply(Command::RemoveNode { id }).unwrap();
        }
        // And every effect the fixture hangs on a layer, for the same
        // reason and in the same spirit: an effect is drawn from a
        // layer's silhouette in passes this backend has not learned, so
        // one anywhere in the document declines the page. Taken off by
        // walking the tree rather than by naming the layers that have
        // them, so the fixture can grow another without this going quiet.
        // The commands that put effects *on* things are still asked —
        // each is applied to a copy of the document and declined by name,
        // which is the audit working rather than the audit blind.
        let with_effects: Vec<NodeId> = f
            .doc
            .nodes()
            .filter(|(_, n)| !n.effects.is_empty())
            .map(|(id, _)| *id)
            .collect();
        assert!(
            !with_effects.is_empty(),
            "the fixture still hangs effects on layers"
        );
        for id in with_effects {
            f.doc
                .apply(Command::SetEffects {
                    id,
                    effects: Vec::new(),
                })
                .unwrap();
        }
        let mut drawn = 0usize;
        let mut declined = Vec::new();
        let check = |doc: &Document, what: &str, drawn: &mut usize| {
            if !GpuRenderer::can_render(doc) {
                return false;
            }
            let mine = gpu.render(doc).expect("can_render said it would");
            let reference = chitrakar_render::render(doc).unwrap();
            let (mean, worst) = difference(&mine, &reference);
            assert!(
                mean < 0.004,
                "after {what}: mean {mean:.5}, worst {worst:.3}"
            );
            *drawn += 1;
            true
        };
        check(&f.doc, "nothing at all", &mut drawn);
        for command in chitrakar_doc::fixture::every_command(&f) {
            let what = format!("{command:?}");
            let what = what
                .split_once(" {")
                .map_or(what.clone(), |(k, _)| k.into());
            let mut doc = f.doc.clone();
            if doc.apply(command).is_err() {
                continue;
            }
            if !check(&doc, &what, &mut drawn) {
                declined.push(what);
            }
        }
        // A backend that declined everything would pass the assertion
        // above without having drawn a thing.
        assert!(
            drawn > 15,
            "the backend drew {drawn} of them, which is too few to have              been asked anything (declined: {declined:?})"
        );
        eprintln!(
            "gpu drew {drawn}, declined {}: {declined:?}",
            declined.len()
        );
    }

    /// A layer held to the one under it, and a run of them.
    ///
    /// Clipping is not a mask cut from a shape: it is another layer's
    /// own alpha, which has to be that layer drawn aside. Two coverages
    /// a layer is held back by — its own mask and what it is clipped to
    /// — are one coverage by the time a fragment reads it.
    #[test]
    fn a_layer_held_to_the_one_under_it_stops_where_that_layer_does() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(60, 40, ColorMode::Rgb);
        let round = VectorShape::Ellipse { rx: 14.0, ry: 10.0 };
        add(
            &mut doc,
            filled("base", round.clone(), RED),
            // An ellipse's own box starts at its origin, so this one
            // spans 6..34 across and 10..30 down.
            Transform::translation(6.0, 10.0),
        );
        let wide = VectorShape::Rect {
            width: 60.0,
            height: 18.0,
            radius: 0.0,
        };
        for (name, at) in [("first", 4.0), ("second", 22.0)] {
            let id = add(
                &mut doc,
                filled(name, wide.clone(), BLUE),
                Transform::translation(0.0, at),
            );
            doc.apply(Command::SetClipped { id, clipped: true })
                .unwrap();
        }
        assert!(GpuRenderer::can_render(&doc), "a run of held layers");
        let (mean, worst) = difference(
            &gpu.render(&doc).unwrap(),
            &chitrakar_render::render(&doc).unwrap(),
        );
        assert!(mean < 0.004, "a run: mean {mean:.5}, worst {worst:.3}");

        // Outside the base there is nothing, however wide the bands
        // themselves are: a page that agreed everywhere by drawing
        // nothing would pass the reading above.
        let drawn = gpu.render(&doc).unwrap();
        assert!(drawn.get(2, 25).a < 0.01, "nothing beyond the base");
        assert!(drawn.get(20, 25).a > 0.99, "and the band inside it");

        // The layer's own mask as well: two coverages, one answer.
        let held = doc.children_of(doc.root()).unwrap()[1];
        doc.apply(Command::SetMask {
            id: held,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: 20.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    transform: Transform::translation(0.0, 0.0),
                },
                invert: false,
                feather: 0.0,
            })),
        })
        .unwrap();
        assert!(GpuRenderer::can_render(&doc), "held and masked at once");
        let (mean, worst) = difference(
            &gpu.render(&doc).unwrap(),
            &chitrakar_render::render(&doc).unwrap(),
        );
        assert!(
            mean < 0.004,
            "held and masked: mean {mean:.5}, worst {worst:.3}"
        );
    }

    /// A frame: a group with a size of its own, a ground painted inside
    /// it, and everything under it held to its rectangle.
    /// A copy of another layer, drawn where the copy is.
    #[test]
    fn a_copy_draws_what_it_is_a_copy_of() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(90, 60, ColorMode::Rgb);
        let round = VectorShape::Ellipse { rx: 10.0, ry: 8.0 };
        let master = add(
            &mut doc,
            filled("master", round.clone(), RED),
            Transform::translation(6.0, 20.0),
        );
        doc.apply(Command::AddNode {
            parent: doc.root(),
            index: 1,
            node: Box::new(Node::instance("copy", master)),
        })
        .unwrap();
        let copy = doc.children_of(doc.root()).unwrap()[1];
        doc.apply(Command::SetTransform {
            id: copy,
            transform: Transform::translation(50.0, 24.0),
        })
        .unwrap();
        assert!(GpuRenderer::can_render(&doc), "a plain copy");
        let drawn = gpu.render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &chitrakar_render::render(&doc).unwrap());
        assert!(mean < 0.004, "a copy: mean {mean:.5}, worst {worst:.3}");
        assert!(drawn.get(16, 28).a > 0.99, "the original is there");
        assert!(drawn.get(60, 32).a > 0.99, "and so is the copy");
        assert!(drawn.get(35, 30).a < 0.01, "with page between them");

        // Moving the original moves only the original: the copy draws
        // what it draws where the copy is, so the original's own
        // placement is undone first.
        let mut moved = doc.clone();
        moved
            .apply(Command::SetTransform {
                id: master,
                transform: Transform::translation(6.0, 40.0),
            })
            .unwrap();
        let after = gpu.render(&moved).unwrap();
        let (mean, worst) = difference(&after, &chitrakar_render::render(&moved).unwrap());
        assert!(mean < 0.004, "moved: mean {mean:.5}, worst {worst:.3}");
        assert!(after.get(60, 32).a > 0.99, "the copy stayed where it was");

        // A copy of a group draws that group's layers somewhere else
        // entirely, so a coverage — a mask, or the alpha a layer is
        // held to — has to be rasterized in the space the layer is
        // being drawn in rather than the one the document places it in.
        // Read off the original's box it would hold the copy back by
        // what is happening at the other end of the page.
        let mut inner = Document::new(90, 60, ColorMode::Rgb);
        let root = inner.root();
        inner
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::group("pair")),
            })
            .unwrap();
        let pair = inner.children_of(root).unwrap()[0];
        for (i, (name, shape, at)) in [
            ("base", round.clone(), 4.0),
            (
                "held",
                VectorShape::Rect {
                    width: 40.0,
                    height: 6.0,
                    radius: 0.0,
                },
                10.0,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            inner
                .apply(Command::AddNode {
                    parent: pair,
                    index: i,
                    node: filled(name, shape, if i == 0 { RED } else { BLUE }),
                })
                .unwrap();
            let id = inner.children_of(pair).unwrap()[i];
            inner
                .apply(Command::SetTransform {
                    id,
                    transform: Transform::translation(2.0, at),
                })
                .unwrap();
        }
        let held = inner.children_of(pair).unwrap()[1];
        inner
            .apply(Command::SetClipped {
                id: held,
                clipped: true,
            })
            .unwrap();
        inner
            .apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::instance("a copy", pair)),
            })
            .unwrap();
        let elsewhere = inner.children_of(root).unwrap()[1];
        inner
            .apply(Command::SetTransform {
                id: elsewhere,
                transform: Transform::translation(48.0, 26.0),
            })
            .unwrap();
        assert!(GpuRenderer::can_render(&inner), "a copy of a held pair");
        let (mean, worst) = difference(
            &gpu.render(&inner).unwrap(),
            &chitrakar_render::render(&inner).unwrap(),
        );
        assert!(
            mean < 0.004,
            "held inside a copy: mean {mean:.5}, worst {worst:.3}"
        );

        // Faded, blended or masked, the CPU draws a copy on a surface of
        // its own, which is a different picture from this.
        let mut faded = doc.clone();
        faded
            .apply(Command::SetOpacity {
                id: copy,
                opacity: 0.5,
            })
            .unwrap();
        assert!(!GpuRenderer::can_render(&faded), "a copy composited whole");
    }

    #[test]
    fn a_frame_paints_its_ground_and_keeps_its_contents_inside() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(80, 60, ColorMode::Rgb);
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::artboard("frame", 30.0, 20.0, Some(WHITE))),
        })
        .unwrap();
        let frame = doc.children_of(root).unwrap()[0];
        doc.apply(Command::SetTransform {
            id: frame,
            transform: Transform::translation(20.0, 20.0),
        })
        .unwrap();
        // A shape far bigger than the frame, so being held to it is the
        // whole of what the picture shows.
        doc.apply(Command::AddNode {
            parent: frame,
            index: 0,
            node: filled(
                "spill",
                VectorShape::Rect {
                    width: 200.0,
                    height: 200.0,
                    radius: 0.0,
                },
                RED,
            ),
        })
        .unwrap();
        assert!(GpuRenderer::can_render(&doc), "an upright frame");
        let drawn = gpu.render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &chitrakar_render::render(&doc).unwrap());
        assert!(mean < 0.004, "a frame: mean {mean:.5}, worst {worst:.3}");
        assert!(drawn.get(30, 30).a > 0.99, "the shape shows inside it");
        assert!(drawn.get(10, 30).a < 0.01, "and stops at the frame's edge");
        assert!(drawn.get(55, 30).a < 0.01, "on both sides of it");

        // The ground alone, with nothing in the frame, is the frame's
        // own rectangle — and it is held inside the page like anything
        // else, so a frame hanging off the edge shows the part that is on
        // it.
        let mut bare = Document::new(80, 60, ColorMode::Rgb);
        let root = bare.root();
        bare.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::artboard("frame", 30.0, 20.0, Some(WHITE))),
        })
        .unwrap();
        let hanging = bare.children_of(root).unwrap()[0];
        bare.apply(Command::SetTransform {
            id: hanging,
            transform: Transform::translation(65.0, 20.0),
        })
        .unwrap();
        assert!(GpuRenderer::can_render(&bare));
        let (mean, worst) = difference(
            &gpu.render(&bare).unwrap(),
            &chitrakar_render::render(&bare).unwrap(),
        );
        assert!(
            mean < 0.004,
            "hanging off: mean {mean:.5}, worst {worst:.3}"
        );

        // Turned, or composited as a whole, the CPU draws a frame on a
        // surface of its own — a different picture from this one — so
        // those go back.
        let mut faded = doc.clone();
        faded
            .apply(Command::SetOpacity {
                id: frame,
                opacity: 0.5,
            })
            .unwrap();
        assert!(!GpuRenderer::can_render(&faded), "a frame composited whole");
        let mut turned = doc.clone();
        turned.apply(Command::TurnCanvas { quarters: 1 }).unwrap();
        assert!(!GpuRenderer::can_render(&turned), "a turned frame");
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

        // A group at less than full opacity composites as a unit: the
        // two rects inside it meet each other at full strength and the
        // result is taken down together, so where they overlap the page
        // shows half of the blue over the red rather than half of each.
        // That is a surface of its own, and the surface is what lands.
        doc.apply(Command::SetOpacity {
            id: group,
            opacity: 0.5,
        })
        .unwrap();
        assert!(
            GpuRenderer::can_render(&doc),
            "a group can composite as a unit"
        );
        let faint = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&faint, &reference);
        assert!(
            mean < 0.004,
            "half-opaque group: mean {mean:.5}, worst {worst:.3}"
        );
        // Half of what the group came to, not half of nothing: there is
        // ink, and it is half-strength.
        let inside = faint.get(15, 15);
        assert!(
            (inside.a - 0.5).abs() < 0.02 && inside.r > 0.2,
            "the group came down at half strength: {inside:?}"
        );
        assert!(
            (inside.a - reference.get(15, 15).a).abs() < 0.02,
            "which is what the CPU makes of it too"
        );
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
        collect(
            &doc,
            doc.root(),
            Transform::default(),
            1.0,
            None,
            &mut scene,
        )
        .unwrap();
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
                color: color.clone(),
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

    fn stroked(name: &str, shape: VectorShape, width: f32, widths: Vec<f32>) -> Box<Node> {
        let mut node = Node::vector(name, shape);
        if let NodeKind::Vector { fill, stroke, .. } = &mut node.kind {
            *fill = None;
            *stroke = Some(chitrakar_doc::Stroke {
                color: BLUE,
                width,
                widths,
                dash: Vec::new(),
                cap: Default::default(),
                join: Default::default(),
                align: None,
                start_marker: Default::default(),
                end_marker: Default::default(),
            });
        }
        Box::new(node)
    }

    /// A stroke is an inner band on a rect or an ellipse — so stroking
    /// one never grows its bounds — and on a path it is the union of
    /// round-capped segments, joins and caps included, laid down as
    /// geometry rather than tested per sample.
    #[test]
    fn strokes_cover_what_the_cpu_strokes() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(140, 90, ColorMode::Rgb);
        add(
            &mut doc,
            stroked(
                "round rect",
                VectorShape::Rect {
                    width: 34.0,
                    height: 26.0,
                    radius: 7.0,
                },
                4.0,
                Vec::new(),
            ),
            Transform::translation(8.0, 8.0),
        );
        // A band lying outside its edge, which the fragment reads as a
        // shape grown rather than a shape shrunk.
        let mut ring = stroked(
            "ellipse",
            VectorShape::Ellipse { rx: 18.0, ry: 12.0 },
            5.0,
            Vec::new(),
        );
        if let NodeKind::Vector {
            stroke: Some(s), ..
        } = &mut ring.kind
        {
            s.align = Some(chitrakar_doc::StrokeAlign::Outside);
        }
        add(&mut doc, ring, Transform::translation(56.0, 14.0));
        // An open path: three segments, so two round joins and two caps.
        add(
            &mut doc,
            stroked(
                "line",
                VectorShape::Path {
                    points: vec![[0.0, 20.0], [16.0, 0.0], [32.0, 22.0], [48.0, 2.0]],
                    closed: false,
                    smooth: false,
                    handles: Vec::new(),
                    subpaths: Vec::new(),
                },
                6.0,
                Vec::new(),
            ),
            Transform::translation(10.0, 52.0),
        );
        // And one that swells and tapers, the way a pressure stroke does.
        add(
            &mut doc,
            stroked(
                "taper",
                VectorShape::Path {
                    points: vec![[0.0, 0.0], [18.0, 10.0], [36.0, 0.0], [54.0, 12.0]],
                    closed: false,
                    smooth: false,
                    handles: Vec::new(),
                    subpaths: Vec::new(),
                },
                9.0,
                vec![0.15, 1.0, 0.6, 0.2],
            ),
            Transform::translation(74.0, 56.0),
        );

        assert!(GpuRenderer::can_render(&doc));
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        // A stroke is nearly all edge, and a stencilled edge is as fine
        // as the sampling rather than exact, so a pixel of it can be a
        // quarter out — over the page that comes to well under this.
        assert!(
            mean < 0.004,
            "mean channel difference {mean:.5} (worst {worst:.3})"
        );
        // The band is inside the shape: its middle is hollow and its
        // outside is bare, on both.
        for (x, y, what) in [(25, 21, "the rect's middle"), (74, 26, "the ellipse's")] {
            assert_eq!(drawn.get(x, y).a, 0.0, "{what} is hollow");
            assert_eq!(reference.get(x, y).a, 0.0, "{what} is hollow on the CPU");
        }
        assert_eq!(drawn.get(4, 4).a, 0.0, "and nothing outside it");
        // The rim itself is painted, and to the reference's own weight.
        for (x, y) in [(9, 21), (25, 9), (54, 26), (18, 62), (101, 61)] {
            let (g, c) = (drawn.get(x, y).a, reference.get(x, y).a);
            assert!(g > 0.5, "the band at ({x}, {y}) is painted: {g}");
            assert!((g - c).abs() < 0.2, "at ({x}, {y}): {g} vs {c}");
        }
        // A round cap reaches past the last anchor by half the width,
        // and does it on both.
        for (x, y) in [(59, 52), (9, 73)] {
            let (g, c) = (drawn.get(x, y).a, reference.get(x, y).a);
            assert!((g - c).abs() < 0.25, "the cap at ({x}, {y}): {g} vs {c}");
        }
        // The tapering one really tapers: thin at the start, fat in the
        // middle, and the same thickness the CPU draws.
        let thickness = |s: &Surface, x: u32| (50..80).filter(|y| s.get(x, *y).a > 0.5).count();
        for x in [76, 92, 110] {
            let (g, c) = (thickness(&drawn, x), thickness(&reference, x));
            assert!(
                g.abs_diff(c) <= 2,
                "column {x} is {g} thick, the CPU's is {c}"
            );
        }
        assert!(
            thickness(&drawn, 92) > thickness(&drawn, 76),
            "it swells from its thin start"
        );
    }

    /// How a line ends and turns is stated once — as the pieces the CPU
    /// tests a sample against — so the GPU lays down the same ends and
    /// the same corners rather than an idea of its own.
    #[test]
    fn ends_and_corners_are_the_same_shape_on_both() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(160, 60, ColorMode::Rgb);
        // The same elbow three times, ending and turning three ways.
        let ways = [
            (StrokeCap::Butt, StrokeJoin::Miter),
            (StrokeCap::Square, StrokeJoin::Bevel),
            (StrokeCap::Round, StrokeJoin::Round),
        ];
        for (i, (cap, join)) in ways.into_iter().enumerate() {
            let mut node = stroked(
                "elbow",
                VectorShape::Path {
                    points: vec![[0.0, 0.0], [24.0, 0.0], [24.0, 30.0]],
                    closed: false,
                    smooth: false,
                    handles: Vec::new(),
                    subpaths: Vec::new(),
                },
                8.0,
                Vec::new(),
            );
            if let NodeKind::Vector {
                stroke: Some(s), ..
            } = &mut node.kind
            {
                s.cap = cap;
                s.join = join;
                // A head at the far end of each: what a line carries is
                // stated as pieces of its own region, so the GPU draws
                // one without being told about markers at all.
                s.start_marker = chitrakar_doc::Marker::Arrow;
            }
            add(
                &mut doc,
                node,
                Transform::translation(12.0 + i as f32 * 48.0, 12.0),
            );
        }
        assert!(GpuRenderer::can_render(&doc));
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        assert!(
            mean < 0.004,
            "mean channel difference {mean:.5} (worst {worst:.3})"
        );
        // Out past the corner, where only a miter reaches; and out past
        // the last point off to one side, where only a square end has
        // anything. The three have to differ here — otherwise the two
        // renderers could agree by both drawing one shape three times.
        for (y, want, what) in [
            (8, [true, false, false], "corner"),
            (45, [false, true, false], "end"),
        ] {
            for (i, on) in want.into_iter().enumerate() {
                let x = 39 + i as u32 * 48;
                let (g, c) = (drawn.get(x, y).a, reference.get(x, y).a);
                assert_eq!(
                    c > 0.5,
                    on,
                    "the CPU's {what} at ({x}, {y}) is {c}, wanted {on}"
                );
                assert!((g - c).abs() < 0.25, "the {what} at ({x}, {y}): {g} vs {c}");
            }
        }
    }

    fn texted(
        name: &str,
        text: &str,
        size: f32,
        tweak: impl FnOnce(&mut chitrakar_doc::TextSpec),
    ) -> Box<Node> {
        let mut spec = chitrakar_doc::TextSpec::new(text, size, BLUE);
        tweak(&mut spec);
        Box::new(Node::text(name, spec))
    }

    /// Text is one bitmap either way: the renderer that decides how
    /// finely to rasterize a block hands the same one to both, and the
    /// GPU reads it the way the CPU reads it — bilinearly, fading off
    /// the edge rather than smearing it.
    #[test]
    fn text_reads_the_same_raster_the_cpu_reads() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(200, 100, ColorMode::Rgb);
        add(
            &mut doc,
            texted("plain", "Chitrakar", 26.0, |_| {}),
            Transform::translation(8.0, 10.0),
        );
        // Leaning, underlined and struck through, and turned: the raster
        // carries all of that, and the quad carries the transform.
        add(
            &mut doc,
            texted("fancy", "vector", 20.0, |spec| {
                spec.italic = true;
                spec.underline = true;
                spec.strike = true;
            }),
            Transform {
                a: 1.2 * 0.97,
                b: 1.2 * 0.24,
                c: -1.2 * 0.24,
                d: 1.2 * 0.97,
                e: 14.0,
                f: 52.0,
            },
        );
        assert!(GpuRenderer::can_render(&doc));
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        // Far tighter than the shapes, because there is nothing to
        // approximate: both read the same bitmap, and what is left is
        // the coverage rounded to half-precision.
        assert!(
            mean < 0.0005,
            "mean channel difference {mean:.5} (worst {worst:.3})"
        );
        // There is ink, and it is where the reference has ink: over the
        // rows the words sit on, the two agree pixel by pixel.
        let inked = |s: &Surface| s.pixels.iter().filter(|p| p.a > 0.5).count();
        assert!(
            inked(&drawn) > 200,
            "the words are drawn: {}",
            inked(&drawn)
        );
        assert!(
            (inked(&drawn) as i64 - inked(&reference) as i64).abs() < inked(&reference) as i64 / 20,
            "{} inked pixels against the reference's {}",
            inked(&drawn),
            inked(&reference)
        );
        for y in [20, 30, 60, 70] {
            for x in (4..196).step_by(7) {
                let (g, c) = (drawn.get(x, y).a, reference.get(x, y).a);
                assert!((g - c).abs() < 0.15, "at ({x}, {y}): {g} vs {c}");
            }
        }
        assert_eq!(drawn.get(196, 96).a, 0.0, "bare page stays bare");
    }

    /// The device is asked for the textures every adapter guarantees,
    /// so a page that would need a bigger one is handed back rather
    /// than overrunning that.
    #[test]
    fn a_page_bigger_than_the_textures_it_asked_for_goes_back() {
        let mut doc = Document::new(MAX_TEXTURE + 1, 100, ColorMode::Rgb);
        add(
            &mut doc,
            filled(
                "r",
                VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 0.0,
                },
                RED,
            ),
            Transform::default(),
        );
        assert!(!GpuRenderer::can_render(&doc));

        // And so is a placed image too big for one.
        let mut wide = Document::new(60, 60, ColorMode::Rgb);
        let id = wide.add_resource(
            MAX_TEXTURE + 1,
            1,
            vec![255; (MAX_TEXTURE as usize + 1) * 4],
        );
        let root = wide.root();
        wide.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: Box::new(Node::raster(
                "img",
                chitrakar_doc::RasterRef {
                    resource_id: id,
                    width: MAX_TEXTURE + 1,
                    height: 1,
                },
            )),
        })
        .unwrap();
        assert!(!GpuRenderer::can_render(&wide));
    }

    /// A mask is a coverage the CPU renderer works out and the fragment
    /// multiplies by, so a masked layer comes out the same both ways —
    /// a shape held to a circle, an ellipse held to a rectangle it is
    /// only half inside, and an inverted mask, which is the hole rather
    /// than the piece.
    #[test]
    fn masks_hold_a_layer_to_the_same_shape_on_both() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(120, 60, ColorMode::Rgb);
        let square = VectorShape::Rect {
            width: 40.0,
            height: 40.0,
            radius: 0.0,
        };
        let held = add(
            &mut doc,
            filled("held", square.clone(), RED),
            Transform::translation(10.0, 10.0),
        );
        doc.apply(Command::SetMask {
            id: held,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Ellipse { rx: 15.0, ry: 15.0 },
                    transform: Transform::translation(25.0, 25.0),
                },
                invert: false,
                feather: 0.0,
            })),
        })
        .unwrap();

        // The other one keeps only what falls outside its mask, and its
        // mask hangs off the layer, so the box the coverage is worked
        // out over matters.
        let hole = add(
            &mut doc,
            filled("hole", VectorShape::Ellipse { rx: 20.0, ry: 20.0 }, BLUE),
            Transform::translation(70.0, 10.0),
        );
        doc.apply(Command::SetMask {
            id: hole,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: 30.0,
                        height: 30.0,
                        radius: 0.0,
                    },
                    transform: Transform::translation(75.0, 15.0),
                },
                invert: true,
                feather: 0.0,
            })),
        })
        .unwrap();

        assert!(GpuRenderer::can_render(&doc), "a masked leaf is drawn");
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        assert!(
            mean < 0.004,
            "mean channel difference {mean:.5} (worst {worst:.3})"
        );
        // The masks did something: the middle of the circle is painted,
        // the corner of the square outside it is not, and the inverted
        // one is the other way round.
        assert!(drawn.get(30, 30).a > 0.9, "inside the mask is painted");
        assert!(drawn.get(12, 12).a < 0.1, "outside it is not");
        assert!(drawn.get(90, 30).a < 0.1, "inside an inverted mask is bare");
        assert!(
            drawn.get(72, 30).a > 0.9,
            "and outside one is painted: {:?}",
            drawn.get(72, 30)
        );
        // Pixel for pixel with the reference wherever the reference is
        // decisive: every pixel the CPU paints solidly is painted, and
        // every one it leaves bare is bare. The soft pixels along an
        // edge are where the two renderers legitimately differ — the
        // GPU's edge is analytic and the CPU's is sampled — and the
        // mean above is what holds those.
        // Two pixels clear of every edge on all sides: an analytic edge
        // and a sampled one legitimately disagree within a pixel of each
        // other, and the mean above is what holds those.
        let all = |s: &Surface, x: u32, y: u32, want: fn(f32) -> bool| {
            (y - 2..=y + 2).all(|b| (x - 2..=x + 2).all(|a| want(s.get(a, b).a)))
        };
        let mut checked = 0;
        for y in 2..58 {
            for x in 2..118 {
                let (g, c) = (drawn.get(x, y).a, reference.get(x, y).a);
                let inside = all(&reference, x, y, |a| a > 0.99);
                let outside = all(&reference, x, y, |a| a < 0.01);
                if !(inside || outside) {
                    continue;
                }
                checked += 1;
                assert!(
                    (g - c).abs() < 0.02,
                    "alpha at {x},{y}: gpu {g:.3} against cpu {c:.3}"
                );
            }
        }
        assert!(checked > 5000, "most of the page is decisive: {checked}");

        // A mask brushed on by hand rather than cut from a shape. Its
        // coverage is worked out over a box rather than read at a point,
        // and the box the GPU asks for is the layer's rather than the
        // whole page, so it is worth proving the two agree.
        let mut painted = Document::new(60, 60, ColorMode::Rgb);
        let under = add(
            &mut painted,
            filled(
                "under",
                VectorShape::Rect {
                    width: 40.0,
                    height: 40.0,
                    radius: 0.0,
                },
                RED,
            ),
            Transform::translation(10.0, 10.0),
        );
        painted
            .apply(Command::SetMask {
                id: under,
                mask: Some(Box::new(chitrakar_doc::Mask {
                    kind: chitrakar_doc::MaskKind::Painted {
                        strokes: vec![chitrakar_doc::PaintStroke {
                            points: vec![[15.0, 30.0], [45.0, 30.0]],
                            radii: vec![8.0],
                            color: RED,
                            softness: 0.0,
                            erase: true,
                            source: [0.0; 2],
                            heal: false,
                            clip: None,
                        }],
                    },
                    invert: false,
                    feather: 0.0,
                })),
            })
            .unwrap();
        assert!(GpuRenderer::can_render(&painted));
        let brushed = gpu.render(&painted).unwrap();
        let by_cpu = chitrakar_render::render(&painted).unwrap();
        let (mean, worst) = difference(&brushed, &by_cpu);
        assert!(
            mean < 0.004,
            "a painted mask: mean {mean:.5} (worst {worst:.3})"
        );
        assert!(
            brushed.get(30, 30).a < 0.02 && brushed.get(30, 14).a > 0.98,
            "the stroke took a band out of it: {:?} against {:?}",
            brushed.get(30, 30),
            brushed.get(30, 14)
        );

        // A mask inside a group is authored in the group's space, not
        // the page's, so the coverage has to be worked out through the
        // group's transform. A layer at the top level would not notice
        // the difference; one inside a moved group would.
        let mut nested = Document::new(60, 60, ColorMode::Rgb);
        let root = nested.root();
        nested
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::group("moved")),
            })
            .unwrap();
        let moved = nested.children_of(root).unwrap()[0];
        nested
            .apply(Command::SetTransform {
                id: moved,
                transform: Transform::translation(12.0, 8.0),
            })
            .unwrap();
        nested
            .apply(Command::AddNode {
                parent: moved,
                index: 0,
                node: filled(
                    "inside",
                    VectorShape::Rect {
                        width: 30.0,
                        height: 30.0,
                        radius: 0.0,
                    },
                    RED,
                ),
            })
            .unwrap();
        let inside = nested.children_of(moved).unwrap()[0];
        nested
            .apply(Command::SetTransform {
                id: inside,
                transform: Transform::translation(5.0, 5.0),
            })
            .unwrap();
        nested
            .apply(Command::SetMask {
                id: inside,
                mask: Some(Box::new(chitrakar_doc::Mask {
                    kind: chitrakar_doc::MaskKind::Vector {
                        shape: VectorShape::Rect {
                            width: 15.0,
                            height: 30.0,
                            radius: 0.0,
                        },
                        transform: Transform::translation(5.0, 5.0),
                    },
                    invert: false,
                    feather: 0.0,
                })),
            })
            .unwrap();
        assert!(GpuRenderer::can_render(&nested));
        let deep = gpu.render(&nested).unwrap();
        let flat = chitrakar_render::render(&nested).unwrap();
        let (mean, worst) = difference(&deep, &flat);
        assert!(
            mean < 0.004,
            "a mask inside a group: mean {mean:.5} (worst {worst:.3})"
        );
        // The mask covers the left half of the layer, and both of them
        // moved with the group: ink at 20,20 and none at 40,20.
        assert!(
            deep.get(20, 20).a > 0.98 && deep.get(40, 20).a < 0.02,
            "and it moved with the group: {:?} against {:?}",
            deep.get(20, 20),
            deep.get(40, 20)
        );

        // A mask on a group is a different thing — it holds what the
        // group composites to, not each child on its own — so the group
        // goes on a surface of its own and the mask holds the one quad
        // that lays that surface down.
        let mut grouped = Document::new(60, 60, ColorMode::Rgb);
        let root = grouped.root();
        grouped
            .apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::group("g")),
            })
            .unwrap();
        let group = grouped.children_of(root).unwrap()[0];
        grouped
            .apply(Command::AddNode {
                parent: group,
                index: 0,
                node: filled("in", square, RED),
            })
            .unwrap();
        assert!(GpuRenderer::can_render(&grouped));
        grouped
            .apply(Command::SetMask {
                id: group,
                mask: Some(Box::new(chitrakar_doc::Mask {
                    kind: chitrakar_doc::MaskKind::Vector {
                        shape: VectorShape::Ellipse { rx: 15.0, ry: 15.0 },
                        transform: Transform::translation(20.0, 20.0),
                    },
                    invert: false,
                    feather: 0.0,
                })),
            })
            .unwrap();
        assert!(
            GpuRenderer::can_render(&grouped),
            "a masked group composites as a unit"
        );
        let held = gpu.render(&grouped).unwrap();
        let by_cpu = chitrakar_render::render(&grouped).unwrap();
        let (mean, worst) = difference(&held, &by_cpu);
        assert!(
            mean < 0.004,
            "a masked group: mean {mean:.5} (worst {worst:.3})"
        );
        // The mask is a circle about (35,35); the square under it
        // reaches to 40. Where the two agree there is ink, and in the
        // corner of the square that the circle does not reach there is
        // none.
        assert!(
            held.get(35, 35).a > 0.98 && held.get(5, 5).a < 0.02,
            "inside the mask is painted and outside it is not: {:?} against {:?}",
            held.get(35, 35),
            held.get(5, 5)
        );

        // Two children overlapping inside a masked group: the mask has
        // to hold what they composite to, not each of them, or the
        // coverage would be taken twice where they meet — which shows
        // as a darker seam under a soft edge. Both renderers say the
        // same thing about that seam.
        grouped
            .apply(Command::AddNode {
                parent: group,
                index: 1,
                node: filled("over", VectorShape::Ellipse { rx: 12.0, ry: 12.0 }, BLUE),
            })
            .unwrap();
        let both = grouped.children_of(group).unwrap()[1];
        grouped
            .apply(Command::SetOpacity {
                id: both,
                opacity: 0.5,
            })
            .unwrap();
        let overlapped = gpu.render(&grouped).unwrap();
        let (mean, worst) = difference(&overlapped, &chitrakar_render::render(&grouped).unwrap());
        assert!(
            mean < 0.004,
            "overlapping under one mask: mean {mean:.5} (worst {worst:.3})"
        );
    }

    /// The mask reaches every kind of draw, not only the shape whose
    /// fragment finds its own coverage: a stencilled path, a stroke, a
    /// text block and a placed image are all held to it too.
    #[test]
    fn every_kind_of_layer_is_held_to_its_mask() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let half = |id: NodeId, doc: &mut Document, x: f32| {
            // The left half of a 40-wide slot, so every layer keeps the
            // half of itself the mask covers and loses the other.
            doc.apply(Command::SetMask {
                id,
                mask: Some(Box::new(chitrakar_doc::Mask {
                    kind: chitrakar_doc::MaskKind::Vector {
                        shape: VectorShape::Rect {
                            width: 20.0,
                            height: 60.0,
                            radius: 0.0,
                        },
                        transform: Transform::translation(x, 0.0),
                    },
                    invert: false,
                    feather: 0.0,
                })),
            })
            .unwrap();
        };

        let mut doc = Document::new(160, 60, ColorMode::Rgb);
        let path = add(
            &mut doc,
            filled(
                "path",
                VectorShape::Path {
                    points: vec![[0.0, 0.0], [36.0, 0.0], [36.0, 36.0], [0.0, 36.0]],
                    closed: true,
                    smooth: false,
                    handles: Vec::new(),
                    subpaths: Vec::new(),
                },
                RED,
            ),
            Transform::translation(2.0, 12.0),
        );
        half(path, &mut doc, 2.0);

        let mut stroked = Node::vector(
            "stroke",
            VectorShape::Path {
                points: vec![[0.0, 0.0], [36.0, 0.0], [36.0, 36.0]],
                closed: false,
                smooth: false,
                handles: Vec::new(),
                subpaths: Vec::new(),
            },
        );
        if let NodeKind::Vector { fill, stroke, .. } = &mut stroked.kind {
            *fill = None;
            *stroke = Some(chitrakar_doc::Stroke {
                color: BLUE,
                width: 6.0,
                widths: Vec::new(),
                dash: Vec::new(),
                cap: Default::default(),
                join: Default::default(),
                align: None,
                start_marker: Default::default(),
                end_marker: Default::default(),
            });
        }
        let stroke = add(
            &mut doc,
            Box::new(stroked),
            Transform::translation(42.0, 12.0),
        );
        half(stroke, &mut doc, 42.0);

        let words = add(
            &mut doc,
            texted("words", "mask", 26.0, |_| {}),
            Transform::translation(82.0, 16.0),
        );
        half(words, &mut doc, 82.0);

        assert!(GpuRenderer::can_render(&doc), "each kind is drawn");
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        assert!(
            mean < 0.004,
            "mean channel difference {mean:.5} (worst {worst:.3})"
        );
        // Each layer kept its left half and lost its right: there is ink
        // in the first twenty columns of each slot and none in the next.
        for (name, x) in [("path", 2.0f32), ("stroke", 42.0), ("words", 82.0)] {
            let ink = |from: u32, to: u32| {
                (0..60)
                    .flat_map(|y| (from..to).map(move |x| (x, y)))
                    .filter(|(x, y)| drawn.get(*x, *y).a > 0.5)
                    .count()
            };
            let (kept, lost) = (
                ink(x as u32, x as u32 + 20),
                ink(x as u32 + 20, x as u32 + 38),
            );
            assert!(
                kept > 40,
                "the {name} keeps the half its mask covers: {kept}"
            );
            assert!(lost == 0, "and loses the half it does not: {lost}");
        }
    }

    /// Groups that composite as a unit, nested and side by side: the
    /// surfaces they are drawn on stack, and two at the same depth take
    /// turns on the same one, since a group's surface is laid down the
    /// moment it is finished with.
    #[test]
    fn surfaces_stack_and_are_taken_in_turn() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(120, 60, ColorMode::Rgb);
        let root = doc.root();
        // A half-opaque group holding a half-opaque group, beside two
        // half-opaque groups that are siblings.
        let group = |doc: &mut Document, parent: NodeId, name: &str, at: f32| {
            let index = doc.children_of(parent).unwrap().len();
            doc.apply(Command::AddNode {
                parent,
                index,
                node: Box::new(Node::group(name)),
            })
            .unwrap();
            let id = doc.children_of(parent).unwrap()[index];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(at, 0.0),
            })
            .unwrap();
            doc.apply(Command::SetOpacity { id, opacity: 0.5 }).unwrap();
            id
        };
        let box_ = |doc: &mut Document, parent: NodeId, name: &str, color, at: f32| {
            let index = doc.children_of(parent).unwrap().len();
            doc.apply(Command::AddNode {
                parent,
                index,
                node: filled(
                    name,
                    VectorShape::Rect {
                        width: 24.0,
                        height: 24.0,
                        radius: 0.0,
                    },
                    color,
                ),
            })
            .unwrap();
            let id = doc.children_of(parent).unwrap()[index];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(at, 10.0),
            })
            .unwrap();
        };
        let outer = group(&mut doc, root, "outer", 4.0);
        box_(&mut doc, outer, "a", RED, 0.0);
        let inner = group(&mut doc, outer, "inner", 16.0);
        box_(&mut doc, inner, "b", BLUE, 0.0);

        let one = group(&mut doc, root, "one", 64.0);
        box_(&mut doc, one, "c", RED, 0.0);
        let two = group(&mut doc, root, "two", 88.0);
        box_(&mut doc, two, "d", BLUE, 0.0);

        assert!(GpuRenderer::can_render(&doc));
        let drawn = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&drawn, &reference);
        assert!(
            mean < 0.004,
            "nested and side by side: mean {mean:.5} (worst {worst:.3})"
        );
        // Each of the four boxes is there, and the one inside two
        // half-opaque groups is the faintest of them.
        let deep = drawn.get(28, 20).a;
        let shallow = drawn.get(10, 20).a;
        assert!(
            (shallow - 0.5).abs() < 0.03,
            "one group deep is half strength: {shallow}"
        );
        assert!(
            (deep - 0.25).abs() < 0.03,
            "two groups deep is a quarter: {deep}"
        );
        assert!(
            (drawn.get(70, 20).a - 0.5).abs() < 0.03 && (drawn.get(94, 20).a - 0.5).abs() < 0.03,
            "and the two beside them took the same surface in turn"
        );
    }

    /// All sixteen blend modes, each over the same backdrop, against
    /// what the CPU makes of them. A blended layer is drawn on a surface
    /// of its own and brought down by a fragment that works out the
    /// whole answer — the backdrop read from a copy, since a pass cannot
    /// sample what it is drawing into.
    #[test]
    fn every_blend_mode_meets_the_page_the_way_the_cpu_does() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let modes = [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ];
        for mode in modes {
            let mut doc = Document::new(60, 40, ColorMode::Rgb);
            // A backdrop of two tones, so a mode that reads the backdrop
            // has something to read, and a source over both of them.
            add(
                &mut doc,
                filled(
                    "dark",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.2,
                        g: 0.35,
                        b: 0.6,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            add(
                &mut doc,
                filled(
                    "light",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.85,
                        g: 0.7,
                        b: 0.3,
                        a: 1.0,
                    },
                ),
                Transform::translation(0.0, 20.0),
            );
            let over = add(
                &mut doc,
                filled(
                    "over",
                    VectorShape::Rect {
                        width: 40.0,
                        height: 30.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.9,
                        g: 0.25,
                        b: 0.45,
                        a: 1.0,
                    },
                ),
                Transform::translation(10.0, 5.0),
            );
            doc.apply(Command::SetBlendMode {
                id: over,
                blend: mode,
            })
            .unwrap();
            assert!(
                GpuRenderer::can_render(&doc),
                "{mode:?} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.004,
                "{mode:?}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
            // And the blend did something: over the dark half and over
            // the light half the layer comes out as two different
            // colours, which is what having a backdrop means. (Two modes
            // take only the source's own colour there, so they are
            // allowed to agree with themselves.)
            let (a, b) = (drawn.get(20, 12), drawn.get(20, 28));
            let apart = (a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs();
            assert!(
                apart > 0.02,
                "{mode:?} reads the backdrop: {a:?} against {b:?}"
            );
            // Off the layer, the page is untouched: a blend replaces
            // what is there with the answer, and the answer where there
            // is no source is what was there.
            let bare = drawn.get(55, 12);
            let cpu = reference.get(55, 12);
            assert!(
                (bare.r - cpu.r).abs() < 0.01 && (bare.a - cpu.a).abs() < 0.01,
                "{mode:?} leaves the rest of the page alone: {bare:?} against {cpu:?}"
            );
        }
    }

    /// A blend on a group, and a blended layer that carries a mask:
    /// the group's contents meet each other first and the result is what
    /// blends, and the mask holds the quad that brings it down.
    #[test]
    fn a_blend_can_sit_on_a_group_or_carry_a_mask() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(80, 60, ColorMode::Rgb);
        add(
            &mut doc,
            filled(
                "page",
                VectorShape::Rect {
                    width: 80.0,
                    height: 60.0,
                    radius: 0.0,
                },
                AuthoredColor::Srgb {
                    r: 0.3,
                    g: 0.5,
                    b: 0.7,
                    a: 1.0,
                },
            ),
            Transform::default(),
        );
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::group("pair")),
        })
        .unwrap();
        let pair = doc.children_of(root).unwrap()[1];
        for (i, at) in [4.0f32, 20.0].iter().enumerate() {
            doc.apply(Command::AddNode {
                parent: pair,
                index: i,
                node: filled(
                    "in",
                    VectorShape::Rect {
                        width: 30.0,
                        height: 30.0,
                        radius: 0.0,
                    },
                    if i == 0 { RED } else { BLUE },
                ),
            })
            .unwrap();
            let id = doc.children_of(pair).unwrap()[i];
            doc.apply(Command::SetTransform {
                id,
                transform: Transform::translation(*at, 10.0),
            })
            .unwrap();
        }
        doc.apply(Command::SetOpacity {
            id: doc.children_of(pair).unwrap()[1],
            opacity: 0.6,
        })
        .unwrap();
        doc.apply(Command::SetBlendMode {
            id: pair,
            blend: BlendMode::Multiply,
        })
        .unwrap();
        assert!(GpuRenderer::can_render(&doc), "a blend on a group is drawn");
        let (mean, worst) = difference(
            &gpu.render(&doc).unwrap(),
            &chitrakar_render::render(&doc).unwrap(),
        );
        assert!(
            mean < 0.004,
            "a blended group: mean {mean:.5} (worst {worst:.3})"
        );

        // And the same group held to a mask as well: the mask rides on
        // the quad that brings the blended surface down.
        doc.apply(Command::SetMask {
            id: pair,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Ellipse { rx: 18.0, ry: 18.0 },
                    transform: Transform::translation(12.0, 12.0),
                },
                invert: false,
                feather: 0.0,
            })),
        })
        .unwrap();
        assert!(GpuRenderer::can_render(&doc));
        let held = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&held, &reference);
        assert!(
            mean < 0.004,
            "blended and masked: mean {mean:.5} (worst {worst:.3})"
        );
        // Outside the mask the page is its own colour again, and the
        // two renderers agree there to the pixel.
        assert!(
            (held.get(70, 50).r - reference.get(70, 50).r).abs() < 0.01,
            "outside the mask nothing of the group is left: {:?}",
            held.get(70, 50)
        );
    }

    /// Adjustment layers: each rewrites what is composited below it, and
    /// each is held against the CPU's own answer. The ones stated by a
    /// table — a curve, a gradient map — and the two that speak in bands
    /// of colour are not here yet, and the page goes back for them.
    #[test]
    fn adjustment_layers_rewrite_the_page_the_way_the_cpu_does() {
        use chitrakar_doc::Adjustment as A;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let known = [
            A::Exposure { stops: 0.8 },
            A::BrightnessContrast {
                brightness: 0.15,
                contrast: 0.4,
            },
            A::HueSaturation {
                hue_degrees: 40.0,
                saturation: 0.3,
                lightness: 0.05,
            },
            A::Levels {
                in_black: 0.1,
                in_white: 0.85,
                gamma: 1.4,
                out_black: 0.05,
                out_white: 0.95,
            },
            A::WhiteBalance {
                temperature: 0.4,
                tint: -0.2,
            },
            A::Vibrance { amount: 0.7 },
            A::BlackAndWhite {
                red: 0.5,
                green: 0.3,
                blue: 0.2,
            },
            A::Invert { amount: 1.0 },
            A::ShadowsHighlights {
                shadows: 0.6,
                highlights: 0.5,
            },
        ];
        for adj in known {
            let mut doc = Document::new(60, 40, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "dark",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.2,
                        g: 0.35,
                        b: 0.6,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            add(
                &mut doc,
                filled(
                    "light",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.85,
                        g: 0.7,
                        b: 0.3,
                        a: 1.0,
                    },
                ),
                Transform::translation(0.0, 20.0),
            );
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 2,
                node: Box::new(Node::adjustment("adj", adj.clone())),
            })
            .unwrap();
            assert!(
                GpuRenderer::can_render(&doc),
                "{adj:?} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.004,
                "{adj:?}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
            // It did something, and to both halves of the page.
            let bare = chitrakar_render::render(&{
                let mut without = doc.clone();
                let id = without.children_of(root).unwrap()[2];
                without.apply(Command::RemoveNode { id }).unwrap();
                without
            })
            .unwrap();
            let moved = |x: u32, y: u32| {
                let (a, b) = (drawn.get(x, y), bare.get(x, y));
                (a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs()
            };
            assert!(
                moved(30, 10) > 0.01 && moved(30, 30) > 0.01,
                "{adj:?} reaches both halves: {} and {}",
                moved(30, 10),
                moved(30, 30)
            );
        }

        // Its opacity and its mask weigh it, and half of it is half of
        // the difference it makes.
        let mut doc = Document::new(40, 40, ColorMode::Rgb);
        add(
            &mut doc,
            filled(
                "under",
                VectorShape::Rect {
                    width: 40.0,
                    height: 40.0,
                    radius: 0.0,
                },
                AuthoredColor::Srgb {
                    r: 0.4,
                    g: 0.5,
                    b: 0.6,
                    a: 1.0,
                },
            ),
            Transform::default(),
        );
        let root = doc.root();
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::adjustment("half", A::Invert { amount: 1.0 })),
        })
        .unwrap();
        let adj = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetOpacity {
            id: adj,
            opacity: 0.5,
        })
        .unwrap();
        doc.apply(Command::SetMask {
            id: adj,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: 20.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    transform: Transform::default(),
                },
                invert: false,
                feather: 0.0,
            })),
        })
        .unwrap();
        let weighed = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&weighed, &reference);
        assert!(
            mean < 0.004,
            "weighed by opacity and mask: mean {mean:.5} (worst {worst:.3})"
        );
        assert!(
            (weighed.get(30, 20).r - 0.133).abs() < 0.02,
            "outside the mask it is untouched: {:?}",
            weighed.get(30, 20)
        );
        assert!(
            (weighed.get(10, 20).r - weighed.get(30, 20).r).abs() > 0.02,
            "and inside it, half inverted: {:?}",
            weighed.get(10, 20)
        );
    }

    /// Filter layers. Two of the five are a function of one pixel and of
    /// where that pixel is on the page, so they ride the same copy-aside
    /// an adjustment does; the other three read a neighbourhood, and the
    /// page goes back to the CPU for them.
    #[test]
    fn the_filters_that_read_one_pixel_are_drawn_the_way_the_cpu_draws_them() {
        use chitrakar_doc::Filter as F;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let two_bands = || {
            let mut doc = Document::new(60, 40, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "dark",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.2,
                        g: 0.35,
                        b: 0.6,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            add(
                &mut doc,
                filled(
                    "light",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.85,
                        g: 0.7,
                        b: 0.3,
                        a: 1.0,
                    },
                ),
                Transform::translation(0.0, 20.0),
            );
            doc
        };
        let with = |filter: F| {
            let mut doc = two_bands();
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 2,
                node: Box::new(Node::filter("f", filter)),
            })
            .unwrap();
            doc
        };

        for filter in [
            F::Vignette {
                amount: 0.8,
                radius: 0.2,
                softness: 0.6,
            },
            F::Vignette {
                amount: -0.5,
                radius: 0.0,
                softness: 0.0,
            },
            F::Noise {
                amount: 0.5,
                grain: 3.0,
                mono: true,
                seed: 0x9e37_79b9,
            },
            F::Noise {
                amount: 0.4,
                grain: 1.5,
                mono: false,
                seed: 7,
            },
        ] {
            let doc = with(filter.clone());
            assert!(
                GpuRenderer::can_render(&doc),
                "{filter:?} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            // Grain is a hash of the cell a pixel is in, so a shader that
            // mixed its bits differently would not be close — it would be
            // a different field of specks. This is a tight number on
            // purpose.
            assert!(
                mean < 0.004,
                "{filter:?}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
            let bare = chitrakar_render::render(&two_bands()).unwrap();
            let moved = |x: u32, y: u32| {
                let (a, b) = (drawn.get(x, y), bare.get(x, y));
                (a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs()
            };
            // A vignette leaves the middle alone and takes the corners,
            // which is the whole point of it; grain is everywhere.
            let (near, far) = (moved(30, 20), moved(2, 2));
            assert!(far > 0.01, "{filter:?} reaches the corner: {far}");
            if matches!(filter, F::Vignette { .. }) {
                assert!(
                    near < far,
                    "{filter:?} holds the middle: {near} against {far}"
                );
            }
        }

        // A grid of squares upright on the page is two passes; the
        // pixelate test below holds them to the CPU's own grid.
        assert!(GpuRenderer::can_render(&with(F::Pixelate { size: 6.0 })));

        // And a filter carrying a blend mode goes back too: the CPU
        // renderer writes a filter straight into what it read and never
        // looks at the mode, so a surface of its own here would be a
        // different picture.
        let mut doc = with(F::Vignette {
            amount: 0.8,
            radius: 0.2,
            softness: 0.4,
        });
        let id = doc.children_of(doc.root()).unwrap()[2];
        doc.apply(Command::SetBlendMode {
            id,
            blend: BlendMode::Multiply,
        })
        .unwrap();
        assert!(!GpuRenderer::can_render(&doc));

        // Opacity and a mask weigh a filter exactly as they weigh an
        // adjustment: half of it is half the difference it makes, and
        // outside the mask there is none.
        let mut doc = with(F::Vignette {
            amount: 1.0,
            radius: 0.0,
            softness: 0.0,
        });
        let id = doc.children_of(doc.root()).unwrap()[2];
        doc.apply(Command::SetOpacity { id, opacity: 0.5 }).unwrap();
        doc.apply(Command::SetMask {
            id,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: 30.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    transform: Transform::default(),
                },
                invert: false,
                feather: 0.0,
            })),
        })
        .unwrap();
        let weighed = gpu.render(&doc).unwrap();
        let reference = chitrakar_render::render(&doc).unwrap();
        let (mean, worst) = difference(&weighed, &reference);
        assert!(
            mean < 0.004,
            "weighed by opacity and mask: mean {mean:.5} (worst {worst:.3})"
        );
        let bare = chitrakar_render::render(&two_bands()).unwrap();
        assert!(
            (weighed.get(45, 2).r - bare.get(45, 2).r).abs() < 0.002,
            "the corner outside the mask is untouched"
        );
        // Inside it, taken down by half of what the vignette asked for.
        // The corner sits 0.668 of the way out with nothing held back
        // and no easing, so the reading is 1 − 1.0 × 0.5 × 0.668.
        let ratio = weighed.get(14, 2).r / bare.get(14, 2).r;
        assert!(
            (ratio - 0.666).abs() < 0.01,
            "and the corner inside it is taken down by half: {ratio}"
        );
    }

    /// A pixelate: two passes, one along each axis, which together are
    /// the average over each block of the grid.
    ///
    /// The grid is laid out in the document rather than on the page, so
    /// which square a pixel lands in has to be worked out the same way on
    /// both sides — a pixel falling one side of a block's edge here and
    /// the other side there puts a whole row in the wrong square, which
    /// no amount of "looks blocky" would catch.
    #[test]
    fn a_grid_of_squares_falls_where_the_cpu_puts_it() {
        use chitrakar_doc::Filter as F;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        // A page with structure both ways, so a grid that has slipped
        // along either axis shows.
        let page = || {
            let mut doc = Document::new(60, 40, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "back",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.1,
                        g: 0.12,
                        b: 0.2,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            add(
                &mut doc,
                filled(
                    "bar",
                    VectorShape::Rect {
                        width: 41.0,
                        height: 7.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.95,
                        g: 0.9,
                        b: 0.5,
                        a: 1.0,
                    },
                ),
                Transform::translation(9.0, 16.0),
            );
            add(
                &mut doc,
                filled(
                    "post",
                    VectorShape::Rect {
                        width: 5.0,
                        height: 31.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.3,
                        g: 0.8,
                        b: 0.6,
                        a: 1.0,
                    },
                ),
                Transform::translation(38.0, 4.0),
            );
            doc
        };
        let with = |filter: F| {
            let mut doc = page();
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 3,
                node: Box::new(Node::filter("f", filter)),
            })
            .unwrap();
            doc
        };

        // Sizes that divide the page and sizes that do not, since a block
        // hanging over the edge is averaged from the part inside it.
        for size in [2.0f32, 5.0, 7.0, 12.5] {
            let doc = with(F::Pixelate { size });
            assert!(
                GpuRenderer::can_render(&doc),
                "a grid of {size} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.002,
                "a grid of {size}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
        }

        // The grid lives in the space the filter sits in, so a group that
        // scales it makes the squares bigger on the page and one that
        // shifts it moves where their edges fall. Both are where the
        // origin and the inverse scale earn their keep: a grid worked out
        // from the page alone would agree with the CPU only at identity.
        // The whole page inside the group, with the filter last in it, so
        // there is something under the filter for it to work on: a group
        // holding nothing but a filter is a filter with nothing below it,
        // which draws the page unchanged and would have made every case
        // here agree by drawing nothing.
        let inside = |at: Transform, size: f32| {
            let mut doc = Document::new(60, 40, ColorMode::Rgb);
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::group("held")),
            })
            .unwrap();
            let group = doc.children_of(root).unwrap()[0];
            doc.apply(Command::SetTransform {
                id: group,
                transform: at,
            })
            .unwrap();
            for (i, (name, w, h, colour, x, y)) in [
                ("back", 60.0, 40.0, [0.1, 0.12, 0.2], 0.0, 0.0),
                ("bar", 41.0, 7.0, [0.95, 0.9, 0.5], 9.0, 16.0),
                ("post", 5.0, 31.0, [0.3, 0.8, 0.6], 38.0, 4.0),
            ]
            .into_iter()
            .enumerate()
            {
                doc.apply(Command::AddNode {
                    parent: group,
                    index: i,
                    node: filled(
                        name,
                        VectorShape::Rect {
                            width: w,
                            height: h,
                            radius: 0.0,
                        },
                        AuthoredColor::Srgb {
                            r: colour[0],
                            g: colour[1],
                            b: colour[2],
                            a: 1.0,
                        },
                    ),
                })
                .unwrap();
                let id = doc.children_of(group).unwrap()[i];
                doc.apply(Command::SetTransform {
                    id,
                    transform: Transform::translation(x, y),
                })
                .unwrap();
            }
            doc.apply(Command::AddNode {
                parent: group,
                index: 3,
                node: Box::new(Node::filter("f", F::Pixelate { size })),
            })
            .unwrap();
            doc
        };
        for (name, at, size) in [
            (
                "scaled",
                Transform {
                    a: 2.0,
                    b: 0.0,
                    c: 0.0,
                    d: 2.0,
                    e: 0.0,
                    f: 0.0,
                },
                5.0,
            ),
            (
                "shifted",
                Transform {
                    a: 1.0,
                    b: 0.0,
                    c: 0.0,
                    d: 1.0,
                    e: 3.5,
                    f: -2.25,
                },
                5.0,
            ),
            (
                "both, and mirrored",
                Transform {
                    a: -1.5,
                    b: 0.0,
                    c: 0.0,
                    d: 1.25,
                    e: 47.0,
                    f: 1.0,
                },
                5.0,
            ),
            // Shrunk, with a block smaller than a device pixel asked
            // for: not a block anyone means, and both renderers put the
            // same floor of one device pixel under it.
            (
                "shrunk, with a block finer than a pixel",
                Transform {
                    a: 0.5,
                    b: 0.0,
                    c: 0.0,
                    d: 0.5,
                    e: 6.0,
                    f: 4.0,
                },
                0.4,
            ),
        ] {
            let doc = inside(at, size);
            assert!(
                GpuRenderer::can_render(&doc),
                "a grid {name} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.002,
                "a grid {name}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
        }

        // And it is a grid rather than a smoothing: inside one square
        // every pixel is the same colour, and the square beside it is a
        // different one where the picture underneath changes.
        let blocky = gpu.render(&with(F::Pixelate { size: 5.0 })).unwrap();
        let square = |x: u32, y: u32| blocky.get(x, y).to_srgb8();
        assert_eq!(
            square(11, 16),
            square(13, 18),
            "one square is one colour throughout"
        );
        assert_ne!(
            square(11, 16),
            square(11, 11),
            "and the square above it, over the bar's edge, is another"
        );

        // A turned filter is handed back: the blocks lie at an angle on
        // the page and neither pass can walk them.
        let mut turned = page();
        let root = turned.root();
        turned
            .apply(Command::AddNode {
                parent: root,
                index: 3,
                node: Box::new(Node::group("tilted")),
            })
            .unwrap();
        let group = turned.children_of(root).unwrap()[3];
        turned
            .apply(Command::SetTransform {
                id: group,
                transform: Transform {
                    a: 0.9,
                    b: 0.4,
                    c: -0.4,
                    d: 0.9,
                    e: 0.0,
                    f: 0.0,
                },
            })
            .unwrap();
        turned
            .apply(Command::AddNode {
                parent: group,
                index: 0,
                node: Box::new(Node::filter("f", F::Pixelate { size: 5.0 })),
            })
            .unwrap();
        assert!(!GpuRenderer::can_render(&turned), "a turned grid goes back");

        // And so does a block wider than a pass is willing to walk.
        assert!(
            !GpuRenderer::can_render(&with(F::Pixelate { size: 200.0 })),
            "a block wider than the walk goes back"
        );
    }

    /// A motion blur: one pass along the line it was given, rather than
    /// the blur's six along the axes. The whole claim of a directional
    /// blur is the direction, and a smear that ran the wrong way — or
    /// that took its taps a pixel out — looks perfectly plausible on its
    /// own, so it is held against the CPU renderer's own smear.
    #[test]
    fn a_smear_runs_the_way_the_cpu_runs_it() {
        use chitrakar_doc::Filter as F;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        // A bar across the middle of a dark page: thin enough that a
        // smear across it is obvious and a smear along it is not.
        let page = || {
            let mut doc = Document::new(60, 40, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "back",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.1,
                        g: 0.12,
                        b: 0.2,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            add(
                &mut doc,
                filled(
                    "bar",
                    VectorShape::Rect {
                        width: 40.0,
                        height: 6.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.95,
                        g: 0.9,
                        b: 0.5,
                        a: 1.0,
                    },
                ),
                Transform::translation(10.0, 17.0),
            );
            // And one the other way, so that a smear along either axis
            // has edges to move. A page uniform along x would make a
            // horizontal smear of the wrong length look right.
            add(
                &mut doc,
                filled(
                    "post",
                    VectorShape::Rect {
                        width: 5.0,
                        height: 30.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.3,
                        g: 0.8,
                        b: 0.6,
                        a: 1.0,
                    },
                ),
                Transform::translation(40.0, 5.0),
            );
            doc
        };
        let with = |filter: F| {
            let mut doc = page();
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 3,
                node: Box::new(Node::filter("f", filter)),
            })
            .unwrap();
            doc
        };

        for filter in [
            F::MotionBlur {
                distance: 12.0,
                degrees: 0.0,
            },
            F::MotionBlur {
                distance: 9.0,
                degrees: 90.0,
            },
            F::MotionBlur {
                distance: 20.0,
                degrees: 30.0,
            },
            // Pointed backwards, which is the same line: a smear is
            // centred on the pixel and has no near end.
            F::MotionBlur {
                distance: 12.0,
                degrees: 180.0,
            },
            // Shorter than a pixel, which moves nothing at all.
            F::MotionBlur {
                distance: 0.2,
                degrees: 45.0,
            },
        ] {
            let doc = with(filter.clone());
            assert!(
                GpuRenderer::can_render(&doc),
                "{filter:?} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.004,
                "{filter:?}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
        }

        // And the direction is read rather than merely obeyed as a
        // quantity: along the bar the rows above it stay dark and the bar
        // stays bright; across it the bar bleeds upward and thins.
        let bare = gpu.render(&page()).unwrap();
        let along = gpu
            .render(&with(F::MotionBlur {
                distance: 16.0,
                degrees: 0.0,
            }))
            .unwrap();
        let across = gpu
            .render(&with(F::MotionBlur {
                distance: 16.0,
                degrees: 90.0,
            }))
            .unwrap();
        let above = |s: &chitrakar_render::Surface| s.get(30, 12).r;
        let middle = |s: &chitrakar_render::Surface| s.get(30, 20).r;
        assert!(
            (above(&along) - above(&bare)).abs() < 0.01,
            "smeared along itself the bar has not spread ({} against {})",
            above(&along),
            above(&bare)
        );
        assert!(
            above(&across) > above(&bare) + 0.05,
            "smeared across itself it has ({} against {})",
            above(&across),
            above(&bare)
        );
        assert!(
            middle(&across) < middle(&along) - 0.05,
            "and the middle is thinner for it ({} against {})",
            middle(&across),
            middle(&along)
        );
    }

    /// Blur and sharpen: six box passes on a pair of scratch textures,
    /// three along each axis, which is the CPU renderer's Gaussian
    /// written out as passes.
    #[test]
    fn a_blur_is_the_same_six_averagings_the_cpu_takes() {
        use chitrakar_doc::Filter as F;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        // Something with edges in it, so a blur has work to do: a light
        // square on a dark page.
        let page = || {
            let mut doc = Document::new(60, 40, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "back",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.15,
                        g: 0.2,
                        b: 0.3,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            add(
                &mut doc,
                filled(
                    "square",
                    VectorShape::Rect {
                        width: 20.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.9,
                        g: 0.85,
                        b: 0.4,
                        a: 1.0,
                    },
                ),
                Transform::translation(20.0, 10.0),
            );
            doc
        };
        let with = |filter: F| {
            let mut doc = page();
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 2,
                node: Box::new(Node::filter("f", filter)),
            })
            .unwrap();
            doc
        };

        for filter in [
            F::GaussianBlur { sigma: 1.0 },
            F::GaussianBlur { sigma: 4.0 },
            F::Sharpen {
                sigma: 2.0,
                amount: 0.8,
            },
            F::Sharpen {
                sigma: 1.0,
                amount: -0.6,
            },
        ] {
            let doc = with(filter.clone());
            assert!(
                GpuRenderer::can_render(&doc),
                "{filter:?} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.004,
                "{filter:?}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
        }

        // A blur softens the square's edge and a sharpen hardens it: at
        // the edge the two go opposite ways from the page that has
        // neither, which is the reading that says the passes ran in the
        // right order rather than merely ran.
        let bare = chitrakar_render::render(&page()).unwrap();
        let soft = gpu.render(&with(F::GaussianBlur { sigma: 4.0 })).unwrap();
        let hard = gpu
            .render(&with(F::Sharpen {
                sigma: 4.0,
                amount: 1.0,
            }))
            .unwrap();
        // Just inside the square's left edge, which the blur pulls down
        // towards the dark page and the sharpen pushes further up.
        let (b, s, h) = (bare.get(21, 20).r, soft.get(21, 20).r, hard.get(21, 20).r);
        assert!(s < b - 0.02, "a blur softens the edge: {s} against {b}");
        assert!(h > b + 0.02, "a sharpen hardens it: {h} against {b}");
        // And flat page well out of the blur's reach — three box passes
        // of radius four reach twelve pixels, and this is eighteen from
        // the square — is left where it was by both.
        for (name, got) in [
            ("blurred", soft.get(2, 20).r),
            ("sharpened", hard.get(2, 20).r),
        ] {
            assert!(
                (got - bare.get(2, 20).r).abs() < 0.01,
                "flat page out of reach is untouched when {name}: {got}"
            );
        }

        // A blur too small to move a pixel, and a sharpen asked for
        // nothing, are both nothing — the CPU renderer returns without
        // touching the page, and so does this.
        for filter in [
            F::GaussianBlur { sigma: 0.0 },
            F::Sharpen {
                sigma: 3.0,
                amount: 0.0,
            },
        ] {
            let doc = with(filter.clone());
            assert!(GpuRenderer::can_render(&doc));
            let (mean, _) = difference(&gpu.render(&doc).unwrap(), &bare);
            assert!(mean < 0.001, "{filter:?} draws nothing: {mean}");
        }

        // Opacity and a mask weigh a blur the way they weigh everything
        // else here.
        let mut doc = with(F::GaussianBlur { sigma: 4.0 });
        let id = doc.children_of(doc.root()).unwrap()[2];
        doc.apply(Command::SetOpacity { id, opacity: 0.5 }).unwrap();
        doc.apply(Command::SetMask {
            id,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: 30.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    transform: Transform::default(),
                },
                invert: false,
                feather: 0.0,
            })),
        })
        .unwrap();
        let weighed = gpu.render(&doc).unwrap();
        let (mean, worst) = difference(&weighed, &chitrakar_render::render(&doc).unwrap());
        assert!(
            mean < 0.004,
            "weighed by opacity and mask: mean {mean:.5} (worst {worst:.3})"
        );
        // Half the softening on the left edge, none on the right one.
        assert!(
            (weighed.get(21, 20).r - (b + s) / 2.0).abs() < 0.01,
            "half of it inside the mask: {}",
            weighed.get(21, 20).r
        );
        assert!(
            (weighed.get(39, 20).r - bare.get(39, 20).r).abs() < 0.005,
            "and none of it outside: {}",
            weighed.get(39, 20).r
        );
    }

    /// The four adjustments with more to say than a handful of numbers:
    /// three read off a table the CPU renderer builds — the curves, the
    /// ramp, the bands — and the fourth carries ten numbers on the quad.
    #[test]
    fn the_adjustments_stated_by_a_table_read_the_cpu_s_own() {
        use chitrakar_doc::Adjustment as A;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let told = [
            A::Curves {
                // A master curve that lifts the middle, and a red curve
                // after it that pulls its own back down: both have to
                // land, in that order.
                points: vec![[0.0, 0.0], [0.5, 0.72], [1.0, 1.0]],
                red: vec![[0.0, 0.0], [0.5, 0.32], [1.0, 1.0]],
                green: Vec::new(),
                blue: Vec::new(),
            },
            A::GradientMap {
                stops: vec![
                    chitrakar_doc::GradientStop {
                        offset: 0.0,
                        color: AuthoredColor::Srgb {
                            r: 0.1,
                            g: 0.0,
                            b: 0.3,
                            a: 1.0,
                        },
                    },
                    chitrakar_doc::GradientStop {
                        offset: 1.0,
                        color: AuthoredColor::Srgb {
                            r: 1.0,
                            g: 0.85,
                            b: 0.2,
                            a: 1.0,
                        },
                    },
                ],
            },
            A::SelectiveHsl {
                // The blues deepened, the greens left alone.
                bands: vec![
                    [0.0; 3],
                    [0.0; 3],
                    [0.0; 3],
                    [0.0; 3],
                    [0.2, 0.8, -0.1],
                    [0.0; 3],
                ],
            },
            A::ColorBalance {
                shadows: [-0.4, 0.0, 0.5],
                midtones: [0.2, -0.1, 0.0],
                highlights: [0.5, 0.0, -0.4],
                preserve_luminosity: true,
            },
            A::ColorBalance {
                shadows: [-0.4, 0.0, 0.5],
                midtones: [0.2, -0.1, 0.0],
                highlights: [0.5, 0.0, -0.4],
                preserve_luminosity: false,
            },
        ];
        for adj in told {
            let mut doc = Document::new(60, 40, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "dark",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.2,
                        g: 0.35,
                        b: 0.6,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            add(
                &mut doc,
                filled(
                    "light",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.85,
                        g: 0.7,
                        b: 0.3,
                        a: 1.0,
                    },
                ),
                Transform::translation(0.0, 20.0),
            );
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 2,
                node: Box::new(Node::adjustment("adj", adj.clone())),
            })
            .unwrap();
            assert!(
                GpuRenderer::can_render(&doc),
                "{adj:?} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.004,
                "{adj:?}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
            for (x, y) in [(30u32, 10u32), (30, 30)] {
                let (a, b) = (drawn.get(x, y), reference.get(x, y));
                assert!(
                    (a.r - b.r).abs() < 0.02
                        && (a.g - b.g).abs() < 0.02
                        && (a.b - b.b).abs() < 0.02,
                    "{adj:?} at {x},{y}: {a:?} against {b:?}"
                );
            }
        }

        // A gradient map with nothing to map through leaves the picture
        // as it is, on both.
        let mut bare = Document::new(20, 20, ColorMode::Rgb);
        add(
            &mut bare,
            filled(
                "r",
                VectorShape::Rect {
                    width: 20.0,
                    height: 20.0,
                    radius: 0.0,
                },
                RED,
            ),
            Transform::default(),
        );
        let root = bare.root();
        bare.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: Box::new(Node::adjustment(
                "empty",
                A::GradientMap { stops: Vec::new() },
            )),
        })
        .unwrap();
        assert!(GpuRenderer::can_render(&bare));
        let (mean, _) = difference(
            &gpu.render(&bare).unwrap(),
            &chitrakar_render::render(&bare).unwrap(),
        );
        assert!(mean < 0.001, "a ramp with no stops changes nothing: {mean}");
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

        // A live effect, or ink authored for a press: either on its own
        // is enough to hand the page back. A stroke is not — that one it
        // draws, and nor is a blend mode any more.
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
                        dash: Vec::new(),
                        cap: Default::default(),
                        join: Default::default(),
                        align: None,
                        start_marker: Default::default(),
                        end_marker: Default::default(),
                    }),
                    gradient: None,
                }),
            })
            .unwrap();
        assert!(GpuRenderer::can_render(&with_stroke));

        let mut with_effect = doc.clone();
        with_effect
            .apply(Command::SetEffects {
                id,
                effects: vec![chitrakar_doc::Effect::Outline {
                    color: BLUE,
                    width: 2.0,
                    opacity: 1.0,
                }],
            })
            .unwrap();
        assert!(!GpuRenderer::can_render(&with_effect));

        // A layer held to the one under it is drawn, since that layer's
        // own alpha is a coverage like a mask's — but only where "its
        // alpha" is a plain question. Faded, the base's alpha depends on
        // how it was composited, and the page goes back.
        let mut held = doc.clone();
        let over = add(
            &mut held,
            filled("over", rect.clone(), BLUE),
            Transform::translation(6.0, 6.0),
        );
        held.apply(Command::SetClipped {
            id: over,
            clipped: true,
        })
        .unwrap();
        assert!(GpuRenderer::can_render(&held), "held to a plain layer");
        let mut faded = held.clone();
        faded
            .apply(Command::SetOpacity { id, opacity: 0.5 })
            .unwrap();
        assert!(!GpuRenderer::can_render(&faded), "held to a faded one");

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
        let mut hidden = with_effect.clone();
        hidden
            .apply(Command::SetVisible { id, visible: false })
            .unwrap();
        assert!(GpuRenderer::can_render(&hidden));
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
