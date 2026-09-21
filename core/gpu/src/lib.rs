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
//! What is declined, and falls back to the CPU, is **not listed here**,
//! on purpose. A list in prose is a list that goes stale: three things
//! this paragraph used to name had been drawn for some while before
//! anybody read the walk again, and one of them was in the roadmap as
//! work still to do. `what_it_will_draw_is_written_down` is the list —
//! every kind of layer against everything a layer can wear, asserted
//! rather than described, with the reason for each `no` beside it — and
//! it is the one to change and the one to read.
//!
//! Declining is always a safe answer: the page comes out right, more
//! slowly. Drawing the wrong thing never is, which is what the audit
//! over every command is there to catch.

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
    /// One stroke of a brush: its segments, gathered into a coverage of
    /// their own on a scratch texture, and the quad that lays that
    /// coverage down in the stroke's colour — or takes it off, for an
    /// eraser.
    Brush {
        segments: std::ops::Range<u32>,
        quad: std::ops::Range<u32>,
        erase: bool,
    },
    /// One clone stroke: the same gathered coverage, filled with what
    /// the surface already holds a fixed distance away rather than with
    /// a colour.
    Clone {
        segments: std::ops::Range<u32>,
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
        /// The layer's live effects, each built from the silhouette that
        /// surface holds: the ones behind it go down before it and the
        /// ones over it after, which is the order the CPU renderer takes
        /// and the only one an inner shadow makes sense in.
        effects: Vec<Painted>,
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
    field: wgpu::RenderPipeline,
    /// One pass of an outline's band: two of these measure the exact
    /// distance from the silhouette, one down each column and one along
    /// each row.
    band: wgpu::RenderPipeline,
    /// A field read at its offset and held inside the silhouette before
    /// it goes down, so a blend can have the texture the stamp would
    /// otherwise be using for that.
    settle: wgpu::RenderPipeline,
    effect: wgpu::RenderPipeline,
    brush: wgpu::RenderPipeline,
    paint: wgpu::RenderPipeline,
    eraser: wgpu::RenderPipeline,
    /// A clone stroke: the same coverage, filled with what the surface
    /// already holds a fixed distance away, composited in full.
    clone: wgpu::RenderPipeline,
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
        let composite = |label: &'static str, entry: &'static str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_image"),
                    compilation_options: Default::default(),
                    buffers: std::slice::from_ref(&vertex_layout),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
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
            })
        };
        // A clone stroke, which works its whole answer out for the same
        // reason a blend does — what it lifts and what it lands on are
        // both what was already there.
        let clone = composite("clone", "fs_clone");
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
        // A layer's silhouette in one flat colour, which every live
        // effect is built from, and the blurred field coming back down
        // over what is under the layer.
        let one_of = |label: &'static str, entry: &'static str, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_image"),
                    compilation_options: Default::default(),
                    buffers: std::slice::from_ref(&vertex_layout),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        blend: Some(blend),
                        ..target.clone()
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let field = one_of("effect field", "fs_field", wgpu::BlendState::REPLACE);
        let band = one_of("outline band", "fs_band", wgpu::BlendState::REPLACE);
        let settle = one_of("effect settle", "fs_settle", wgpu::BlendState::REPLACE);
        // A brush stroke's segments, gathered with max blending: they
        // union rather than pile up, so a stroke that doubles back is
        // not darker where it crossed itself.
        let most = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Max,
        };
        let brush = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("brush"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_brush"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    blend: Some(wgpu::BlendState {
                        color: most,
                        alpha: most,
                    }),
                    ..target.clone()
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        // The stroke coming down in its colour, over what the strokes
        // before it left; and an eraser, which is the same coverage
        // taking off instead of laying on.
        let off = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        // On a page, so multisampled and carrying the stencil every
        // other pass that draws on one does.
        let laying = |label: &'static str, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_image"),
                    compilation_options: Default::default(),
                    buffers: std::slice::from_ref(&vertex_layout),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_paint"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        blend: Some(blend),
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
            })
        };
        let paint = laying("paint", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let eraser = laying(
            "eraser",
            wgpu::BlendState {
                color: off,
                alpha: off,
            },
        );
        // The stamp goes onto the surface under the layer, so it
        // composites over what is already there the way a placed picture
        // does — the same target and the same stencil as every other
        // pass that draws on a page.
        let effect = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("effect"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vertex_layout),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_effect"),
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
            field,
            band,
            settle,
            effect,
            brush,
            paint,
            eraser,
            clone,
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
        let size = (doc.meta.width, doc.meta.height);
        self.render_view(doc, Transform::default(), size)
    }

    /// The page drawn onto a surface of `size` device pixels, with the
    /// document mapped through `view` on the way — which is what lets the
    /// surface stop being the page: a scale of two on a surface twice the
    /// size draws the document at twice the resolution, outlines re-solved
    /// at that scale rather than a magnified bitmap, and a view that also
    /// translates draws whatever part of the document the surface is
    /// looking at.
    ///
    /// The same mapping the CPU renderer takes in `render_region_at`, so
    /// the two can be held against each other at any view rather than
    /// only at the page's own size.
    pub fn render_view(
        &self,
        doc: &Document,
        view: Transform,
        size: (u32, u32),
    ) -> Option<Surface> {
        let mut scene = Scene::default();
        gather(doc, view, size, &mut scene)?;
        Some(self.draw(size.0, size.1, &scene))
    }

    /// Whether [`render`](Self::render) would draw this document.
    pub fn can_render(doc: &Document) -> bool {
        let size = (doc.meta.width, doc.meta.height);
        Self::can_render_view(doc, Transform::default(), size)
    }

    /// Whether [`render_view`](Self::render_view) would draw it.
    pub fn can_render_view(doc: &Document, view: Transform, size: (u32, u32)) -> bool {
        gather(doc, view, size, &mut Scene::default()).is_some()
    }

    fn draw(&self, width: u32, height: u32, scene: &Scene) -> Surface {
        let page = scene.page;
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
                contents: bytemuck::cast_slice(&[
                    width as f32,
                    height as f32,
                    page.x0 as f32,
                    page.y0 as f32,
                    page.x1 as f32,
                    page.y1 as f32,
                    scene.size[0],
                    scene.size[1],
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        // The same numbers with the page's rectangle standing for the
        // whole surface: what a live effect's field is built and blurred
        // over, since the silhouette it comes from is wherever the layer
        // is rather than wherever the page is.
        let everywhere = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("surface"),
                contents: bytemuck::cast_slice(&[
                    width as f32,
                    height as f32,
                    0.0,
                    0.0,
                    width as f32,
                    height as f32,
                    scene.size[0],
                    scene.size[1],
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let whole = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("surface"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: everywhere.as_entire_binding(),
            }],
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
                    | Some(Opening::Effect { .. })
                    | Some(Opening::Brush { .. })
                    | Some(Opening::Clone { .. })
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
            // A brush stroke, before the pass that lays it down: its
            // segments are gathered into a coverage of their own with
            // max blending, since the segments of one stroke union
            // rather than pile up.
            let gathering = match &step.lay {
                Some(Opening::Brush { segments, .. }) | Some(Opening::Clone { segments, .. }) => {
                    Some(segments)
                }
                _ => None,
            };
            if let (Some(segments), Some(quads)) = (gathering, &quads) {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("brush"),
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
                pass.set_pipeline(&self.brush);
                pass.set_bind_group(0, &whole, &[]);
                pass.set_bind_group(1, &self.open, &[]);
                pass.set_bind_group(2, &self.open, &[]);
                pass.set_bind_group(3, &self.open, &[]);
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.draw(segments.clone(), 0..1);
            }
            // A live effect, before the pass that stamps it down: the
            // layer's own surface goes in, one pass turns its silhouette
            // into a field of flat colour, and the box passes blur that
            // exactly as they blur a page — six of them, three each way,
            // which is the CPU renderer's Gaussian.
            //
            // The field is built and blurred over the whole surface
            // rather than over the page: it is the layer's silhouette
            // that decides where it reaches, and the CPU renderer builds
            // it over a window grown by that reach whether or not the
            // page ends first. Only the stamp is held to the page.
            if let (Some(Opening::Effect { from, at, .. }), Some(quads)) = (&step.lay, &quads) {
                // A band is measured out instead of blurred: two passes
                // rather than twelve, the first down the columns and the
                // second along the rows, which between them are the
                // exact distance from the silhouette.
                let rounds = if at.steps.is_empty() {
                    0
                } else if at.band {
                    2
                } else {
                    6
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("effect field"),
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
                pass.set_pipeline(&self.field);
                pass.set_bind_group(0, &whole, &[]);
                pass.set_bind_group(1, &surfaces[from - 1].2, &[]);
                pass.set_bind_group(2, &self.open, &[]);
                pass.set_bind_group(3, &self.open, &[]);
                pass.set_vertex_buffer(0, quads.slice(..));
                pass.draw(at.field.clone(), 0..1);
                drop(pass);
                let along = |axis: usize| {
                    at.steps.start + 6 * axis as u32..at.steps.start + 6 * axis as u32 + 6
                };
                for round in 0..rounds {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(if at.band {
                            "outline band"
                        } else {
                            "effect blur"
                        }),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &scratch[(round + 1) % 2].0,
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
                    pass.set_pipeline(if at.band { &self.band } else { &self.box_blur });
                    pass.set_bind_group(0, &whole, &[]);
                    pass.set_bind_group(1, &scratch[round % 2].1, &[]);
                    pass.set_bind_group(2, &self.open, &[]);
                    pass.set_bind_group(3, &self.open, &[]);
                    pass.set_vertex_buffer(0, quads.slice(..));
                    pass.draw(along(round % 2), 0..1);
                }
                // A field the layer's blend will bring down is read at
                // its offset and held inside the silhouette here, on the
                // second of the pair, rather than as it lands: the stamp
                // will be reading what is under it by then, and there is
                // one texture for the two. The layer's own surface goes
                // where the backdrop does, which nothing reads on a
                // scratch pass.
                if !at.settle.is_empty() {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("effect settle"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &scratch[1].0,
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
                    pass.set_pipeline(&self.settle);
                    pass.set_bind_group(0, &whole, &[]);
                    pass.set_bind_group(1, &scratch[0].1, &[]);
                    pass.set_bind_group(2, &self.open, &[]);
                    pass.set_bind_group(3, &surfaces[from - 1].2, &[]);
                    pass.set_vertex_buffer(0, quads.slice(..));
                    pass.draw(at.settle.clone(), 0..1);
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
                        Opening::Brush {
                            quad, erase, mask, ..
                        } => {
                            pass.set_pipeline(if *erase { &self.eraser } else { &self.paint });
                            pass.set_bind_group(1, &scratch[0].1, &[]);
                            (quad, mask)
                        }
                        Opening::Clone { quad, mask, .. } => {
                            // What is under the stroke and what it lifts
                            // are the same copy, taken before the pass:
                            // a stroke running over its own source reads
                            // what was there rather than what it has just
                            // laid.
                            pass.set_pipeline(&self.clone);
                            pass.set_bind_group(1, &scratch[0].1, &[]);
                            (quad, mask)
                        }
                        Opening::Effect {
                            from,
                            at,
                            mask,
                            blend,
                        } => {
                            if *blend == BlendMode::Normal || under.is_none() {
                                pass.set_pipeline(&self.effect);
                                // The rounds end on the first of the
                                // pair; a field that was never carried
                                // out from the silhouette is still on it.
                                pass.set_bind_group(1, &scratch[0].1, &[]);
                                // An inner shadow is held to the layer's
                                // own coverage, which is the surface it
                                // was drawn on — read where a blend
                                // reads what is under it, since an
                                // effect otherwise has no use for that.
                                if at.inside {
                                    pass.set_bind_group(3, &surfaces[from - 1].2, &[]);
                                }
                            } else {
                                // Brought down by the layer's blend, the
                                // same way its surface is: the offset
                                // and the hold-inside have already been
                                // taken, on the second of the pair, so
                                // what is left is a picture and what is
                                // under it.
                                pass.set_pipeline(&self.blend);
                                pass.set_bind_group(1, &scratch[1].1, &[]);
                            }
                            (&at.quad, mask)
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
                        | Draw::Blocks { .. }
                        | Draw::Brush { .. }
                        | Draw::Clone { .. } => {}
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
    /// The surface being drawn, in device pixels, and where the page
    /// lands on it once mapped through the view.
    ///
    /// The two were the same thing while this backend drew the page at
    /// its own size. They part company the moment a view is asked for: a
    /// viewport is a window onto the document, so the surface is the
    /// window's size and the page is a rectangle somewhere on it —
    /// sometimes covering the whole of it and sometimes a patch in the
    /// middle. Everything that used to reach for the page's size wants
    /// one or the other of these.
    surface: (u32, u32),
    page: chitrakar_render::ClipRect,
    /// The document's own size, in the document's own units — which is
    /// neither of the two above the moment a view scales anything, and
    /// which a filter measured on the *picture* rather than on the
    /// surface needs: a vignette is placed from the page's middle in
    /// page units, so panning slides the picture under it.
    size: [f32; 2],
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

impl Default for Scene {
    fn default() -> Self {
        Self {
            vertices: Vec::new(),
            draws: Vec::new(),
            textures: Vec::new(),
            ids: Vec::new(),
            surface: (0, 0),
            page: chitrakar_render::ClipRect {
                x0: 0,
                y0: 0,
                x1: 0,
                y1: 0,
            },
            size: [0.0; 2],
        }
    }
}

impl Scene {
    /// Take a run of vertices as the range it occupies.
    fn push(&mut self, verts: Vec<Vertex>) -> std::ops::Range<u32> {
        let start = self.vertices.len() as u32;
        self.vertices.extend(verts);
        start..self.vertices.len() as u32
    }
}

/// Whether anything a layer *draws* carries a mask of its own — its
/// children, or, for a copy, the layer it copies and that one's children.
/// The layer itself is not counted: its own mask is the other half of the
/// pair this asks about.
fn draws_something_masked(doc: &Document, id: NodeId) -> bool {
    fn walk(doc: &Document, id: NodeId, depth: usize) -> bool {
        if depth > 32 {
            // Deeper than anything here, and a ring would otherwise spin.
            return true;
        }
        let Ok(node) = doc.node(id) else {
            return false;
        };
        if depth > 0 && node.mask.is_some() {
            return true;
        }
        if let NodeKind::Instance { of, .. } = &node.kind {
            if walk(doc, *of, depth + 1) {
                return true;
            }
        }
        doc.children_of(id)
            .map(|kids| kids.iter().any(|k| walk(doc, *k, depth + 1)))
            .unwrap_or(false)
    }
    walk(doc, id, 0)
}

/// Everything the page needs drawn onto a surface of `size`, with the
/// document mapped through `view` on the way — or `None` when some of it
/// cannot be.
fn gather(doc: &Document, view: Transform, size: (u32, u32), out: &mut Scene) -> Option<()> {
    if size.0 > MAX_TEXTURE || size.1 > MAX_TEXTURE {
        return None;
    }
    // Where the page lands on the surface, rounded outward but no
    // further: a pixel the page's edge partly covers is the page's to
    // paint and one it does not touch is not, which is the rectangle the
    // CPU renderer works out for the same reason.
    let chitrakar_render::Bounds::Rect(x0, y0, x1, y1) = chitrakar_render::transformed_box(
        view,
        [0.0, 0.0, doc.meta.width as f32, doc.meta.height as f32],
    ) else {
        return None;
    };
    let page = chitrakar_render::ClipRect {
        x0: x0.floor().max(0.0) as u32,
        y0: y0.floor().max(0.0) as u32,
        x1: (x1.ceil().max(0.0) as u32).min(size.0),
        y1: (y1.ceil().max(0.0) as u32).min(size.1),
    };
    out.surface = size;
    out.page = page;
    out.size = [doc.meta.width as f32, doc.meta.height as f32];
    // The page's own edge is what clips the artwork, and with the surface
    // no longer being the page that has to be said rather than assumed.
    let bound = (view != Transform::default()).then_some(page);
    collect(doc, doc.root(), view, 1.0, bound, out)
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
        one(doc, child, parent, opacity, bound, true, out)?;
    }
    Some(())
}

/// One layer, where its parent puts it, turned into quads.
///
/// Pulled out of the walk so a caller with a single layer in mind can
/// ask for it: a copy of another layer draws that layer where the copy
/// is, which is this same work with a different space and no parent to
/// have walked down from.
#[allow(clippy::too_many_arguments)]
fn one(
    doc: &Document,
    child: NodeId,
    parent: Transform,
    opacity: f32,
    bound: Option<chitrakar_render::ClipRect>,
    // Whether this layer is being drawn in its own place among its
    // siblings, where being held to the one below it means something, or
    // as what a *copy* draws — where it does not. A copy draws the layer,
    // not the layer's place in a run of clipped ones, so the renderer
    // being matched draws the layer whole there: it reaches it through
    // `render_layer`, and a clip run is the parent group's business.
    in_a_run: bool,
    out: &mut Scene,
) -> Option<()> {
    let node = doc.node(child).ok()?;
    if !node.visible || node.opacity <= 0.0 {
        return Some(());
    }
    // A layer with live effects goes onto a surface of its own — the
    // effects are built from its silhouette, so there has to be one —
    // and only where its silhouette is a plain question. Its own
    // opacity, its mask and being held to the layer below all belong to
    // the layer as the CPU renderer draws it aside, and this backend
    // puts them on the quad that lays the surface down instead: the two
    // readings agree about the picture and not about the silhouette, so
    // the page goes back rather than casting a shadow of the wrong
    // shape. A group is out for the same reason and one more: what a
    // group's opacity means to its children is not what a layer's means
    // to itself.
    // A layer that rewrites what is under it has no silhouette to build
    // an effect from, and the reference renderer reads that from what the
    // layer *draws* rather than from its kind: a copy of an adjustment or
    // a filter is one of these just as much as the layer it copies. Taken
    // literally the effect would put such a copy on a surface of its own,
    // where what it copies is handed a transparent page to rewrite and
    // comes back with nothing — a copy of a filter wearing a drop shadow
    // at no opacity at all, vanishing. So the effects are ignored here
    // exactly as they are there.
    let rewriting = chitrakar_render::rewrites_what_is_under_it(doc, child);
    let shadings: Vec<Shading> = if node.effects.is_empty() || rewriting {
        Vec::new()
    } else {
        // What is left out, and only these two. A frame: the CPU renderer
        // cuts its contents to its own rectangle before anything is made
        // of them, and the silhouette here is the surface uncut. A clone
        // layer: it paints with what is under it, so it is never on a
        // surface of its own to take a silhouette from. The reference
        // renderer builds one for it out of the strokes it lays — see
        // `a_clone_layer_casts_the_shadow_its_strokes_cast` — which is
        // what makes handing the page back *necessary* here rather than
        // merely safe: there is now a shadow to disagree about.
        // A group and a copy used to be on this list and are not: both
        // are drawn, and the comment saying otherwise outlived the code
        // by long enough to be written into the roadmap as work still to
        // do.
        if !matches!(
            node.kind,
            NodeKind::Vector { .. }
                | NodeKind::Raster(_)
                | NodeKind::Text(_)
                | NodeKind::Paint { .. }
                | NodeKind::Group
                | NodeKind::Instance { .. }
                | NodeKind::Artboard { .. }
        ) {
            return None;
        }
        // One coverage texture, and here two things want it. An effect is
        // built from the layer's silhouette, and that silhouette is what
        // the layer's own mask decides — so the mask rides the slot for
        // the silhouette pass. If something *inside* the layer is masked
        // too, its mask wants the same slot and does not get it: the
        // child's mask is dropped, and not only from the shadow. Measured
        // rather than reasoned — a masked group holding a masked ellipse
        // and casting a drop shadow came out pixel for pixel like the
        // reference renderer's answer for the same page with the child's
        // mask *removed*, while the reference renderer's own answer
        // differed from that by two hundredths of the page and two thirds
        // of a channel at its worst.
        //
        // Either mask alone is fine, which is what makes this narrow: with
        // no mask on the layer the child's rides the slot and is honoured,
        // and with no mask inside there is nothing to collide. So the page
        // goes back only for the pair of them, which is the same answer a
        // stroke carrying a region gets on a layer whose own mask is
        // already on that slot.
        if node.mask.is_some() && draws_something_masked(doc, child) {
            return None;
        }
        node.effects
            .iter()
            .map(|e| effect_of(doc, e, parent, node.opacity))
            .collect::<Option<_>>()?
    };
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
    let held_to = if node.clipped && in_a_run {
        match chitrakar_render::clip_base(doc, child).ok()? {
            // Nothing under it to be held to — the first of a run
            // is what the rest are held to — so it draws whole.
            None => None,
            Some(base) => {
                let b = doc.node(base).ok()?;
                // A base is a layer whose alpha is its *own*: it puts
                // something down, so "the layer drawn aside" means
                // something and both sides read the same number for it.
                // A clone layer, an adjustment and a filter draw by
                // reading what is under them and have no alpha of their
                // own to be held to — drawn aside they are nothing, or
                // nothing like what they are on the page, and the two
                // renderers come apart by a sixth of full scale. Those
                // three go back; everything that paints is a base.
                let draws = !matches!(
                    b.kind,
                    NodeKind::Clone { .. } | NodeKind::Adjustment(_) | NodeKind::Filter(_)
                );
                // What the layer above is held to is the base's own
                // alpha, and that reading comes from the renderer being
                // matched — `layer_coverage_at`, the base drawn aside —
                // so its opacity, its mask and its blend are already in
                // the number both sides read and none of the three is a
                // reason to hand the page back. Asked directly, a base
                // faded, masked or blended lands within a thousandth of
                // the reference, which is where a plain one lands.
                //
                // An *effect* is: the coverage then includes the shadow
                // the base casts, and what the CPU renderer holds the
                // layer above to does not. That one is off by a fiftieth
                // and goes back.
                if !draws || !b.effects.is_empty() {
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
    // A layer that rewrites what is under it has nothing left over to
    // blend against it, and the CPU renderer reads it that way: it hands
    // an adjustment and a filter their opacity and their mask and never
    // looks at the blend mode. Asked directly, a blend makes no
    // difference at all to what it draws.
    //
    // This used to hand the page back, on the grounds that a blend would
    // put the layer on a surface of its own and that would be a
    // different picture. The surface was the problem, not the blend: the
    // passes these two are drawn by carry their own parameters and never
    // read `node.blend`, so the answer is to keep the blend from forcing
    // a surface and then ignore it, which is what the renderer being
    // matched does.
    // What a layer *draws*, not what kind it is: a copy of an adjustment
    // or a filter rewrites the page exactly as the layer it copies does,
    // and taking its blend literally would put it on a surface of its own
    // where what it copies is handed a transparent page to work on and
    // comes back with nothing. The reference renderer had the same
    // `matches!` here and the same hole behind it.
    let rewrites = chitrakar_render::rewrites_what_is_under_it(doc, child);
    // A copy of one of those, wearing a mask or a fade, goes back. What it
    // draws is a change to what is under it, so it cannot go on a surface
    // of its own — there is nothing under a fresh surface to change — and
    // a mask or an opacity below one is exactly what sends a copy to a
    // surface here. The renderer being matched hands such a copy's mask
    // and opacity down as a coverage instead, which is a pass this backend
    // has no shape for: its own masks ride the one coverage slot a layer
    // already uses. Drawing it anyway is how both of them used to lose the
    // layer altogether.
    //
    // A copy of a *blended* layer goes back on the same terms and for the
    // same reason. The blend belongs to what the copy draws, not to the
    // copy, and on a surface of its own it meets a transparent page:
    // every separable blend collapses to Normal against nothing, so the
    // blend is spent and never asked for again. Six pages of six hundred
    // changed colour when a mask that hides nothing went on a copy, and
    // every one of them copied a layer wearing a blend.
    // …and not one wearing a blend of its own: there the reference
    // renderer keeps the surface too, and spends the copied blend on it,
    // so both draw the same picture and there is nothing to decline.
    //
    // Being *held to* the layer below is the third of these and was
    // missing. A held layer is confined to that layer's alpha, which
    // reaches this backend as a coverage — the same pass a mask or a
    // fade on such a copy already goes back for. Drawn alone instead,
    // what the copy copies was handed a transparent page to change and
    // came back with nothing, so a copy of an adjustment held to a
    // shape *vanished* — and the reference renderer lost it the same
    // way, which is why the two agreed and neither was right. It is
    // fixed there and declined here.
    if (rewrites
        || (chitrakar_render::copies_a_blend(doc, child) && node.blend == BlendMode::Normal))
        && matches!(node.kind, NodeKind::Instance { .. })
        && (node.mask.is_some() || node.opacity < 1.0 || held_to.is_some())
    {
        return None;
    }
    let alone = !shadings.is_empty()
        // A brush layer always: its strokes go on one after another and
        // an eraser takes off what the ones before it left, which is a
        // conversation among the strokes and not with the page. On a
        // surface of its own, its opacity, its mask and its blend are
        // taken once over the finished layer — which is what the CPU
        // renderer does to it whenever any of the three is in play, and
        // what source-over being associative makes identical when none
        // of them is.
        || matches!(node.kind, NodeKind::Paint { .. })
        || (node.blend != BlendMode::Normal && !rewrites)
        || (matches!(node.kind, NodeKind::Group)
            && (node.opacity < 1.0
                || node.mask.is_some()
                || chitrakar_render::reads_backdrop(doc, child).ok()?))
        // And a copy on the same terms. What a copy draws is a whole
        // layer, and that layer may be a group whose children overlap: a
        // fade, a mask or the alpha it is held to, taken as each child
        // lands, would be taken twice where two of them meet. So the
        // three things that isolate a group isolate a copy, and the
        // surface is what lands — which is also what the CPU renderer
        // does with such a copy.
        || (matches!(node.kind, NodeKind::Instance { .. })
            && (node.opacity < 1.0 || node.mask.is_some() || held_to.is_some()));
    // Except a clone layer, never: what it paints with is what is under
    // it, and a surface of its own would have nothing under it to paint
    // with. Its blend, its opacity and its mask go on each stroke as it
    // lands, which is where the CPU renderer puts them too.
    let alone = alone && !matches!(node.kind, NodeKind::Clone { .. });
    if alone {
        out.draws.push(Item::of(Draw::Open));
    }
    // What the layer itself is drawn at: its own opacity, unless it is
    // going on a surface of its own, where the opacity belongs to the
    // quad that brings the surface back — except when it has effects,
    // where its own opacity belongs *inside* the surface, because that
    // is what the silhouette a shadow is cast from is made of. A layer
    // at a third opacity casts a third of a shadow, and it does so
    // because its silhouette is a third covered.
    // Whether the layer's own opacity is already inside the surface an
    // effect's silhouette is built from. It is for a layer that draws
    // one thing: the CPU renderer fades the fill and the stroke as it
    // paints them, so where they overlap the fade is taken twice and
    // that overlap is what the silhouette is. It is not for a group,
    // whose opacity belongs to the composite rather than to each child,
    // nor for a brush layer, whose strokes have their conversation with
    // each other before any of it fades. Those two owe the silhouette
    // their opacity when it is built instead.
    let in_surface = matches!(
        node.kind,
        NodeKind::Vector { .. } | NodeKind::Raster(_) | NodeKind::Text(_)
    );
    let owed = if in_surface { 1.0 } else { node.opacity };
    let alpha = match (alone, shadings.is_empty()) {
        (true, true) => 1.0,
        (true, false) => {
            if in_surface {
                node.opacity
            } else {
                1.0
            }
        }
        (false, _) => node.opacity * opacity,
    };
    // Where the layer's own drawing starts, so the mask can be put
    // on everything the layer turns into and nothing else.
    let mut mark = (out.vertices.len(), out.draws.len());
    // What the layer as a whole is held back by, which a stroke carrying
    // a region of its own has to fold into that region rather than lose
    // — the one slot holds one coverage, and two coverages read together
    // are one coverage.
    //
    // Except where the mask is not going on the strokes at all. A layer
    // drawn on a surface of its own with no effects wears its mask on
    // the quad that lays that surface down, which is the whole point of
    // a surface: the strokes have their conversation with each other
    // first and the mask is taken once over the result. Folding it into
    // a stroke as well would take it twice, which a hard edge hides and
    // a feathered one does not. So a paint layer, which is always on one
    // of its own, folds in nothing; one with effects does fold, since
    // there the mask rides the layer's own drawing so that the
    // silhouette a shadow is cast from is the masked one; and a clone
    // layer, never on a surface of its own, always folds.
    let held = if alone && shadings.is_empty() {
        Held {
            mask: None,
            to: None,
            bound: None,
            parent,
        }
    } else {
        Held {
            mask: node.mask.as_ref(),
            to: held_to,
            bound,
            parent,
        }
    };
    // The strokes that did exactly that. They already carry everything
    // the pass at the end would apply, so that pass puts them back
    // afterwards rather than writing over them.
    let mut settled: Vec<Settled> = Vec::new();
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
        // A brush layer: the strokes that were laid on it, in the order
        // they were laid. Each is gathered into a coverage of its own
        // first — its segments union rather than pile up — and then
        // comes down in its colour, or takes off what is under it.
        NodeKind::Paint { strokes } => {
            let Some(inv) = chitrakar_render::invert(t) else {
                return Some(());
            };
            let _ = inv;
            // The softest fade is still one device pixel wide, which is
            // the floor the CPU renderer puts under it.
            let band = 1.0 / t.max_scale().max(1e-6);
            for stroke in strokes {
                // A stroke laid inside a region carries that region, so
                // it stays confined after the region is let go of. It is
                // a coverage over the stroke's own box — one texture per
                // stroke rather than one per layer, which is what makes
                // it the stroke's rather than the layer's. One slot,
                // though: a layer's own mask is already riding it for
                // everything the layer draws, so a stroke that also
                // carries a region has nowhere to put it and that page
                // is the CPU's.
                let confined = match stroke.clip.as_deref() {
                    Some(region) => match stroke.bounds() {
                        Some(box_) => Some(clip_texture(doc, region, box_, t, held, out)?),
                        None => continue,
                    },
                    None => None,
                };
                let n = stroke.points.len();
                if n == 0 {
                    continue;
                }
                let colour = if stroke.erase {
                    // The colour of an eraser is nothing at all; what it
                    // hands down is the coverage, and the blend that
                    // brings it down takes that much off.
                    [0.0, 0.0, 0.0, 1.0]
                } else {
                    premultiplied_color(doc, &stroke.color, 1.0)
                };
                let segments = stroke_segments(stroke, t, band, out);
                if segments.is_empty() {
                    continue;
                }
                let quad = out.push(page_quad(
                    (out.page, out.surface),
                    alpha,
                    [0.0; 4],
                    colour,
                    [0.0; 3],
                ));
                let (at, box_) = confined.unwrap_or((None, NO_MASK));
                for v in &mut out.vertices[quad.start as usize..quad.end as usize] {
                    v.mask = box_;
                }
                if confined.is_some() {
                    settled.push(Settled {
                        vertices: quad.start as usize..quad.end as usize,
                        draw: out.draws.len(),
                        texture: at,
                        quad: box_,
                    });
                }
                out.draws.push(Item {
                    draw: Draw::Brush {
                        segments,
                        quad,
                        erase: stroke.erase,
                    },
                    mask: at,
                });
            }
        }
        // A clone layer: the same strokes, filled with what the surface
        // already holds a fixed distance away rather than with a colour.
        // Not on a surface of its own, unlike a brush layer — what it
        // paints with is what is under it, and on one of its own there
        // would be nothing under it to paint with.
        NodeKind::Clone { strokes } => {
            let band = 1.0 / t.max_scale().max(1e-6);
            for stroke in strokes {
                // Healing takes the texture from the source and the
                // colour from where it lands: the shift between what the
                // two average over the whole stroke, worked out before a
                // single pixel of it goes down. That is a reduction, and
                // a pass of quads is not where one happens.
                if stroke.heal {
                    return None;
                }
                let confined = match stroke.clip.as_deref() {
                    Some(region) => match stroke.bounds() {
                        Some(box_) => Some(clip_texture(doc, region, box_, t, held, out)?),
                        None => continue,
                    },
                    None => None,
                };
                let segments = stroke_segments(stroke, t, band, out);
                if segments.is_empty() {
                    continue;
                }
                // The offset is a direction rather than a place: the
                // source is written in the layer's own space, so where
                // it points is that space's to say and its shift is no
                // part of it — the CPU renderer's own carry.
                let (sx, sy) = (
                    t.a * stroke.source[0] + t.c * stroke.source[1],
                    t.b * stroke.source[0] + t.d * stroke.source[1],
                );
                let quad = out.push(page_quad(
                    (out.page, out.surface),
                    alpha,
                    [blend_index(node.blend) as f32, sx, sy, 0.0],
                    [0.0; 4],
                    [0.0; 3],
                ));
                let (at, box_) = confined.unwrap_or((None, NO_MASK));
                for v in &mut out.vertices[quad.start as usize..quad.end as usize] {
                    v.mask = box_;
                }
                if confined.is_some() {
                    settled.push(Settled {
                        vertices: quad.start as usize..quad.end as usize,
                        draw: out.draws.len(),
                        texture: at,
                        quad: box_,
                    });
                }
                out.draws.push(Item {
                    draw: Draw::Clone { segments, quad },
                    mask: at,
                });
            }
        }
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
            // The quad is the image's own box, grown a device pixel on
            // every side; its local coordinates are the texture's, so
            // the vertex shader passes them straight through as texture
            // coordinates. The grown skirt is what lets the fragment
            // shader fade the border by the area the box really covers
            // — outside 0..1 it reads as no coverage at all.
            let mut verts = quad(t, size, [0.0; 4], [0.0, 0.0, 0.0, alpha], [0.0; 4], 1.0);
            for v in &mut verts {
                v.local = [v.local[0] / size[0], v.local[1] / size[1]];
            }
            let quad = out.push(verts);
            out.draws.push(Item::of(Draw::Image { quad, texture: at }));
        }
        NodeKind::Text(spec) => {
            let color = premultiplied_color(doc, &spec.fill, alpha);
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
            let quad = out.push(page_quad(
                (out.page, out.surface),
                alpha,
                plan.params,
                plan.grad,
                plan.extra,
            ));
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
                Filtering::Pointwise(params, grad, extra) => {
                    let quad = out.push(page_quad(
                        (out.page, out.surface),
                        alpha,
                        params,
                        grad,
                        extra,
                    ));
                    out.draws.push(Item::of(Draw::Adjust { quad, table: None }));
                }
                Filtering::Blur { radius, sharpen } => {
                    let on = (out.page, out.surface);
                    let axis =
                        |a: f32| page_quad(on, 1.0, [radius, a, 0.0, 0.0], [0.0; 4], [0.0; 3]);
                    let mut steps = out.push(axis(0.0));
                    steps.end = out.push(axis(1.0)).end;
                    let quad = out.push(page_quad(
                        (out.page, out.surface),
                        alpha,
                        [sharpen, 0.0, 0.0, 0.0],
                        [0.0; 4],
                        [0.0; 3],
                    ));
                    out.draws.push(Item::of(Draw::Blur { steps, quad }));
                }
                Filtering::Blocks { across, down } => {
                    let mut steps = out.push(page_quad(
                        (out.page, out.surface),
                        1.0,
                        across,
                        [0.0; 4],
                        [0.0; 3],
                    ));
                    steps.end = out
                        .push(page_quad(
                            (out.page, out.surface),
                            1.0,
                            down,
                            [0.0; 4],
                            [0.0; 3],
                        ))
                        .end;
                    // Laid down the way a blur is: nothing is added back,
                    // so the amount that makes a sharpen is zero.
                    let quad = out.push(page_quad(
                        (out.page, out.surface),
                        alpha,
                        [0.0; 4],
                        [0.0; 4],
                        [0.0; 3],
                    ));
                    out.draws.push(Item::of(Draw::Blocks { steps, quad }));
                }
                Filtering::Smear { taps, step } => {
                    let along = out.push(page_quad(
                        (out.page, out.surface),
                        1.0,
                        [taps, step[0], step[1], 0.0],
                        [0.0; 4],
                        [0.0; 3],
                    ));
                    // Laid down the way a blur is: what was under it,
                    // mixed with the smeared copy by the layer's opacity
                    // and its mask. Nothing is added back, so the amount
                    // that makes a sharpen out of a blur is zero here.
                    let quad = out.push(page_quad(
                        (out.page, out.surface),
                        alpha,
                        [0.0; 4],
                        [0.0; 4],
                        [0.0; 3],
                    ));
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
            if !upright || node.opacity < 1.0 || node.mask.is_some() {
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
            let (pw, ph) = out.surface;
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
            //
            // Unless the frame wears an effect, and then there is: the
            // surface it was drawn into has to be brought down again,
            // and that is below, past the end of this match. Returning
            // here used to skip it, so a frame with a shadow opened a
            // surface nothing ever closed — no shadow anywhere, and the
            // frame's own pixels wrong besides. It was declined rather
            // than drawn, so nothing was ever wrong on a page; but the
            // reason written down for declining it was not this one.
            if !alone {
                return Some(());
            }
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
            let master = doc.node(*of).ok()?;
            let back = chitrakar_render::invert(master.transform)?;
            let stand_ins = if chitrakar_render::takes_stand_ins(doc, *of) {
                chitrakar_render::copy_children(doc, child).ok()?
            } else {
                Vec::new()
            };
            // On a surface of its own the fade and the cut belong to the
            // quad that brings the surface back, so what is drawn into it
            // is drawn whole and unbounded — which is what the group arm
            // above does for the same reason.
            let inner = if alone { 1.0 } else { opacity };
            let within = if alone { None } else { bound };
            if stand_ins.is_empty() {
                one(doc, *of, t.compose(back), inner, within, false, out)?;
            } else {
                // What a copy with stand-ins draws is a list of layers
                // rather than the group itself, and a group holding an
                // adjustment, a filter, a clone layer or a blend is
                // isolated so that what is inside it changes only what
                // is inside it. The CPU renderer puts such a copy on a
                // surface of its own; this pass draws straight in, which
                // is a different picture, so the page goes back.
                if chitrakar_render::any_reads_backdrop(doc, &stand_ins).ok()? {
                    return None;
                }
                for part in stand_ins {
                    one(doc, part, t, inner, within, false, out)?;
                }
            }
        } // No arm left over, and none wanted: every kind of layer is
          // drawn here now, so a new one will not compile until it says
          // how — which is the same bargain `Node::each_color_mut` makes.
          // What a layer is still handed back for is a thing it holds
          // rather than the kind it is: a healing stroke, which is an
          // average over the whole stroke before any of it goes down,
          // and a band wider than a pass will walk.
    }
    // Where the quads that lay the surface down start, so the mask can
    // be kept off them: a layer with effects wears its mask on its own
    // drawing rather than on the way down, since a mask decides what the
    // silhouette is and so what the shadow is a shadow of.
    let mut laid = None;
    // A frame above cuts what the layer *lays down*, and not the
    // silhouette its effects grew from: the CPU renderer builds a field
    // over a window grown past the frame's edge by the effect's own
    // reach and then writes only inside the frame, so a shadow ends at
    // the frame while the shape it is a shadow of does not. Whole
    // pixels, so the cut can be the rectangle the quads are drawn over
    // rather than a coverage they read — which leaves the mask texture
    // for the mask, and leaves what was drawn into the surface uncut.
    let framed = match bound {
        Some(b) if !shadings.is_empty() => Some(out.page.intersect(b)),
        _ => None,
    };
    let down = framed.unwrap_or(out.page);
    if alone {
        if shadings.is_empty() {
            // The mask and the opacity go on the quad that lays the
            // surface down, not on what was drawn into it.
            mark = (out.vertices.len(), out.draws.len());
        } else {
            laid = Some((out.vertices.len(), out.draws.len()));
        }
        // Each effect first: the field over the whole surface, since the
        // silhouette it is built from is wherever the layer is, then the
        // pair of axis quads the box passes alternate between, then the
        // quad that stamps the blurred field down over the page.
        let whole = chitrakar_render::ClipRect {
            x0: 0,
            y0: 0,
            x1: out.surface.0,
            y1: out.surface.1,
        };
        let effects: Vec<Painted> = shadings
            .iter()
            .map(|s| {
                let on = (whole, out.surface);
                // A band is measured from a yes or a no rather than
                // from a coverage, which is what the 2 asks the field
                // pass for and what the second number is the threshold
                // of; everything else is built as a coverage, taken
                // from the layer or from the hole around it.
                let said = match s.spread {
                    Spread::Band { inside, .. } => [2.0, inside, 0.0, 0.0],
                    _ => [if s.invert { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
                };
                let field = out.push(page_quad(on, owed, said, s.tint, [0.0; 3]));
                let mut axes = |first: [f32; 4], second: [f32; 4], tint: [f32; 4]| {
                    let mut steps = out.push(page_quad(on, 1.0, first, [0.0; 4], [0.0; 3]));
                    steps.end = out.push(page_quad(on, 1.0, second, tint, [0.0; 3])).end;
                    steps
                };
                let steps = match s.spread {
                    // The 1 says the box passes find nothing past the
                    // end of a line rather than its edge repeated: a
                    // field is nothing past the window it was built
                    // over, and repeating its edge where the surface cut
                    // that window short would invent silhouette.
                    Spread::Blurred(radius) => {
                        axes([radius, 0.0, 1.0, 0.0], [radius, 1.0, 1.0, 0.0], [0.0; 4])
                    }
                    // The tint rides the second pass, which is the one
                    // that has a distance to cut to a width and so the
                    // first that has a colour to say.
                    Spread::Band { width, .. } => {
                        axes([width, 0.0, 0.0, 0.0], [width, 1.0, 0.0, 0.0], s.tint)
                    }
                    Spread::Still => 0..0,
                };
                // The offset and the hold-inside are the stamp's own
                // work, unless the layer carries a blend mode: then the
                // stamp needs what is under it where those want the
                // layer's coverage, so they go on a pass of their own
                // beforehand and the stamp is left an ordinary picture
                // to bring down.
                let carried = [
                    s.offset[0],
                    s.offset[1],
                    if s.inside { 1.0 } else { 0.0 },
                    owed,
                ];
                let blended = node.blend != BlendMode::Normal;
                let settle = if blended {
                    out.push(page_quad(on, 1.0, carried, [0.0; 4], [0.0; 3]))
                } else {
                    0..0
                };
                let quad = out.push(page_quad(
                    (down, out.surface),
                    opacity,
                    if blended {
                        [blend_index(node.blend) as f32, 0.0, 0.0, 0.0]
                    } else {
                        carried
                    },
                    [0.0; 4],
                    [0.0; 3],
                ));
                Painted {
                    over: s.over,
                    inside: s.inside,
                    band: matches!(s.spread, Spread::Band { .. }),
                    settle,
                    field,
                    steps,
                    quad,
                }
            })
            .collect();
        let quad = out.push(page_quad(
            (down, out.surface),
            if shadings.is_empty() || !in_surface {
                node.opacity * opacity
            } else {
                // Its own opacity is already in the surface; what is
                // left is whatever it inherited, which weighs the layer
                // and the effects around it alike.
                opacity
            },
            [blend_index(node.blend) as f32, 0.0, 0.0, 0.0],
            [0.0; 4],
            [0.0; 3],
        ));
        out.draws.push(Item::of(Draw::Close {
            quad,
            blend: node.blend,
            effects,
        }));
    }
    // The mask, once, over everything the layer drew: the CPU
    // renderer rasterizes the coverage and the fragments read it.
    // What a layer is held to rides the same texture — two
    // coverages a layer is held back by are one coverage, and a
    // fragment reads it once.
    let coverage = if framed.is_some() { None } else { bound };
    if node.mask.is_some() || held_to.is_some() || coverage.is_some() {
        let (at, box_) = mask_texture(
            doc,
            child,
            node.mask.as_ref(),
            held_to,
            coverage,
            parent,
            out,
        )?;
        for v in &mut out.vertices[mark.0..] {
            v.mask = box_;
        }
        for item in &mut out.draws[mark.1..] {
            item.mask = at;
        }
        // A stroke carrying a region of its own already holds all of
        // this, folded into that region when it was built. Putting those
        // back is what lets one slot answer two questions.
        for own in &settled {
            for v in &mut out.vertices[own.vertices.clone()] {
                v.mask = own.quad;
            }
            out.draws[own.draw].mask = own.texture;
        }
        // The surface already holds the mask; laying it down is not the
        // place to take the coverage a second time.
        //
        // Not *all* of the coverage, though, and that distinction is the
        // whole of a defect. A layer's own mask belongs to its drawing:
        // the effects are built from the silhouette the mask left, and a
        // shadow of a masked shape falls outside the mask, as it should.
        // What a layer is **held to** is the other thing: it cuts what
        // the layer lays down, its effects with it — that is what a
        // clipping group means, and it is what the reference renderer
        // does. Riding the same slot, the two were folded into the one
        // texture and then cleared together here, so a layer held to the
        // one below it drew its outline outside that layer, where the
        // reference renderer draws none. The same slot carries a frame's
        // bound, which leaked the same way.
        //
        // So the lay-down keeps a coverage of its own, built from what
        // holds it back and not from its mask.
        if let Some((from, draws)) = laid {
            let (over, box_over) = if held_to.is_some() || coverage.is_some() {
                mask_texture(doc, child, None, held_to, coverage, parent, out)?
            } else {
                (None, NO_MASK)
            };
            for v in &mut out.vertices[from..] {
                v.mask = box_over;
            }
            for item in &mut out.draws[draws..] {
                item.mask = over;
            }
        }
    }
    Some(())
}

/// A stroke's segments as quads: the round-capped bands the brush shader
/// turns into a coverage, gathered with max blending because the
/// segments of one stroke union rather than pile up.
///
/// A brush layer and a clone layer lay the same shape and differ only in
/// what fills it, so they read this the same way.
fn stroke_segments(
    stroke: &chitrakar_doc::PaintStroke,
    t: Transform,
    band: f32,
    out: &mut Scene,
) -> std::ops::Range<u32> {
    let n = stroke.points.len();
    let start = out.vertices.len() as u32;
    if n == 0 {
        return start..start;
    }
    for i in 0..n.saturating_sub(1).max(1) {
        let j = (i + 1).min(n - 1);
        let (a, b) = (stroke.points[i], stroke.points[j]);
        let (ra, rb) = (stroke.radius(i), stroke.radius(j));
        let reach = ra.max(rb);
        if reach <= 0.0 {
            continue;
        }
        let box_ = [
            a[0].min(b[0]) - reach,
            a[1].min(b[1]) - reach,
            a[0].max(b[0]) + reach,
            a[1].max(b[1]) + reach,
        ];
        let at = t.compose(Transform::translation(box_[0], box_[1]));
        let mut verts = quad(
            at,
            [box_[2] - box_[0], box_[3] - box_[1]],
            [a[0], a[1], b[0], b[1]],
            [0.0; 4],
            [ra, rb, stroke.softness.clamp(0.0, 1.0), band],
            0.0,
        );
        // The quad is placed at the segment's corner, so its local
        // coordinate starts there; the segment is written in the layer's
        // own space, and both have to be read in the same one.
        for v in &mut verts {
            v.local = [v.local[0] + box_[0], v.local[1] + box_[1]];
        }
        out.vertices.extend(verts);
    }
    start..out.vertices.len() as u32
}

/// The region a brush stroke was laid inside, rasterized over the box
/// the stroke covers.
///
/// A stroke carries its region rather than reading one off the document,
/// so that letting the region go does not let the stroke spill — which
/// is also why the coverage is the stroke's and not the layer's: one
/// texture per stroke. `box_` is the stroke's own bounds in the layer's
/// space, and `t` is the space the layer is being drawn in, which is
/// where the CPU renderer rasterizes the same region.
fn clip_texture(
    doc: &Document,
    region: &chitrakar_doc::Mask,
    box_: [f32; 4],
    t: Transform,
    layer: Held<'_>,
    out: &mut Scene,
) -> Option<(Option<usize>, [f32; 4])> {
    let page = out.surface;
    let chitrakar_render::Bounds::Rect(bx0, by0, bx1, by1) =
        chitrakar_render::transformed_box(t, box_)
    else {
        return Some((None, NO_MASK));
    };
    // A pixel of margin, as a layer's own coverage takes: the quads are
    // grown by a device pixel so an edge is not cut short.
    let x0 = (bx0.floor() as i64 - 1).clamp(0, page.0 as i64) as u32;
    let y0 = (by0.floor() as i64 - 1).clamp(0, page.1 as i64) as u32;
    let x1 = (bx1.ceil() as i64 + 1).clamp(0, page.0 as i64) as u32;
    let y1 = (by1.ceil() as i64 + 1).clamp(0, page.1 as i64) as u32;
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w == 0 || h == 0 {
        // None of the stroke is on the page, so there is nothing for the
        // region to hold back either.
        return Some((None, NO_MASK));
    }
    let clip = chitrakar_render::ClipRect { x0, y0, x1, y1 };
    let mut cover = chitrakar_render::mask_plane_over(doc, region, t, clip, page);
    // And whatever the layer as a whole is held back by, over the same
    // pixels: one slot holds one coverage, so the stroke's region and the
    // layer's own become one here rather than one displacing the other.
    // Multiplied, which is what the CPU renderer does with them — it asks
    // the region as the stroke goes down and the layer's coverage as the
    // layer does, and a coverage taken twice is a coverage multiplied.
    held_back(doc, &mut cover, (x0, y0, w), layer, clip, page)?;
    let at = out.textures.len();
    out.textures.push(Image {
        width: w,
        height: h,
        channels: 1,
        texels: cover.iter().map(|c| f32_to_f16(*c)).collect(),
    });
    Some((Some(at), [x0 as f32, y0 as f32, w as f32, h as f32]))
}

/// A stroke that carries a region of its own, and so already holds the
/// whole of what its layer is held back by: where its quad's vertices and
/// its draw landed, and the coverage they were given.
struct Settled {
    vertices: std::ops::Range<usize>,
    draw: usize,
    texture: Option<usize>,
    quad: [f32; 4],
}

/// What a layer as a whole is held back by: its own mask, the layer it is
/// held to, and the frame it is inside. Carried together because they are
/// read together — by the time a fragment sees them they are one number.
#[derive(Clone, Copy)]
struct Held<'a> {
    mask: Option<&'a chitrakar_doc::Mask>,
    to: Option<NodeId>,
    bound: Option<chitrakar_render::ClipRect>,
    /// The space the mask is authored in, which is the layer's parent's.
    parent: Transform,
}

/// Multiply a coverage plane, laid over the device pixels `(x0, y0)` to
/// `(x0 + w, ..)`, by everything `layer` is held back by.
///
/// The one place that folds the three together, so a coverage built for a
/// layer and a coverage built for one of its strokes cannot come to
/// disagree about what "held back" means.
fn held_back(
    doc: &Document,
    cover: &mut [f32],
    at: (u32, u32, u32),
    layer: Held<'_>,
    clip: chitrakar_render::ClipRect,
    page: (u32, u32),
) -> Option<()> {
    let (x0, y0, w) = at;
    if let Some(mask) = layer.mask {
        let plane = chitrakar_render::mask_plane_over(doc, mask, layer.parent, clip, page);
        for (c, m) in cover.iter_mut().zip(plane) {
            *c *= m;
        }
    }
    if let Some(base) = layer.to {
        let held = chitrakar_render::layer_coverage_at(doc, base, layer.parent, page).ok()?;
        for (i, c) in cover.iter_mut().enumerate() {
            let (x, y) = (x0 + i as u32 % w, y0 + i as u32 / w);
            *c *= held[(y * page.0 + x) as usize];
        }
    }
    // A frame somewhere above: everything drawn inside one is held to its
    // rectangle, which is whole pixels, so this takes all of a pixel or
    // none of it.
    if let Some(inside) = layer.bound {
        for (i, c) in cover.iter_mut().enumerate() {
            let (x, y) = (x0 + i as u32 % w, y0 + i as u32 / w);
            if x < inside.x0 || x >= inside.x1 || y < inside.y0 || y >= inside.y1 {
                *c = 0.0;
            }
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
    // The surface, not the page: what a coverage plane is indexed by is
    // the thing being drawn on.
    let page = out.surface;
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
    //
    // And as far as the layer's *effects* reach, which is further. A
    // coverage is read off this texture and a fragment outside it is
    // read as uncovered-by-nothing — let through — so an outline or a
    // shadow standing beyond the layer's own box was held back by
    // nothing at all. A layer held to the one below it drew its outline
    // outside that layer, where the reference renderer draws none: the
    // clip cut the layer and let its effects escape. The same texture
    // carries a mask and a frame's bound, so all three leaked the same
    // way and for the same reason.
    let reach = doc
        .node(id)
        .ok()?
        .effects
        .iter()
        .map(chitrakar_doc::Effect::reach)
        .fold(0.0f32, f32::max);
    let pad = 1 + (reach * parent.max_scale()).ceil().max(0.0) as i64;
    let x0 = (bx0.floor() as i64 - pad).clamp(0, page.0 as i64) as u32;
    let y0 = (by0.floor() as i64 - pad).clamp(0, page.1 as i64) as u32;
    let x1 = (bx1.ceil() as i64 + pad).clamp(0, page.0 as i64) as u32;
    let y1 = (by1.ceil() as i64 + pad).clamp(0, page.1 as i64) as u32;
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
    let mut cover = vec![1.0; (w * h) as usize];
    held_back(
        doc,
        &mut cover,
        (x0, y0, w),
        Held {
            mask,
            to: held_to,
            bound,
            parent,
        },
        clip,
        page,
    )?;
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
            let (ramp, geom, radial) = bake(doc, g);
            let at = out.textures.len();
            out.textures.push(ramp);
            let kind = if radial { 1.0 } else { 0.0 };
            Some(([kind, 0.0, 0.0, alpha], geom, Some(at)))
        }
        None => fill.map(|c| (premultiplied_color(doc, &c, alpha), [0.0; 4], None)),
    };
    let ink = match stroke {
        Some(s) if s.width > 0.0 => Some((premultiplied_color(doc, &s.color, alpha), s)),
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
/// layer's opacity.
///
/// Ink authored for a press resolves through the document's press profile
/// — and that resolution is a colour at a time, on the CPU, in both
/// renderers: `chitrakar_render::resolve_color` is the one that answers,
/// so the two cannot drift. A colour standing for a swatch is whatever
/// that swatch means, which that function reads past the name to find.
fn premultiplied_color(
    doc: &Document,
    color: &chitrakar_color::AuthoredColor,
    alpha: f32,
) -> [f32; 4] {
    let c = chitrakar_render::resolve_color(doc, color);
    [c.r * alpha, c.g * alpha, c.b * alpha, c.a * alpha]
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
/// A stop authored for a press resolves through the document's profile,
/// the same colour at a time the CPU renderer resolves it. The caller has
/// already ruled out a gradient with no stops.
fn bake(doc: &Document, g: &chitrakar_doc::Gradient) -> (Image, [f32; 4], bool) {
    let mut stops = Vec::with_capacity(g.stops().len());
    for stop in g.stops() {
        stops.push((
            stop.offset,
            chitrakar_render::resolve_color(doc, &stop.color),
        ));
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
    (
        Image {
            width: RAMP,
            height: 1,
            channels: 4,
            texels,
        },
        geom,
        radial,
    )
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
/// shader reads them: which adjustment it is, then up to seven numbers,
/// and for the three stated by more than numbers a table baked here from
/// what the CPU renderer prepares.
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
    let boxes = |sigma: f32| box_radius(sigma * scale);
    // Where a page pixel is in the space the layer lives in, which is
    // the question both pointwise filters ask and which the page pixel
    // itself only answers while the view is the identity. Under a view,
    // or inside a copy — which draws what it copies somewhere else
    // entirely — the two part company, and a grain anchored to the
    // surface stops being anchored to the picture.
    //
    // The inverse travels rather than the transform, and it travels as
    // `Inverse::of` computes it and is read as `Inverse::at` reads it,
    // arithmetic for arithmetic: a grain cell is a `floor`, and a last
    // bit that rounds the other way puts a whole speck in the next cell.
    let det = view.a * view.d - view.b * view.c;
    if det.abs() < 1e-9 {
        // A transform that collapses space maps every device pixel
        // nowhere, and the CPU renderer returns without touching the
        // page rather than guessing.
        return Some(Filtering::Nothing);
    }
    let back = [view.d / det, -view.b / det, -view.c / det, view.a / det];
    let from = [view.e, view.f];
    let point = |params: [f32; 4], third: f32| {
        Filtering::Pointwise(params, back, [from[0], from[1], third])
    };
    Some(match filter {
        F::Vignette {
            amount,
            radius,
            softness,
        } => point([14.0, *amount, *radius, *softness], 0.0),
        F::Noise {
            amount,
            grain,
            mono,
            seed,
        } => point(
            // Mono is the *kind* rather than a flag: the quad's four
            // numbers are spoken for by the inverse above, and a page's
            // grain being one colour or three is as much a different
            // filter as a vignette is. A seed is a whole 32 bits and a
            // vertex carries floats, so it travels as two halves that a
            // float holds exactly.
            [
                if *mono { 15.0 } else { 16.0 },
                *amount,
                *grain,
                (seed >> 16) as f32,
            ],
            (seed & 0xffff) as f32,
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

/// A live effect's quads: the one that builds the field from the
/// layer's silhouette, the pair the box passes alternate between, and
/// the one that stamps the blurred field down.
#[derive(Clone)]
struct Painted {
    over: bool,
    inside: bool,
    /// A band's two distance passes rather than a blur's twelve.
    band: bool,
    /// The pass that reads the field at its offset and holds it inside
    /// the silhouette before the stamp, which is what leaves the stamp
    /// nothing to do but bring a picture down by a blend mode. Empty
    /// unless the layer carries one.
    settle: std::ops::Range<u32>,
    field: std::ops::Range<u32>,
    /// Empty when the field is not carried out from the silhouette at
    /// all, in which case it goes down as it was built.
    steps: std::ops::Range<u32>,
    quad: std::ops::Range<u32>,
}

/// A live effect ready to draw: the field to build from the layer's
/// silhouette, blurred, and stamped back down.
#[derive(Clone)]
struct Shading {
    /// Painted over the layer rather than behind it — an inner shadow
    /// shades the pixels it sits on, so it cannot go down before they
    /// are there.
    over: bool,
    /// Built from the hole around the layer instead of the layer.
    invert: bool,
    /// Held to the layer's own coverage on the way down, which is what
    /// keeps an inner shadow inside the silhouette.
    inside: bool,
    /// Premultiplied, already weighed by the effect's own opacity.
    tint: [f32; 4],
    /// How the field is carried out from the silhouette it was built on.
    spread: Spread,
    /// In device pixels, which is where the layer's parent space has
    /// already carried it.
    offset: [f32; 2],
}

/// How a field is carried out from the silhouette it was built on.
#[derive(Clone, Copy)]
enum Spread {
    /// Not carried out at all: a blur too small to move a pixel, or an
    /// effect asked for nothing. The field goes down as it was built.
    Still,
    /// Three box passes each way, which is the CPU renderer's Gaussian.
    Blurred(f32),
    /// Measured out to a true distance and cut at a width: an outline's
    /// band. Two passes, since the exact Euclidean transform separates —
    /// one down each column, one along each row. `inside` is the
    /// coverage at which a pixel counts as part of the silhouette.
    Band { width: f32, inside: f32 },
}

/// What a live effect turns into here, or `None` when it stays the
/// CPU's.
fn effect_of(
    doc: &Document,
    effect: &chitrakar_doc::Effect,
    parent: Transform,
    layer_opacity: f32,
) -> Option<Shading> {
    use chitrakar_doc::Effect as E;
    let scale = parent.max_scale();
    // An offset is a vector in the layer's parent space, so where it
    // points is that space's to say — the same carry the CPU renderer
    // makes before it stamps.
    let along = |dx: f32, dy: f32| [parent.a * dx + parent.c * dy, parent.b * dx + parent.d * dy];
    // Ink authored for a press resolves through the document's profile,
    // which `premultiplied_color` is the one place that asks — the same
    // place, and the same answer, the CPU renderer's own tints come from.
    let tinted = |color: &chitrakar_color::AuthoredColor, opacity: f32| {
        premultiplied_color(doc, color, opacity)
    };
    match effect {
        E::DropShadow {
            dx,
            dy,
            blur,
            color,
            opacity,
        }
        | E::InnerShadow {
            dx,
            dy,
            blur,
            color,
            opacity,
        } => {
            let inner = matches!(effect, E::InnerShadow { .. });
            Some(Shading {
                over: inner,
                invert: inner,
                inside: inner,
                tint: tinted(color, *opacity),
                spread: match box_radius(blur * scale) {
                    Some(radius) => Spread::Blurred(radius),
                    None => Spread::Still,
                },
                offset: along(*dx, *dy),
            })
        }
        // A band hugging the silhouette from outside, whose width is a
        // true distance rather than a blur. The exact Euclidean
        // transform separates — a pass down each column for how far the
        // nearest inside pixel in it is, then a pass along each row
        // taking the least of `dx² + g²` — so the band is two passes
        // here and the same distance the CPU renderer measures, rather
        // than a different band arrived at faster.
        E::Outline {
            width,
            color,
            opacity,
        } => {
            let w = width * scale;
            // Asked for nothing: the CPU renderer draws no band at all
            // for either of these, and an empty tint says so without a
            // pass arrangement of its own.
            if *opacity <= 0.0 || w <= 0.0 {
                return Some(Shading {
                    over: false,
                    invert: false,
                    inside: false,
                    tint: [0.0; 4],
                    spread: Spread::Still,
                    offset: [0.0; 2],
                });
            }
            // Both passes walk out as far as the band reaches, and a
            // band wider than this is more of a walk per pixel than a
            // pass should be — the same cap, in taps, that a smear is
            // held to.
            if w + 1.0 > BAND_MOST {
                return None;
            }
            // The CPU renderer builds the band inside the layer's box
            // grown by the effect's reach — width + 2 in the layer's
            // own units, which is 2·scale device pixels past where the
            // band ends. Under half a device pixel to the unit that
            // slack is gone and it cuts the band's outer fringe where
            // this, drawing over the whole surface, would not.
            if scale < 0.5 {
                return None;
            }
            Some(Shading {
                over: false,
                invert: false,
                inside: false,
                tint: tinted(color, *opacity),
                // Half covered is inside. The layer's own opacity is
                // already in the surface, so half of *that* is where
                // its edge is: a layer at a third opacity would
                // otherwise have no inside at all, and cast no outline.
                spread: Spread::Band {
                    width: w,
                    inside: 0.5 * layer_opacity.max(1e-3),
                },
                offset: [0.0; 2],
            })
        }
    }
}

/// The widest band, in device pixels, either pass will walk: 129 taps
/// out from the pixel, which is the count a smear is capped at.
const BAND_MOST: f32 = 128.0;

/// The W3C's box size for a Gaussian after three passes each way, read
/// off the CPU renderer so the two blur by the same amount. Nothing when
/// the blur is too small to move a pixel, which the CPU renderer returns
/// from without touching anything.
fn box_radius(sigma: f32) -> Option<f32> {
    (sigma > 0.01).then(|| {
        let d = ((sigma * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0) + 0.5).floor() as i32;
        (d.max(1) / 2).max(1) as f32
    })
}

/// What a filter layer turns into here.
enum Filtering {
    /// A function of one pixel and of where it is: the quad says which
    /// filter, what it was asked for, and *where a page pixel is in the
    /// layer's own space* — which is the question these two ask and the
    /// one that used to be answered with the page pixel itself.
    Pointwise([f32; 4], [f32; 4], [f32; 3]),
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
    /// One live effect of the layer drawn on surface `from`: the field
    /// built from its silhouette and blurred by the time this opens the
    /// pass, and the quad that stamps the result down.
    Effect {
        from: usize,
        at: Painted,
        mask: Option<usize>,
        /// The layer's own blend mode, which the CPU renderer brings
        /// the effect down by as well as the layer.
        blend: BlendMode,
    },
    /// One brush stroke. Its segments have been gathered into a coverage
    /// on the scratch texture by the time this opens the pass; what is
    /// left is the quad that lays that coverage down in the stroke's
    /// colour.
    Brush {
        segments: std::ops::Range<u32>,
        quad: std::ops::Range<u32>,
        erase: bool,
        /// The region the stroke was laid inside, if it carries one.
        mask: Option<usize>,
    },
    /// One clone stroke. Its segments have been gathered into a coverage
    /// on the scratch texture by the time this opens the pass; what is
    /// left is the quad that fills that coverage with what the surface
    /// already holds a fixed distance away.
    Clone {
        segments: std::ops::Range<u32>,
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
            // An effect reads a texture of its own rather than what is
            // under it — unless it is brought down by a blend mode,
            // which is a question about what is under it by definition.
            Opening::Effect { blend, .. } => *blend != BlendMode::Normal,
            Opening::Brush { .. } => false,
            // What a clone lifts and what it lands on are both what was
            // already there, which is the one copy.
            Opening::Clone { .. } => true,
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
            | Draw::Blocks { .. }
            | Draw::Brush { .. }
            | Draw::Clone { .. } => {}
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
            Draw::Close {
                quad,
                blend,
                effects,
            } => {
                let from = stack.pop().unwrap_or(0);
                clear = false;
                if effects.is_empty() {
                    lay = Some(Opening::Lay {
                        from,
                        quad: quad.clone(),
                        mask: item.mask,
                        blend: *blend,
                    });
                } else {
                    // A pass each, in the order the CPU renderer draws
                    // them: what is behind the layer, the layer, then
                    // what is over it. Each reads the surface the layer
                    // was drawn on, so none of them can be the pass that
                    // is drawing onto it.
                    let target = *stack.last().unwrap();
                    let painted = |at: &Painted| Pass {
                        target,
                        clear: false,
                        lay: Some(Opening::Effect {
                            from,
                            at: at.clone(),
                            mask: item.mask,
                            blend: *blend,
                        }),
                        items: i..i,
                    };
                    for effect in effects.iter().filter(|e| !e.over) {
                        passes.push(painted(effect));
                    }
                    passes.push(Pass {
                        target,
                        clear: false,
                        lay: Some(Opening::Lay {
                            from,
                            quad: quad.clone(),
                            mask: item.mask,
                            blend: *blend,
                        }),
                        items: i..i,
                    });
                    for effect in effects.iter().filter(|e| e.over) {
                        passes.push(painted(effect));
                    }
                    lay = None;
                }
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
            Draw::Brush {
                segments,
                quad,
                erase,
            } => {
                clear = false;
                lay = Some(Opening::Brush {
                    segments: segments.clone(),
                    quad: quad.clone(),
                    erase: *erase,
                    mask: item.mask,
                });
            }
            Draw::Clone { segments, quad } => {
                clear = false;
                lay = Some(Opening::Clone {
                    segments: segments.clone(),
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
    on: (chitrakar_render::ClipRect, (u32, u32)),
    alpha: f32,
    params: [f32; 4],
    grad: [f32; 4],
    extra: [f32; 3],
) -> Vec<Vertex> {
    let (page, surface) = on;
    // Over the page rather than over the whole surface: what is beside
    // the page is not the page's to write, and an adjustment that
    // rewrote it would be painting where the CPU renderer never goes.
    // The texture coordinate is still the surface's, since that is what
    // these fragments sample.
    let (x0, y0) = (page.x0 as f32, page.y0 as f32);
    let (x1, y1) = (page.x1 as f32, page.y1 as f32);
    let (sw, sh) = (surface.0 as f32, surface.1 as f32);
    let corner = |u: f32, v: f32| Vertex {
        doc: [x0 + u * (x1 - x0), y0 + v * (y1 - y0)],
        local: [
            (x0 + u * (x1 - x0)) / sw.max(1e-6),
            (y0 + v * (y1 - y0)) / sh.max(1e-6),
        ],
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

    /// Every layer of the fixture, asked for its own contribution.
    ///
    /// The audit below reads the finished page, and a finished page is
    /// the wrong place to look for a small layer. Nine of this fixture's
    /// twenty-six layers can be *removed outright* without moving the
    /// whole-page mean past the 0.004 it allows; the text block moves it
    /// by 0.00003, three orders of magnitude clear. The interior reading
    /// cannot see them either — it exists to tell a drawing apart from
    /// its antialiasing, and a layer thin enough is all edge. Drop the
    /// text from this backend altogether and the interiors read 0.0004
    /// with nothing over the threshold. So the whole of text rendering,
    /// on every audit this document has ever carried, was being compared
    /// on twenty-five antialiased pixels.
    ///
    /// What a layer *puts on the page* is the thing that cannot be
    /// diluted: render the page with it and without it on each backend,
    /// and the two differences are what that layer contributed. A layer
    /// the backend draws wrongly shows up at its own size rather than the
    /// page's, however small it is and however large the page grows.
    ///
    /// Asked of the bare fixture only — two renders a layer is not free —
    /// which is enough, because the commands below change the page and
    /// not the backend.
    #[test]
    fn every_layer_of_the_fixture_puts_down_what_the_cpu_puts_down() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut f = chitrakar_doc::fixture::everything();
        // As the audit below: what this backend hands a page back for is
        // taken off first, so that what is left is a page it draws. No
        // whole layer is on that list any more — only the effects below.
        let mut hung: Vec<(NodeId, Vec<chitrakar_doc::Effect>)> = f
            .doc
            .nodes()
            .filter(|(_, n)| !n.effects.is_empty())
            .map(|(id, n)| (*id, n.effects.clone()))
            .collect();
        hung.sort_by_key(|(id, _)| *id);
        for (id, _) in &hung {
            f.doc
                .apply(Command::SetEffects {
                    id: *id,
                    effects: Vec::new(),
                })
                .unwrap();
        }
        for (id, effects) in &hung {
            f.doc
                .apply(Command::SetEffects {
                    id: *id,
                    effects: effects.clone(),
                })
                .unwrap();
            if !GpuRenderer::can_render(&f.doc) {
                f.doc
                    .apply(Command::SetEffects {
                        id: *id,
                        effects: Vec::new(),
                    })
                    .unwrap();
            }
        }
        assert!(
            GpuRenderer::can_render(&f.doc),
            "the fixture has to be a page this backend draws"
        );
        let mine = gpu.render(&f.doc).unwrap();
        let theirs = chitrakar_render::render(&f.doc).unwrap();
        let mut ids: Vec<NodeId> = f.doc.nodes().map(|(i, _)| *i).collect();
        ids.sort_by_key(|i| i.0);
        let (mut asked, mut inked) = (0usize, 0usize);
        for id in ids {
            if f.doc.parent_of(id).is_none() {
                continue;
            }
            let mut without = f.doc.clone();
            if without
                .apply(Command::SetVisible { id, visible: false })
                .is_err()
            {
                continue;
            }
            if !GpuRenderer::can_render(&without) {
                continue;
            }
            let name = f.doc.node(id).unwrap().name.clone();
            let gone_mine = gpu.render(&without).unwrap();
            let gone_theirs = chitrakar_render::render(&without).unwrap();
            // What the layer put down, on each backend in turn.
            let (mut n, mut worst, mut drew) = (0usize, 0.0f32, 0usize);
            for i in 0..theirs.pixels.len() {
                let (a, b) = (&mine.pixels[i], &gone_mine.pixels[i]);
                let (c, d) = (&theirs.pixels[i], &gone_theirs.pixels[i]);
                let mut most = 0.0f32;
                let mut theirs_put = 0.0f32;
                for (u, v, y, z) in [
                    (a.r, b.r, c.r, d.r),
                    (a.g, b.g, c.g, d.g),
                    (a.b, b.b, c.b, d.b),
                    (a.a, b.a, c.a, d.a),
                ] {
                    let scale = (y - z).abs().max(1.0);
                    most = most.max(((u - v) - (y - z)).abs() / scale);
                    theirs_put = theirs_put.max((y - z).abs());
                }
                if theirs_put > 0.01 {
                    drew += 1;
                }
                if most > 0.1 {
                    n += 1;
                    worst = worst.max(most);
                }
            }
            asked += 1;
            if drew > 0 {
                inked += 1;
            }
            // Every pixel a layer touches is one of its own edges when the
            // layer is a glyph or a hairline, so an exact agreement is not
            // what is being asked for: what is, is that the two backends
            // put down the same layer. A tenth of the layer's own pixels
            // is wide enough for antialiasing to differ along an edge and
            // far too narrow for a layer to go missing.
            let allowed = (drew / 10).max(4);
            assert!(
                n <= allowed,
                "{name:?} is not the layer the reference puts down: {n} of \
                 the {drew} pixels it inks differ by more than a tenth, the \
                 worst by {worst:.4} ({allowed} allowed)"
            );
        }
        // A backend that drew nothing, or a fixture whose layers land off
        // the page, would pass every assertion above without being asked
        // anything at all.
        assert!(
            asked > 15 && inked > 15,
            "{asked} layers compared, {inked} of them drawing anything"
        );
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
        // The fixture holds one of every node kind and this backend now
        // draws every one of them, the paint layer included — press ink
        // and a stroke carrying a region inside a layer that wears a mask
        // were the last two things keeping a whole layer out of this
        // comparison, and neither does now. Nothing is removed here any
        // more, so every command that speaks to a paint layer is in scope
        // by itself.
        //
        // The effects the fixture hangs on layers do come off, for a
        // reason of a different shape. An effect is drawn from a layer's
        // silhouette, and this backend draws a shadow and an outline
        // that way now — what it still hands back is one on a blended
        // layer or inside a frame, and one of those anywhere declines
        // the whole page. So: every effect comes off, then each goes
        // back wherever the page is still accepted with it there. What
        // the backend can draw stays in the audit and what it cannot is
        // out, and which is which is *asked* rather than named — so the
        // comparison widens by itself as the backend learns another.
        // The commands that put effects *on* things are still asked too,
        // each against a copy of the document and declined by name,
        // which is the audit working rather than the audit blind.
        let mut hung: Vec<(NodeId, Vec<chitrakar_doc::Effect>)> = f
            .doc
            .nodes()
            .filter(|(_, n)| !n.effects.is_empty())
            .map(|(id, n)| (*id, n.effects.clone()))
            .collect();
        hung.sort_by_key(|(id, _)| *id);
        assert!(
            !hung.is_empty(),
            "the fixture still hangs effects on layers"
        );
        for (id, _) in &hung {
            f.doc
                .apply(Command::SetEffects {
                    id: *id,
                    effects: Vec::new(),
                })
                .unwrap();
        }
        let mut kept = 0usize;
        for (id, effects) in &hung {
            f.doc
                .apply(Command::SetEffects {
                    id: *id,
                    effects: effects.clone(),
                })
                .unwrap();
            if GpuRenderer::can_render(&f.doc) {
                kept += 1;
            } else {
                f.doc
                    .apply(Command::SetEffects {
                        id: *id,
                        effects: Vec::new(),
                    })
                    .unwrap();
            }
        }
        assert!(
            kept > 0,
            "the audit compares at least one layer with its effects on it"
        );
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
            // And a reading that a small layer cannot hide from. The
            // mean above is taken over the whole page, and most of this
            // fixture's layers are far too small to move it: nine of its
            // twenty-six can be *removed outright* and still come in
            // under 0.004 — the text layer by three orders of magnitude,
            // at 0.00003. Several of them are shapes put here precisely
            // to be compared: the clone layer, the held-to layer, the
            // adjustment inside a group, the copy's stand-in. A whole-page
            // average cannot see any of them.
            //
            // An interior pixel is one whose eight neighbours the
            // reference draws in its own colour, so it is not on an edge
            // and the two renderers' antialiasing is not being compared.
            // Away from an edge they should agree closely, so this is
            // the assertion that notices a small layer drawn wrongly —
            // and unlike the mean it does not get weaker as the fixture
            // grows.
            let (in_mean, in_worst, over, at) = interiors(&mine, &reference);
            assert!(
                over == 0,
                "after {what}: {over} interior pixels off by more than a \
                 fifth, the worst {in_worst:.4} at {at:?}"
            );
            assert!(
                in_mean < 0.002,
                "after {what}: interiors mean {in_mean:.5}, worst \
                 {in_worst:.4} at {at:?}"
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

        // A copy of a layer that is itself held to the one under *it* is
        // drawn whole: a copy draws the layer, not the layer's place in a
        // run of clipped ones, and the renderer being matched reaches it
        // through `render_layer`, where a clip run is the parent group's
        // business. Asked of the page, its far corner — nowhere near the
        // base the original is held to — carries the original's own
        // colour.
        {
            let mut apart = Document::new(60, 40, ColorMode::Rgb);
            add(
                &mut apart,
                filled(
                    "ground",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.85,
                        g: 0.8,
                        b: 0.2,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            add(
                &mut apart,
                filled("base", VectorShape::Ellipse { rx: 10.0, ry: 8.0 }, RED),
                Transform::translation(4.0, 4.0),
            );
            let one_held = add(
                &mut apart,
                filled(
                    "held",
                    VectorShape::Rect {
                        width: 24.0,
                        height: 18.0,
                        radius: 0.0,
                    },
                    BLUE,
                ),
                Transform::translation(8.0, 6.0),
            );
            apart
                .apply(Command::SetClipped {
                    id: one_held,
                    clipped: true,
                })
                .unwrap();
            add(
                &mut apart,
                Box::new(Node::instance("a copy", one_held)),
                Transform::translation(30.0, 18.0),
            );
            let reference = chitrakar_render::render(&apart).unwrap();
            let far = reference.get(52, 34);
            assert!(
                far.b > 0.5 && far.r < 0.2,
                "the copy is whole out there rather than cut to a base it is not near ({far:?})"
            );
            assert!(
                GpuRenderer::can_render(&apart),
                "and the backend draws it rather than handing the page back"
            );
            let (mean, worst) = difference(&gpu.render(&apart).unwrap(), &reference);
            assert!(
                mean < 0.004,
                "a copy of a held layer: mean {mean:.5} (worst {worst:.3})"
            );
        }

        // Faded, blended, masked or held to the layer under it, a copy
        // goes on a surface of its own and that surface is what lands —
        // the same three things that isolate a group, for the same
        // reason: what a copy draws may be a group whose children
        // overlap, and a coverage taken as each child lands would be
        // taken twice where two of them meet.
        for (what, dress) in [
            (
                "faded",
                Command::SetOpacity {
                    id: copy,
                    opacity: 0.5,
                },
            ),
            (
                "blended",
                Command::SetBlendMode {
                    id: copy,
                    blend: BlendMode::Multiply,
                },
            ),
            (
                "wearing a shadow",
                Command::SetEffects {
                    id: copy,
                    effects: vec![chitrakar_doc::Effect::DropShadow {
                        dx: 3.0,
                        dy: 2.0,
                        blur: 1.5,
                        color: BLUE,
                        opacity: 0.8,
                    }],
                },
            ),
            (
                "masked",
                Command::SetMask {
                    id: copy,
                    mask: Some(Box::new(chitrakar_doc::Mask {
                        kind: chitrakar_doc::MaskKind::Vector {
                            shape: VectorShape::Ellipse { rx: 9.0, ry: 7.0 },
                            transform: Transform::translation(2.0, 2.0),
                        },
                        invert: false,
                        feather: 0.0,
                    })),
                },
            ),
        ] {
            let mut dressed = doc.clone();
            dressed.apply(dress).unwrap();
            assert!(
                GpuRenderer::can_render(&dressed),
                "a copy {what} is drawn, not handed back"
            );
            let (mean, worst) = difference(
                &gpu.render(&dressed).unwrap(),
                &chitrakar_render::render(&dressed).unwrap(),
            );
            assert!(
                mean < 0.004,
                "a copy {what}: mean {mean:.5}, worst {worst:.3}"
            );
        }
    }

    /// A layer with effects inside a frame: the frame cuts what the
    /// layer lays down, and not the silhouette its effects grew from.
    ///
    /// These two are easy to get the wrong way round, and both readings
    /// look plausible on a page. The CPU renderer builds a field over a
    /// window grown past the frame's edge by the effect's own reach and
    /// then writes only inside the frame — so a shape half out of a
    /// frame casts the shadow of the whole shape, cut off at the
    /// frame's edge, rather than the shadow of the part that shows.
    /// Holding the layer to the frame on the way *in* would give the
    /// second, and a shadow with a straight edge down the middle of it
    /// is what that looks like.
    #[test]
    fn an_effect_inside_a_frame_ends_where_the_frame_does() {
        use chitrakar_doc::Effect as E;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |r: f32, g: f32, b: f32, a: f32| AuthoredColor::Srgb { r, g, b, a };
        let inside = |effects: Vec<E>| {
            let mut doc = Document::new(80, 60, ColorMode::Rgb);
            let root = doc.root();
            add(
                &mut doc,
                filled(
                    "back",
                    VectorShape::Rect {
                        width: 80.0,
                        height: 60.0,
                        radius: 0.0,
                    },
                    ink(0.86, 0.88, 0.9, 1.0),
                ),
                Transform::default(),
            );
            doc.apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::artboard("frame", 36.0, 30.0, Some(WHITE))),
            })
            .unwrap();
            let frame = doc.children_of(root).unwrap()[1];
            doc.apply(Command::SetTransform {
                id: frame,
                transform: Transform::translation(20.0, 15.0),
            })
            .unwrap();
            // Hung off the frame's right edge, so half the shape is
            // outside it and the effect has an edge to be cut at.
            doc.apply(Command::AddNode {
                parent: frame,
                index: 0,
                node: filled(
                    "shape",
                    VectorShape::Rect {
                        width: 20.0,
                        height: 12.0,
                        radius: 2.0,
                    },
                    ink(0.15, 0.2, 0.55, 1.0),
                ),
            })
            .unwrap();
            let shape = doc.children_of(frame).unwrap()[0];
            doc.apply(Command::SetTransform {
                id: shape,
                transform: Transform::translation(26.0, 9.0),
            })
            .unwrap();
            doc.apply(Command::SetEffects { id: shape, effects })
                .unwrap();
            doc
        };
        let shadow = E::DropShadow {
            dx: 4.0,
            dy: 4.0,
            blur: 1.5,
            color: ink(0.0, 0.0, 0.0, 1.0),
            opacity: 0.8,
        };
        let outline = E::Outline {
            width: 3.0,
            color: ink(0.95, 0.15, 0.1, 1.0),
            opacity: 1.0,
        };
        for (name, effects) in [
            ("a shadow in a frame", vec![shadow.clone()]),
            ("an outline in a frame", vec![outline.clone()]),
            (
                "an inner shadow in a frame",
                vec![E::InnerShadow {
                    dx: 2.0,
                    dy: 2.0,
                    blur: 1.0,
                    color: ink(0.0, 0.0, 0.1, 1.0),
                    opacity: 0.9,
                }],
            ),
            ("both at once", vec![shadow.clone(), outline.clone()]),
        ] {
            let doc = inside(effects);
            assert!(
                GpuRenderer::can_render(&doc),
                "{name} is drawn rather than handed back"
            );
            let (mean, worst) = difference(
                &gpu.render(&doc).unwrap(),
                &chitrakar_render::render(&doc).unwrap(),
            );
            assert!(mean < 0.004, "{name}: mean {mean:.5}, worst {worst:.3}");
        }

        // The readings a mean would hide. The frame runs from x = 20 to
        // x = 56; the shape sits from 46 to 66, so its right half and
        // everything its outline would put beyond the frame are outside.
        let drawn = gpu.render(&inside(vec![outline])).unwrap();
        let red = |x: u32, y: u32| {
            let p = drawn.get(x, y);
            p.r > p.b + 0.15
        };
        assert!(red(44, 30), "the band shows inside the frame");
        assert!(!red(57, 30), "and stops dead at the frame's edge");
        // And the band along the shape's top runs the whole way to that
        // edge — it is the whole shape's outline, cut, rather than the
        // outline of the part of the shape that shows.
        assert!(red(54, 22), "the band is the whole shape's, up to the cut");
    }

    /// An effect on a group, and on a brush layer: built from what the
    /// whole thing composites to, faded once.
    ///
    /// These two were out for the same reason, and it is a reason worth
    /// keeping straight. A layer that draws one thing has its opacity
    /// applied as it paints — the CPU renderer fades the fill and the
    /// stroke as they go down, so where they overlap the fade is taken
    /// twice and *that* is the silhouette. A group's opacity is not
    /// that: it belongs to the composite, so two overlapping children in
    /// a half-faded group make one half-faded shape with no seam down
    /// the overlap, and the shadow of it has no seam either. A brush
    /// layer is the same story — its strokes have their conversation
    /// with each other before any of it fades. So those two owe the
    /// silhouette their opacity at the moment it is built, rather than
    /// having it already inside the surface.
    #[test]
    fn an_effect_on_a_group_is_built_from_what_it_composites() {
        use chitrakar_doc::Effect as E;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |r: f32, g: f32, b: f32, a: f32| AuthoredColor::Srgb { r, g, b, a };
        // Two slabs that overlap between x = 30 and x = 40.
        let grouped = |opacity: f32, effects: Vec<E>| {
            let mut doc = Document::new(80, 60, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "back",
                    VectorShape::Rect {
                        width: 80.0,
                        height: 60.0,
                        radius: 0.0,
                    },
                    ink(0.92, 0.9, 0.86, 1.0),
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
            let group = doc.children_of(root).unwrap()[1];
            for (i, x) in [10.0f32, 30.0].into_iter().enumerate() {
                doc.apply(Command::AddNode {
                    parent: group,
                    index: i,
                    node: filled(
                        "slab",
                        VectorShape::Rect {
                            width: 30.0,
                            height: 20.0,
                            radius: 0.0,
                        },
                        ink(0.15, 0.2, 0.6, 1.0),
                    ),
                })
                .unwrap();
                let id = doc.children_of(group).unwrap()[i];
                doc.apply(Command::SetTransform {
                    id,
                    transform: Transform::translation(x, 10.0),
                })
                .unwrap();
            }
            doc.apply(Command::SetOpacity { id: group, opacity })
                .unwrap();
            doc.apply(Command::SetEffects { id: group, effects })
                .unwrap();
            doc
        };
        // Two overlapping strokes, which is the same question asked of a
        // brush layer.
        let brushed = |opacity: f32, effects: Vec<E>| {
            let mut doc = Document::new(80, 60, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "back",
                    VectorShape::Rect {
                        width: 80.0,
                        height: 60.0,
                        radius: 0.0,
                    },
                    ink(0.92, 0.9, 0.86, 1.0),
                ),
                Transform::default(),
            );
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::paint("brushed")),
            })
            .unwrap();
            let id = doc.children_of(root).unwrap()[1];
            for (i, x) in [[14.0f32, 46.0], [34.0, 66.0]].into_iter().enumerate() {
                doc.apply(Command::AddStroke {
                    id,
                    index: i,
                    stroke: Box::new(chitrakar_doc::PaintStroke {
                        points: vec![[x[0], 20.0], [x[1], 20.0]],
                        // Not a whole number of pixels from the line to
                        // the edge: what an outline is measured from is
                        // a yes or a no at exactly half covered, and a
                        // coverage landing exactly on a half is a tie
                        // two renderers can break differently in the
                        // last bit of a float. Nothing here is about
                        // that, so it does not sit on one.
                        radii: vec![9.4],
                        color: ink(0.15, 0.2, 0.6, 1.0),
                        softness: 0.0,
                        erase: false,
                        source: [0.0, 0.0],
                        heal: false,
                        clip: None,
                    }),
                    on_mask: false,
                })
                .unwrap();
            }
            doc.apply(Command::SetOpacity { id, opacity }).unwrap();
            doc.apply(Command::SetEffects { id, effects }).unwrap();
            doc
        };
        let shadow = |blur: f32| E::DropShadow {
            dx: 5.0,
            dy: 8.0,
            blur,
            color: ink(0.0, 0.0, 0.0, 1.0),
            opacity: 0.9,
        };
        let outline = E::Outline {
            width: 3.0,
            color: ink(0.9, 0.2, 0.05, 1.0),
            opacity: 1.0,
        };
        let inner = E::InnerShadow {
            dx: 2.0,
            dy: 2.0,
            blur: 1.0,
            color: ink(0.0, 0.0, 0.1, 1.0),
            opacity: 0.9,
        };
        for (what, build) in [
            ("a group", &grouped as &dyn Fn(f32, Vec<E>) -> Document),
            ("a brush layer", &brushed),
        ] {
            for opacity in [1.0f32, 0.45] {
                for (name, effects) in [
                    ("a hard shadow", vec![shadow(0.0)]),
                    ("a blurred shadow", vec![shadow(2.0)]),
                    ("an outline", vec![outline.clone()]),
                    ("an inner shadow", vec![inner.clone()]),
                    (
                        "all three",
                        vec![shadow(1.5), outline.clone(), inner.clone()],
                    ),
                ] {
                    let doc = build(opacity, effects);
                    assert!(
                        GpuRenderer::can_render(&doc),
                        "{what} at {opacity} with {name} is drawn rather than handed back"
                    );
                    let (mean, worst) = difference(
                        &gpu.render(&doc).unwrap(),
                        &chitrakar_render::render(&doc).unwrap(),
                    );
                    assert!(
                        mean < 0.004,
                        "{what} at {opacity} with {name}: mean {mean:.5}, worst {worst:.3}"
                    );
                }
            }
        }

        // The reading a mean would hide, and the one the whole
        // distinction is about. The two slabs overlap between x = 30 and
        // x = 40; at group opacity the pair is one half-faded shape, so
        // there is no seam down the overlap — and the shadow it casts has
        // none either. Fade the children instead and both show one.
        let half = gpu.render(&grouped(0.45, vec![shadow(0.0)])).unwrap();
        let on = |x: u32| half.get(x, 20);
        assert!(
            (on(20).r - on(35).r).abs() < 0.01 && (on(35).r - on(50).r).abs() < 0.01,
            "a half-faded group is one shape, not two laid over each other \
             ({:?} {:?} {:?})",
            on(20),
            on(35),
            on(50)
        );
        // The shadow is thrown five right and eight down, so the strip
        // just below the slabs is shadow and nothing else.
        let under = |x: u32| half.get(x, 34).r;
        assert!(
            (under(25) - under(40)).abs() < 0.01 && (under(40) - under(55)).abs() < 0.01,
            "and its shadow has no seam either ({} {} {})",
            under(25),
            under(40),
            under(55)
        );
        // The same of a brush layer, whose strokes have the same
        // conversation with each other that a group's children do.
        let painted = gpu.render(&brushed(0.45, vec![shadow(0.0)])).unwrap();
        let cast = |x: u32| painted.get(x, 34).r;
        assert!(
            (cast(25) - cast(40)).abs() < 0.01 && (cast(40) - cast(55)).abs() < 0.01,
            "a half-faded painting casts one shadow, not one per stroke ({} {} {})",
            cast(25),
            cast(40),
            cast(55)
        );

        // And the fade reaches the shadow at all: the same group at full
        // strength casts a darker one.
        let full = gpu.render(&grouped(1.0, vec![shadow(0.0)])).unwrap();
        assert!(
            full.get(40, 34).r < under(40) - 0.15,
            "a faded group casts a faded shadow ({} against {})",
            full.get(40, 34).r,
            under(40)
        );
    }

    /// An effect on a layer with a blend mode, brought down by that
    /// blend as the layer itself is.
    ///
    /// The CPU renderer stamps every effect with the layer's blend, not
    /// only the layer — a shadow under a Multiply layer multiplies. That
    /// wanted the one texture the stamp already had spoken for: a shadow
    /// is read at an offset and an inner one is held inside the
    /// silhouette, and both of those want the layer's own coverage where
    /// a blend wants what is under it. So when there is a blend those two
    /// happen a pass earlier, on the scratch pair, and what the stamp is
    /// left with is an ordinary picture to bring down.
    #[test]
    fn an_effect_on_a_blended_layer_comes_down_by_the_blend() {
        use chitrakar_doc::Effect as E;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |r: f32, g: f32, b: f32, a: f32| AuthoredColor::Srgb { r, g, b, a };
        let black = ink(0.0, 0.0, 0.0, 1.0);
        let ground = ink(0.75, 0.55, 0.35, 1.0);
        let page = |blend: BlendMode, effects: Vec<E>| {
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
                    ground.clone(),
                ),
                Transform::default(),
            );
            let id = add(
                &mut doc,
                filled(
                    "shape",
                    VectorShape::Rect {
                        width: 22.0,
                        height: 14.0,
                        radius: 3.0,
                    },
                    ink(0.2, 0.35, 0.75, 1.0),
                ),
                Transform::translation(18.0, 12.0),
            );
            doc.apply(Command::SetEffects { id, effects }).unwrap();
            doc.apply(Command::SetBlendMode { id, blend }).unwrap();
            doc
        };
        let shadow = |blur: f32| E::DropShadow {
            dx: 6.0,
            dy: 5.0,
            blur,
            color: black.clone(),
            opacity: 1.0,
        };
        let inner = E::InnerShadow {
            dx: 2.0,
            dy: 2.0,
            blur: 1.0,
            color: ink(0.0, 0.0, 0.1, 1.0),
            opacity: 0.9,
        };
        let outline = E::Outline {
            width: 3.0,
            color: ink(0.9, 0.15, 0.1, 1.0),
            opacity: 1.0,
        };
        for blend in [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Difference,
            BlendMode::Overlay,
        ] {
            for (name, effects) in [
                ("a shadow", vec![shadow(2.0)]),
                // No blur at all, so the field goes down as it was built
                // and the settling pass is the only thing between it and
                // the page.
                ("a hard shadow", vec![shadow(0.0)]),
                ("an inner shadow", vec![inner.clone()]),
                ("an outline", vec![outline.clone()]),
                (
                    "one of each",
                    vec![shadow(1.5), inner.clone(), outline.clone()],
                ),
            ] {
                let doc = page(blend, effects);
                assert!(
                    GpuRenderer::can_render(&doc),
                    "{name} under {blend:?} is drawn rather than handed back"
                );
                let (mean, worst) = difference(
                    &gpu.render(&doc).unwrap(),
                    &chitrakar_render::render(&doc).unwrap(),
                );
                assert!(
                    mean < 0.004,
                    "{name} under {blend:?}: mean {mean:.5}, worst {worst:.3}"
                );
            }
        }

        // The reading a mean would hide, and the one that says the blend
        // is really being asked: a *black* shadow screened onto the page
        // leaves it exactly as it was, since screening with black is the
        // one thing that does nothing at all. Stamp the same shadow
        // plainly instead and the page darkens under it.
        let screened = gpu
            .render(&page(BlendMode::Screen, vec![shadow(0.0)]))
            .unwrap();
        let bare = gpu.render(&page(BlendMode::Screen, Vec::new())).unwrap();
        // Past the shape's bottom-right corner, where the shadow falls
        // and the layer itself does not.
        for (x, y) in [(44u32, 28u32), (40, 30), (46, 24)] {
            let (a, b) = (screened.get(x, y), bare.get(x, y));
            assert!(
                (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01,
                "a black shadow screened on leaves ({x},{y}) alone: {a:?} against {b:?}"
            );
        }
        // And it is landing there at all — the same shadow multiplied
        // takes that spot down, so the reading above is the blend rather
        // than a shadow that missed.
        let multiplied = gpu
            .render(&page(BlendMode::Multiply, vec![shadow(0.0)]))
            .unwrap();
        assert!(
            multiplied.get(44, 28).r < bare.get(44, 28).r - 0.2,
            "the same shadow multiplied darkens it ({} against {})",
            multiplied.get(44, 28).r,
            bare.get(44, 28).r
        );
    }

    /// A field is nothing where the surface cut the layer short, rather
    /// than its own edge repeated.
    ///
    /// Every effect is built from the layer's silhouette over a window,
    /// and the stamp reads that window at the effect's offset. The CPU
    /// renderer's window is the layer's box grown by how far the effect
    /// reaches, and the field is nothing at the edge of that box — so
    /// reading past it gives nothing whichever way it is done. Except
    /// where the surface itself cuts the layer: there the field is *not*
    /// nothing at the window's edge, and a clamped read repeats it. A
    /// shape hanging off the top of the page then casts a shadow back
    /// onto the first row, out of a silhouette neither renderer has.
    #[test]
    fn an_effect_reads_nothing_where_the_surface_cut_the_layer() {
        use chitrakar_doc::Effect as E;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |r: f32, g: f32, b: f32, a: f32| AuthoredColor::Srgb { r, g, b, a };
        let hanging = |effects: Vec<E>| {
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
                    ink(0.75, 0.55, 0.35, 1.0),
                ),
                Transform::default(),
            );
            let id = add(
                &mut doc,
                filled(
                    "shape",
                    VectorShape::Rect {
                        width: 22.0,
                        height: 14.0,
                        radius: 3.0,
                    },
                    ink(0.2, 0.35, 0.75, 1.0),
                ),
                // Turned as well as hung off the top, so the silhouette
                // at the cut is not the silhouette above it — a
                // rectangle repeats its own edge and would hide this.
                Transform {
                    a: 1.5,
                    b: 0.25,
                    c: -0.25,
                    d: 1.5,
                    e: 7.0,
                    f: -3.0,
                },
            );
            doc.apply(Command::SetEffects { id, effects }).unwrap();
            doc
        };
        let doc = hanging(vec![E::DropShadow {
            dx: 5.0,
            dy: 4.0,
            blur: 2.5,
            color: ink(0.0, 0.05, 0.15, 1.0),
            opacity: 0.75,
        }]);
        assert!(GpuRenderer::can_render(&doc));
        let (mean, worst) = difference(
            &gpu.render(&doc).unwrap(),
            &chitrakar_render::render(&doc).unwrap(),
        );
        assert!(
            mean < 0.001,
            "a shape hung off the top: mean {mean:.5}, worst {worst:.3}"
        );
        // The reading itself: the first row, where the shadow would come
        // from a part of the shape that is off the page. Cast down and to
        // the right, so nothing that *is* on the page can put a shadow
        // there either.
        let cast = gpu.render(&doc).unwrap();
        let plain = gpu.render(&hanging(Vec::new())).unwrap();
        for x in [26u32, 30, 36] {
            assert!(
                (cast.get(x, 0).r - plain.get(x, 0).r).abs() < 0.1,
                "the first row keeps its colour at x = {x} ({} against {})",
                cast.get(x, 0).r,
                plain.get(x, 0).r
            );
        }
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

        // And a filter carrying a blend mode is drawn as though it had
        // none, which is what the renderer being matched does: it writes
        // a filter straight into what it read and never looks at the
        // mode. This used to hand the page back, on the grounds that a
        // blend would put the layer on a surface of its own — true, and
        // the surface was the thing to stop rather than the page.
        let mut doc = with(F::Vignette {
            amount: 0.8,
            radius: 0.2,
            softness: 0.4,
        });
        let id = doc.children_of(doc.root()).unwrap()[2];
        let plain = chitrakar_render::render(&doc).unwrap();
        for blend in [
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Difference,
        ] {
            doc.apply(Command::SetBlendMode { id, blend }).unwrap();
            assert!(
                GpuRenderer::can_render(&doc),
                "a filter wearing {blend:?} is drawn"
            );
            let reference = chitrakar_render::render(&doc).unwrap();
            // First that the mode really is ignored on both sides, since
            // that is the claim: the page is the one it draws with no
            // mode at all.
            let (was, _) = difference(&reference, &plain);
            assert!(was < 1e-6, "the mode makes no difference to the reference");
            let (mean, worst) = difference(&gpu.render(&doc).unwrap(), &reference);
            assert!(
                mean < 0.004,
                "a filter wearing {blend:?}: mean {mean:.5} (worst {worst:.3})"
            );
        }
        doc.apply(Command::SetBlendMode {
            id,
            blend: BlendMode::Normal,
        })
        .unwrap();

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

    /// A brush layer: the strokes that were laid on it, in the order
    /// they were laid.
    ///
    /// The one node kind this backend had never drawn. A stroke is not a
    /// shape with an outline — it is a round-capped band from each point
    /// to the next, as wide as the radius at each end says and fading
    /// across whatever softness the brush was set to — and the segments
    /// of one stroke *union* rather than pile up, so a stroke that
    /// doubles back is not darker where it crossed itself.
    /// A clone layer lifts what the surface already holds.
    ///
    /// It paints with what is under it at a fixed offset, read at the
    /// moment of drawing rather than kept as a copy — which is why it is
    /// never put on a surface of its own here: on one there would be
    /// nothing under it to paint with. What it lifts and what it lands
    /// on are the same copy of the surface, taken before the stroke lays
    /// anything, so a stroke running over its own source reads what was
    /// there rather than what it has just laid.
    #[test]
    fn a_clone_lifts_what_the_cpu_lifts() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |r: f32, g: f32, b: f32, a: f32| AuthoredColor::Srgb { r, g, b, a };
        let dab = |points: &[[f32; 2]], radius: f32, source: [f32; 2]| chitrakar_doc::PaintStroke {
            points: points.to_vec(),
            radii: vec![radius],
            color: ink(0.0, 0.0, 0.0, 1.0),
            softness: 0.0,
            erase: false,
            source,
            heal: false,
            clip: None,
        };
        let page = |strokes: Vec<chitrakar_doc::PaintStroke>| {
            let mut doc = Document::new(80, 60, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "back",
                    VectorShape::Rect {
                        width: 80.0,
                        height: 60.0,
                        radius: 0.0,
                    },
                    ink(0.88, 0.86, 0.82, 1.0),
                ),
                Transform::default(),
            );
            // A patch to lift from, in the top-left quarter.
            add(
                &mut doc,
                filled(
                    "patch",
                    VectorShape::Rect {
                        width: 24.0,
                        height: 20.0,
                        radius: 0.0,
                    },
                    ink(0.85, 0.15, 0.1, 1.0),
                ),
                Transform::translation(6.0, 6.0),
            );
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 2,
                node: Box::new(Node::clone_layer("cloned")),
            })
            .unwrap();
            let id = doc.children_of(root).unwrap()[2];
            for (i, s) in strokes.into_iter().enumerate() {
                doc.apply(Command::AddStroke {
                    id,
                    index: i,
                    stroke: Box::new(s),
                    on_mask: false,
                })
                .unwrap();
            }
            (doc, id)
        };
        // Painting at (50, 36) lifts from (16, 16), inside the patch.
        let lift = [-34.0, -20.0];

        type Dress = Box<dyn Fn(&mut Document, NodeId)>;
        let dressed: Vec<(&str, Dress)> = vec![
            ("plain", Box::new(|_: &mut Document, _: NodeId| {})),
            (
                "faded",
                Box::new(|doc: &mut Document, id: NodeId| {
                    doc.apply(Command::SetOpacity { id, opacity: 0.45 })
                        .unwrap();
                }),
            ),
            // The layer's blend goes on each stroke as it lands, which is
            // where the CPU renderer puts it too.
            (
                "blended",
                Box::new(|doc: &mut Document, id: NodeId| {
                    doc.apply(Command::SetBlendMode {
                        id,
                        blend: BlendMode::Multiply,
                    })
                    .unwrap();
                }),
            ),
            (
                "masked",
                Box::new(|doc: &mut Document, id: NodeId| {
                    doc.apply(Command::SetMask {
                        id,
                        mask: Some(Box::new(chitrakar_doc::Mask {
                            kind: chitrakar_doc::MaskKind::Vector {
                                shape: VectorShape::Ellipse { rx: 10.0, ry: 10.0 },
                                transform: Transform::translation(50.0, 36.0),
                            },
                            invert: false,
                            feather: 2.0,
                        })),
                    })
                    .unwrap();
                }),
            ),
        ];
        for (how, dress) in &dressed {
            for (name, strokes) in [
                ("one dab", vec![dab(&[[50.0, 36.0]], 9.0, lift)]),
                (
                    "a line",
                    vec![dab(&[[40.0, 30.0], [64.0, 44.0]], 6.0, lift)],
                ),
                // Lifting from across a sharp edge in the source: the
                // patch's own left edge falls inside what this dab
                // reads, so which pixel it reads is written down the
                // middle of what it lays. Half a pixel out and the edge
                // moves a whole one.
                (
                    "across the source's edge",
                    vec![dab(&[[44.0, 36.0]], 10.0, [-32.0, -20.0])],
                ),
                // Reading from off the page, where there is nothing to
                // lift and so nothing lands.
                (
                    "out of nowhere",
                    vec![dab(&[[10.0, 10.0]], 8.0, [-60.0, -40.0])],
                ),
                // Two, so the second sees what the first laid.
                (
                    "one after another",
                    vec![
                        dab(&[[40.0, 36.0]], 8.0, lift),
                        dab(&[[52.0, 40.0]], 8.0, [-10.0, -6.0]),
                    ],
                ),
                // Running over its own source: what it reads is what was
                // there before the stroke, not what it has just laid.
                (
                    "over its own source",
                    vec![dab(&[[20.0, 14.0], [44.0, 30.0]], 10.0, [-8.0, -5.0])],
                ),
            ] {
                let (mut doc, id) = page(strokes);
                dress(&mut doc, id);
                assert!(
                    GpuRenderer::can_render(&doc),
                    "{how} {name} is drawn rather than handed back"
                );
                let (mean, worst) = difference(
                    &gpu.render(&doc).unwrap(),
                    &chitrakar_render::render(&doc).unwrap(),
                );
                assert!(
                    mean < 0.004,
                    "{how} {name}: mean {mean:.5}, worst {worst:.3}"
                );
            }
        }

        // A clone layer is never on a surface of its own — what it paints
        // with is what is under it — so its mask goes on each stroke as
        // it lands, on the one slot a stroke's own region wants. That
        // used to hand the page back; the two are folded into one
        // coverage now, which is the same answer this backend gives a
        // layer that is both masked and held to another.
        //
        // The two are laid out so that each has a side of the dab to
        // itself and they share the middle: the layer's mask is
        // everything left of x = 56, the stroke's region everything
        // right of x = 44, and the dab is wide enough to reach past
        // both. Hard edges on purpose — what is being asked is which
        // coverages were read, and a feathered one answers that in
        // fractions.
        let band = |x: f32, w: f32| {
            Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: w,
                        height: 60.0,
                        radius: 0.0,
                    },
                    transform: Transform::translation(x, 0.0),
                },
                invert: false,
                feather: 0.0,
            })
        };
        let confined = |mask: bool, region: bool| {
            let mut stroke = dab(&[[50.0, 36.0]], 14.0, lift);
            if region {
                stroke.clip = Some(band(44.0, 36.0));
            }
            let (mut doc, id) = page(vec![stroke]);
            if mask {
                doc.apply(Command::SetMask {
                    id,
                    mask: Some(band(0.0, 56.0)),
                })
                .unwrap();
            }
            doc
        };
        // Lifted from the patch, the dab is red where it lands; the page
        // under it is not. So one channel says whether it landed.
        let landed = |s: &Surface, x: u32| s.get(x, 36).g < 0.3;
        let free = chitrakar_render::render(&confined(false, false)).unwrap();
        for x in [40u32, 50, 60] {
            assert!(
                landed(&free, x),
                "the dab reaches {x} with nothing in its way"
            );
        }
        for (what, mask, region, reach) in [
            ("a region", false, true, [false, true, true]),
            ("a mask", true, false, [true, true, false]),
            ("both", true, true, [false, true, false]),
        ] {
            let doc = confined(mask, region);
            assert!(
                GpuRenderer::can_render(&doc),
                "{what} on a clone layer is drawn rather than handed back"
            );
            let mine = gpu.render(&doc).unwrap();
            let theirs = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&mine, &theirs);
            assert!(mean < 0.004, "{what}: mean {mean:.5}, worst {worst:.3}");
            // Read on both renderers rather than compared, so that two
            // backends losing the same coverage cannot agree their way
            // past this.
            for (i, x) in [40u32, 50, 60].into_iter().enumerate() {
                for (whose, page) in [("gpu", &mine), ("cpu", &theirs)] {
                    assert_eq!(
                        landed(page, x),
                        reach[i],
                        "{what}: {whose} at {x} {:?}",
                        page.get(x, 36)
                    );
                }
            }
        }

        // The reading a mean over a page hides, which is most of what a
        // clone gets wrong: *which* pixel it reads. The patch's own left
        // edge at x = 6 falls inside what this dab lifts, so that edge
        // is written down the middle of what it lays — half a pixel out
        // in the read and the whole edge moves a pixel, which is twenty
        // pixels on a page of five thousand and disappears into a mean.
        let (doc, _) = page(vec![dab(&[[44.0, 36.0]], 10.0, [-32.0, -20.0])]);
        let (_, worst) = difference(
            &gpu.render(&doc).unwrap(),
            &chitrakar_render::render(&doc).unwrap(),
        );
        assert!(
            worst < 0.1,
            "the edge it lifts lands where the CPU lands it (worst {worst:.3})"
        );

        // The readings a mean would hide: the dab lands in the patch's
        // colour where it was painted, and the patch itself is untouched.
        let (doc, _) = page(vec![dab(&[[50.0, 36.0]], 9.0, lift)]);
        let drawn = gpu.render(&doc).unwrap();
        let patch = drawn.get(16, 16);
        let landed = drawn.get(50, 36);
        assert!(
            (landed.r - patch.r).abs() < 0.02 && (landed.g - patch.g).abs() < 0.02,
            "the dab lands in what its source shows ({landed:?} against {patch:?})"
        );
        let (bare, _) = page(Vec::new());
        let plain = gpu.render(&bare).unwrap();
        assert!(
            (drawn.get(16, 16).r - plain.get(16, 16).r).abs() < 0.01,
            "and the source itself is untouched"
        );
        assert!(
            (drawn.get(50, 12).r - plain.get(50, 12).r).abs() < 0.01,
            "and nothing lands where it did not paint"
        );

        // A stroke laid inside a region stays inside it, filled with
        // what it lifts as much as with a colour.
        let mut held = dab(&[[50.0, 36.0]], 12.0, lift);
        held.clip = Some(Box::new(chitrakar_doc::Mask {
            kind: chitrakar_doc::MaskKind::Vector {
                shape: VectorShape::Rect {
                    width: 80.0,
                    height: 36.0,
                    radius: 0.0,
                },
                transform: Transform::default(),
            },
            invert: false,
            feather: 0.0,
        }));
        let (doc, _) = page(vec![held]);
        assert!(GpuRenderer::can_render(&doc));
        let (mean, worst) = difference(
            &gpu.render(&doc).unwrap(),
            &chitrakar_render::render(&doc).unwrap(),
        );
        assert!(
            mean < 0.004,
            "a clone held to a region: mean {mean:.5}, worst {worst:.3}"
        );
        let confined = gpu.render(&doc).unwrap();
        assert!(
            (confined.get(50, 30).r - patch.r).abs() < 0.05,
            "it lands inside the region"
        );
        assert!(
            (confined.get(50, 44).r - plain.get(50, 44).r).abs() < 0.01,
            "and nothing of it outside"
        );

        // Healing is an average over the whole stroke before any of it
        // goes down, which is a reduction and not a pass of quads.
        let mut healing = dab(&[[50.0, 36.0]], 9.0, lift);
        healing.heal = true;
        let (doc, _) = page(vec![healing]);
        assert!(!GpuRenderer::can_render(&doc), "a healing stroke goes back");
    }

    /// A brush stroke laid inside a region stays inside it.
    ///
    /// The region rides on the stroke rather than being read off the
    /// document as the stroke is drawn — one held to whatever happens to
    /// be picked *now* would spill the instant the selection changed —
    /// and confining it means it stays confined after the region is let
    /// go of. So the coverage is the stroke's own, one texture per
    /// stroke, riding the same slot a layer's mask does. Nothing is
    /// baked: the whole stroke is there under the region, which is what
    /// the halves of these strokes that never show are for.
    #[test]
    fn a_stroke_laid_in_a_region_stays_in_it() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |r: f32, g: f32, b: f32, a: f32| AuthoredColor::Srgb { r, g, b, a };
        // The left half of the page, with a soft edge down the middle so
        // the region is read as a coverage rather than as a yes or a no.
        let region = |feather: f32, invert: bool| {
            Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Rect {
                        width: 30.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    transform: Transform::default(),
                },
                invert,
                feather,
            })
        };
        let stroke = |clip: Option<Box<chitrakar_doc::Mask>>, erase: bool| {
            chitrakar_doc::PaintStroke {
                // Right across the page, so half of it is outside the
                // region whichever way round the region is.
                points: vec![[4.0, 20.0], [56.0, 20.0]],
                radii: vec![6.0],
                color: ink(0.1, 0.2, 0.7, 1.0),
                softness: 0.0,
                erase,
                source: [0.0, 0.0],
                heal: false,
                clip,
            }
        };
        let page = |strokes: Vec<chitrakar_doc::PaintStroke>| {
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
                    ink(0.9, 0.88, 0.8, 1.0),
                ),
                Transform::default(),
            );
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::paint("brushed")),
            })
            .unwrap();
            let id = doc.children_of(root).unwrap()[1];
            for (i, s) in strokes.into_iter().enumerate() {
                doc.apply(Command::AddStroke {
                    id,
                    index: i,
                    stroke: Box::new(s),
                    on_mask: false,
                })
                .unwrap();
            }
            (doc, id)
        };

        for (name, strokes) in [
            (
                "held to a region",
                vec![stroke(Some(region(0.0, false)), false)],
            ),
            // Softened, so what is read is a coverage between nought and
            // one rather than an edge.
            (
                "held to a softened region",
                vec![stroke(Some(region(3.0, false)), false)],
            ),
            // The other way round, which is a different region and not
            // the same one read backwards by whoever draws it.
            (
                "held to the outside of one",
                vec![stroke(Some(region(0.0, true)), false)],
            ),
            // An eraser takes off only inside the region too.
            (
                "an eraser held to one",
                vec![stroke(None, false), stroke(Some(region(0.0, false)), true)],
            ),
            // One held and one not, on the same layer.
            (
                "one held and one free",
                vec![stroke(Some(region(0.0, false)), false), stroke(None, false)],
            ),
        ] {
            let (doc, _) = page(strokes);
            assert!(
                GpuRenderer::can_render(&doc),
                "{name} is drawn rather than handed back"
            );
            let (mean, worst) = difference(
                &gpu.render(&doc).unwrap(),
                &chitrakar_render::render(&doc).unwrap(),
            );
            assert!(mean < 0.004, "{name}: mean {mean:.5}, worst {worst:.3}");
        }

        // The readings a mean would hide. The region is the left half of
        // the page and the stroke runs the whole width of it.
        let (doc, _) = page(vec![stroke(Some(region(0.0, false)), false)]);
        let drawn = gpu.render(&doc).unwrap();
        let bare = gpu.render(&page(Vec::new()).0).unwrap();
        assert!(
            drawn.get(15, 20).b > drawn.get(15, 20).r + 0.2,
            "the stroke is laid inside the region"
        );
        assert!(
            (drawn.get(45, 20).b - bare.get(45, 20).b).abs() < 0.01,
            "and nothing of it outside ({} against {})",
            drawn.get(45, 20).b,
            bare.get(45, 20).b
        );
        // And the other way round is the other half, not the same half:
        // a region inverted is a different region, not a sign flipped
        // somewhere in the drawing.
        let (other, _) = page(vec![stroke(Some(region(0.0, true)), false)]);
        let flipped = gpu.render(&other).unwrap();
        assert!(
            flipped.get(45, 20).b > flipped.get(45, 20).r + 0.2,
            "inverted, the stroke shows on the other side"
        );
        assert!(
            (flipped.get(15, 20).b - bare.get(15, 20).b).abs() < 0.01,
            "and not on this one"
        );

        // One slot still holds one coverage, and a layer's own mask was
        // already riding it — so a stroke that also carried a region used
        // to send the page back. Two coverages read together are one
        // coverage, which is the same answer this backend has always given
        // for a layer that is both masked and held to another, so the two
        // are multiplied into the stroke's own texture instead.
        //
        // The two regions here are opposite halves of the page with a
        // stroke running across both, so a fold that dropped either one
        // would show as a whole half of the stroke.
        let (mut both, id) = page(vec![stroke(Some(region(0.0, false)), false)]);
        both.apply(Command::SetMask {
            id,
            mask: Some(region(0.0, true)),
        })
        .unwrap();
        assert!(
            GpuRenderer::can_render(&both),
            "a masked layer whose stroke also carries a region"
        );
        let two = gpu.render(&both).unwrap();
        let (mean, worst) = difference(&two, &chitrakar_render::render(&both).unwrap());
        assert!(
            mean < 0.004,
            "a region inside a mask: mean {mean:.5}, worst {worst:.3}"
        );
        // Opposite halves leave nothing, which is the answer — and an
        // answer a dropped coverage cannot give, since dropping either
        // one puts back half the stroke.
        for x in [15u32, 45] {
            assert!(
                (two.get(x, 20).b - bare.get(x, 20).b).abs() < 0.01,
                "nothing of the stroke survives both at {x}: {} against {}",
                two.get(x, 20).b,
                bare.get(x, 20).b
            );
        }
        // And the same stroke inside a mask that agrees with its region
        // is the stroke, so the fold is not simply erasing. Feathered,
        // which is the case that says *how many times* each coverage was
        // read: a hard edge is idempotent and would pass on a mask taken
        // twice, and a paint layer is drawn on a surface of its own
        // where its mask belongs to the quad that lays that surface
        // down. Half a coverage squared is a quarter, and the CPU's own
        // answer is the half.
        let (mut agreed, id) = page(vec![stroke(Some(region(0.0, false)), false)]);
        agreed
            .apply(Command::SetMask {
                id,
                mask: Some(region(8.0, false)),
            })
            .unwrap();
        let same = gpu.render(&agreed).unwrap();
        let (mean, worst) = difference(&same, &chitrakar_render::render(&agreed).unwrap());
        assert!(
            mean < 0.004,
            "a region inside the same mask: mean {mean:.5}, worst {worst:.3}"
        );
        assert!(
            same.get(15, 20).b > same.get(15, 20).r + 0.2,
            "the stroke is still laid where both agree: {:?}",
            same.get(15, 20)
        );
        // Along the feather, read against the reference rather than
        // against a number: a coverage taken twice is a different curve,
        // not a different edge.
        let theirs = chitrakar_render::render(&agreed).unwrap();
        let mut soft = 0;
        for x in 20u32..30 {
            let (a, b) = (same.get(x, 20), theirs.get(x, 20));
            if b.b > 0.02 && b.b < 0.9 {
                soft += 1;
            }
            assert!(
                (a.b - b.b).abs() < 0.02,
                "along the feather at {x}: {a:?} against {b:?}"
            );
        }
        assert!(soft >= 4, "the feather really is a ramp here: {soft} steps");
    }

    #[test]
    fn a_brush_lays_the_strokes_the_cpu_lays() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |r: f32, g: f32, b: f32, a: f32| AuthoredColor::Srgb { r, g, b, a };
        let stroke = |points: &[[f32; 2]], radii: &[f32], softness: f32, erase: bool| {
            chitrakar_doc::PaintStroke {
                points: points.to_vec(),
                radii: radii.to_vec(),
                color: ink(0.1, 0.2, 0.7, 1.0),
                softness,
                erase,
                source: [0.0, 0.0],
                heal: false,
                clip: None,
            }
        };
        let page = |strokes: Vec<chitrakar_doc::PaintStroke>| {
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
                    ink(0.9, 0.88, 0.8, 1.0),
                ),
                Transform::default(),
            );
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::paint("brushed")),
            })
            .unwrap();
            let id = doc.children_of(root).unwrap()[1];
            for (i, s) in strokes.into_iter().enumerate() {
                doc.apply(Command::AddStroke {
                    id,
                    index: i,
                    stroke: Box::new(s),
                    on_mask: false,
                })
                .unwrap();
            }
            (doc, id)
        };

        for (name, strokes) in [
            ("one dab", vec![stroke(&[[20.0, 20.0]], &[6.0], 0.0, false)]),
            (
                "a line",
                vec![stroke(&[[10.0, 10.0], [50.0, 30.0]], &[5.0], 0.0, false)],
            ),
            // Swelling from one end to the other, which is what pressure
            // and a slow hand both come out as.
            (
                "a line that swells",
                vec![stroke(
                    &[[8.0, 30.0], [30.0, 12.0], [52.0, 26.0]],
                    &[2.0, 7.0, 3.0],
                    0.0,
                    false,
                )],
            ),
            (
                "a soft brush",
                vec![stroke(&[[15.0, 20.0], [45.0, 20.0]], &[8.0], 0.8, false)],
            ),
            // Doubling back over itself: taking the most any segment lays
            // rather than adding them is what keeps the crossing from
            // being darker.
            (
                "a stroke that crosses itself",
                vec![stroke(
                    &[[15.0, 12.0], [45.0, 28.0], [15.0, 28.0], [45.0, 12.0]],
                    &[4.0],
                    0.35,
                    false,
                )],
            ),
            (
                "one over another",
                vec![
                    stroke(&[[10.0, 15.0], [50.0, 15.0]], &[6.0], 0.0, false),
                    stroke(&[[10.0, 25.0], [50.0, 25.0]], &[6.0], 0.5, false),
                ],
            ),
            (
                "and an eraser over both",
                vec![
                    stroke(&[[10.0, 15.0], [50.0, 15.0]], &[6.0], 0.0, false),
                    stroke(&[[10.0, 25.0], [50.0, 25.0]], &[6.0], 0.5, false),
                    stroke(&[[30.0, 8.0], [30.0, 32.0]], &[5.0], 0.2, true),
                ],
            ),
        ] {
            let (doc, _) = page(strokes);
            assert!(
                GpuRenderer::can_render(&doc),
                "{name} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.004,
                "{name}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
        }

        // What a mean would hide. A stroke that doubles back is one
        // colour where it crossed itself, not two coats of it.
        let (crossed, _) = page(vec![stroke(
            &[[10.0, 20.0], [50.0, 20.0], [10.0, 20.0]],
            &[5.0],
            0.6,
            false,
        )]);
        let twice = gpu.render(&crossed).unwrap();
        let (once, _) = page(vec![stroke(
            &[[10.0, 20.0], [50.0, 20.0]],
            &[5.0],
            0.6,
            false,
        )]);
        let single = gpu.render(&once).unwrap();
        assert_eq!(
            twice.get(30, 20).to_srgb8(),
            single.get(30, 20).to_srgb8(),
            "a stroke laid over itself is not darker for it"
        );
        // And the eraser really takes off rather than painting the page's
        // colour over: what is left is the layer under it.
        let (rubbed, _) = page(vec![
            stroke(&[[10.0, 20.0], [50.0, 20.0]], &[8.0], 0.0, false),
            stroke(&[[30.0, 8.0], [30.0, 32.0]], &[5.0], 0.0, true),
        ]);
        let out = gpu.render(&rubbed).unwrap();
        assert_eq!(
            out.get(30, 20).to_srgb8(),
            out.get(2, 2).to_srgb8(),
            "the eraser leaves the page showing through"
        );
    }

    /// Live effects: a shadow is the layer's own silhouette, tinted,
    /// blurred and stamped back down — which is why a shadow of a
    /// photograph is a shape rather than a picture of one.
    ///
    /// The blur is the same six box passes a blur layer takes, so what
    /// this is really holding is the rest: that the field is built from
    /// the silhouette and not from the picture, that the offset points
    /// where the layer's parent space says, that a shadow behind the
    /// layer goes down before it and an inner one after, and that an
    /// inner shadow is kept inside the silhouette rather than spilling
    /// past it.
    #[test]
    fn a_shadow_is_the_silhouette_the_cpu_casts() {
        use chitrakar_doc::Effect as E;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |r: f32, g: f32, b: f32, a: f32| AuthoredColor::Srgb { r, g, b, a };
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
                    ink(0.85, 0.87, 0.9, 1.0),
                ),
                Transform::default(),
            );
            add(
                &mut doc,
                filled(
                    "shape",
                    VectorShape::Rect {
                        width: 22.0,
                        height: 14.0,
                        radius: 3.0,
                    },
                    ink(0.2, 0.35, 0.75, 1.0),
                ),
                Transform::translation(18.0, 12.0),
            );
            doc
        };
        let with = |effects: Vec<E>| {
            let mut doc = page();
            let root = doc.root();
            let id = doc.children_of(root).unwrap()[1];
            doc.apply(Command::SetEffects { id, effects }).unwrap();
            doc
        };
        let shadow = |dx: f32, dy: f32, blur: f32, opacity: f32| E::DropShadow {
            dx,
            dy,
            blur,
            color: ink(0.0, 0.0, 0.0, 1.0),
            opacity,
        };
        let inner = |dx: f32, dy: f32, blur: f32| E::InnerShadow {
            dx,
            dy,
            blur,
            color: ink(0.05, 0.0, 0.1, 1.0),
            opacity: 0.9,
        };

        for (name, effects) in [
            ("a shadow", vec![shadow(4.0, 3.0, 2.0, 0.6)]),
            // Pointed the other way, and harder, so the offset is read
            // rather than merely applied.
            ("a shadow the other way", vec![shadow(-5.0, -2.0, 0.5, 1.0)]),
            // No blur at all: the field goes down as it was built.
            ("a hard shadow", vec![shadow(3.0, 3.0, 0.0, 0.8)]),
            ("an inner shadow", vec![inner(2.0, 2.0, 1.5)]),
            // Both at once, which is the order they go down in.
            (
                "one of each",
                vec![shadow(4.0, 4.0, 2.0, 0.7), inner(-2.0, 1.0, 1.0)],
            ),
        ] {
            let doc = with(effects);
            assert!(
                GpuRenderer::can_render(&doc),
                "{name} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.004,
                "{name}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
        }

        // The readings a mean would hide. A shadow is cast down and to
        // the right of the shape, so the page just past its bottom-right
        // corner is darker than the bare page, and the same spot on the
        // other side is not.
        let bare = gpu.render(&page()).unwrap();
        let cast = gpu.render(&with(vec![shadow(5.0, 5.0, 1.0, 1.0)])).unwrap();
        let below = |s: &chitrakar_render::Surface| s.get(42, 28).r;
        let above = |s: &chitrakar_render::Surface| s.get(15, 9).r;
        assert!(
            below(&cast) < below(&bare) - 0.1,
            "the shadow lands past the corner it is cast towards ({} against {})",
            below(&cast),
            below(&bare)
        );
        assert!(
            (above(&cast) - above(&bare)).abs() < 0.01,
            "and not on the other side of the shape ({} against {})",
            above(&cast),
            above(&bare)
        );
        // An inner shadow stays inside: the shape darkens at its edge and
        // the page beside it does not.
        let held = gpu.render(&with(vec![inner(3.0, 3.0, 1.0)])).unwrap();
        let inside = |s: &chitrakar_render::Surface| s.get(21, 15).b;
        assert!(
            inside(&held) < inside(&bare) - 0.05,
            "an inner shadow darkens the inside of the edge ({} against {})",
            inside(&held),
            inside(&bare)
        );
        assert!(
            (below(&held) - below(&bare)).abs() < 0.01,
            "and never leaves the layer ({} against {})",
            below(&held),
            below(&bare)
        );

        // A layer's own opacity, its mask and being held to the one
        // under it all decide what its silhouette is, so all three have
        // to be inside the surface a shadow is cast from rather than on
        // the way down from it. A shadow of the wrong shape is what
        // getting that backwards looks like, and it looks plausible.
        let shadowed = |go: &dyn Fn(&mut Document, NodeId)| {
            let mut doc = page();
            let root = doc.root();
            let id = doc.children_of(root).unwrap()[1];
            doc.apply(Command::SetEffects {
                id,
                effects: vec![shadow(4.0, 4.0, 1.5, 0.8)],
            })
            .unwrap();
            go(&mut doc, id);
            doc
        };
        for (name, doc) in [
            (
                "a faded layer",
                shadowed(&|doc, id| {
                    doc.apply(Command::SetOpacity { id, opacity: 0.35 })
                        .unwrap();
                }),
            ),
            (
                "a masked layer",
                shadowed(&|doc, id| {
                    doc.apply(Command::SetMask {
                        id,
                        mask: Some(Box::new(chitrakar_doc::Mask {
                            kind: chitrakar_doc::MaskKind::Vector {
                                shape: VectorShape::Ellipse { rx: 8.0, ry: 8.0 },
                                transform: Transform::translation(24.0, 19.0),
                            },
                            invert: false,
                            feather: 1.0,
                        })),
                    })
                    .unwrap();
                }),
            ),
            (
                "a layer held to the one under it",
                shadowed(&|doc, id| {
                    doc.apply(Command::SetClipped { id, clipped: true })
                        .unwrap();
                }),
            ),
        ] {
            assert!(
                GpuRenderer::can_render(&doc),
                "{name} with a shadow is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.004,
                "{name} with a shadow: mean channel difference {mean:.5} (worst {worst:.3})"
            );
        }
    }

    /// An outline: a band hugging the layer's silhouette from outside,
    /// as wide in every direction as the width it was asked for.
    ///
    /// The band is a distance rather than a blur — a blurred silhouette
    /// lifted would be a band whose softness grew with its width — and a
    /// *true* distance rather than the chamfer approximation that draws
    /// a circle as an octagon. The exact Euclidean transform separates,
    /// which is what lets this be two passes here at all: one down the
    /// columns, one along the rows. The CPU renderer measures the same
    /// distance in a sweep of its own, so what this holds is that the
    /// two land on the same band, and — the reading a mean over a page
    /// would hide — that the band round a disc comes out round.
    #[test]
    fn an_outline_is_the_band_the_cpu_measures() {
        use chitrakar_doc::Effect as E;
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |r: f32, g: f32, b: f32, a: f32| AuthoredColor::Srgb { r, g, b, a };
        let disc = || VectorShape::Ellipse { rx: 12.0, ry: 12.0 };
        // An ellipse is drawn out from the corner its transform puts,
        // so this is a disc of 12 about the middle of an 80-square page.
        let middle = Transform::translation(28.0, 28.0);
        let outlined = |shape: VectorShape, at: Transform, width: f32, opacity: f32, fade: f32| {
            let mut doc = Document::new(80, 80, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "back",
                    VectorShape::Rect {
                        width: 80.0,
                        height: 80.0,
                        radius: 0.0,
                    },
                    ink(0.88, 0.9, 0.92, 1.0),
                ),
                Transform::default(),
            );
            let id = add(
                &mut doc,
                filled("shape", shape, ink(0.1, 0.15, 0.45, 1.0)),
                at,
            );
            doc.apply(Command::SetEffects {
                id,
                effects: vec![E::Outline {
                    width,
                    color: ink(0.9, 0.1, 0.05, 1.0),
                    opacity,
                }],
            })
            .unwrap();
            doc.apply(Command::SetOpacity { id, opacity: fade })
                .unwrap();
            doc
        };

        for (name, doc) in [
            (
                "a band round a disc",
                outlined(disc(), middle, 6.0, 1.0, 1.0),
            ),
            // A band a single pixel wide, which is all feather and no
            // solid part: the arithmetic at the very edge of the band,
            // where the two renderers have the most room to disagree.
            ("a hair of a band", outlined(disc(), middle, 1.0, 1.0, 1.0)),
            // A width that lands between pixels, and a partly clear ink.
            (
                "a fraction of a pixel",
                outlined(disc(), middle, 3.5, 0.65, 1.0),
            ),
            // A faded layer still has an edge, and it is half of *its*
            // coverage rather than half of a full one — a layer at a
            // third opacity would otherwise cast no outline at all.
            (
                "round a faded layer",
                outlined(disc(), middle, 4.0, 1.0, 0.35),
            ),
            // Corners, where a distance rounds and a box would not.
            (
                "round a box's corners",
                outlined(
                    VectorShape::Rect {
                        width: 30.0,
                        height: 18.0,
                        radius: 4.0,
                    },
                    Transform::translation(25.0, 31.0),
                    5.0,
                    0.8,
                    1.0,
                ),
            ),
            // Asked for nothing: the CPU renderer draws no band, and
            // neither can this.
            ("no width at all", outlined(disc(), middle, 0.0, 1.0, 1.0)),
        ] {
            assert!(
                GpuRenderer::can_render(&doc),
                "{name} is drawn rather than handed back"
            );
            let drawn = gpu.render(&doc).unwrap();
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&drawn, &reference);
            assert!(
                mean < 0.004,
                "{name}: mean channel difference {mean:.5} (worst {worst:.3})"
            );
        }

        // Round, and not merely near enough on average: the furthest
        // pixel the band paints against the nearest one it leaves bare,
        // both by their true distance from the middle. A chamfer sweep
        // reaches nearly a pixel less far at an eighth of a turn than it
        // does along an axis, and a mean over a page cannot tell that
        // from a band a shade too thin.
        //
        // The same disc and the same band the CPU renderer's own
        // `an_outline_round_a_disc_is_round` measures, so the two
        // readings can be held beside each other: a disc of 30 with a
        // band of 20 on a bare page, which reaches 50.
        let mut doc = Document::new(160, 160, ColorMode::Rgb);
        let id = add(
            &mut doc,
            filled(
                "disc",
                VectorShape::Ellipse { rx: 30.0, ry: 30.0 },
                ink(0.0, 0.0, 0.0, 1.0),
            ),
            Transform::translation(50.0, 50.0),
        );
        doc.apply(Command::SetEffects {
            id,
            effects: vec![E::Outline {
                width: 20.0,
                color: ink(1.0, 0.0, 0.0, 1.0),
                opacity: 1.0,
            }],
        })
        .unwrap();
        let drawn = gpu.render(&doc).unwrap();
        let (mut furthest_on, mut nearest_off) = (0.0f32, f32::MAX);
        for y in 0..drawn.height {
            for x in 0..drawn.width {
                let (dx, dy) = (x as f32 + 0.5 - 80.0, y as f32 + 0.5 - 80.0);
                let r = (dx * dx + dy * dy).sqrt();
                // Past the disc's own edge, where the band is all there
                // is to paint anything.
                if r < 32.0 {
                    continue;
                }
                if drawn.get(x, y).a > 0.5 {
                    furthest_on = furthest_on.max(r);
                } else {
                    nearest_off = nearest_off.min(r);
                }
            }
        }
        let gap = furthest_on - nearest_off;
        assert!(
            gap < 0.5,
            "the band's edge is ragged: painted out to {furthest_on:.2} \
             and bare from {nearest_off:.2} ({gap:.2} of raggedness)"
        );
        // And as wide as it was asked for, not merely even.
        assert!(
            (furthest_on - 50.0).abs() < 1.0,
            "a disc of 30 with a band of 20 reaches about 50, not {furthest_on:.2}"
        );
    }

    /// A view: the surface stops being the page.
    ///
    /// This backend drew the page at its own size, and the app shows a
    /// viewport — a scale and an origin — so presenting from it at all
    /// means the walk and the shaders learning a mapping. Held against
    /// the CPU renderer's own `render_region_at`, which is the same
    /// mapping on the other side, at four views that pull the two apart
    /// in different ways: bigger than the surface, smaller than it,
    /// panned so the page's corner is inside it, and at a scale that
    /// lands nothing on whole pixels.
    #[test]
    fn a_view_draws_what_the_cpu_draws_into_the_same_surface() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
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
                    r: 0.12,
                    g: 0.14,
                    b: 0.22,
                    a: 1.0,
                },
            ),
            Transform::default(),
        );
        // A round-cornered box and an ellipse, so there are curves whose
        // edges are re-solved at the view's scale rather than magnified.
        add(
            &mut doc,
            filled(
                "box",
                VectorShape::Rect {
                    width: 24.0,
                    height: 16.0,
                    radius: 5.0,
                },
                AuthoredColor::Srgb {
                    r: 0.95,
                    g: 0.85,
                    b: 0.35,
                    a: 1.0,
                },
            ),
            Transform::translation(8.0, 6.0),
        );
        add(
            &mut doc,
            filled(
                "blob",
                VectorShape::Ellipse { rx: 9.0, ry: 6.0 },
                AuthoredColor::Srgb {
                    r: 0.3,
                    g: 0.75,
                    b: 0.9,
                    a: 0.7,
                },
            ),
            Transform::translation(40.0, 26.0),
        );
        // Text, whose outlines are re-solved at the view's scale on both
        // sides rather than magnified.
        add(
            &mut doc,
            Box::new(Node::text(
                "words",
                chitrakar_doc::TextSpec::new(
                    "Ag",
                    11.0,
                    AuthoredColor::Srgb {
                        r: 0.95,
                        g: 0.95,
                        b: 0.9,
                        a: 1.0,
                    },
                ),
            )),
            Transform::translation(6.0, 30.0),
        );
        // A masked layer and a layer held to the one under it: both read a
        // coverage plane the CPU renderer rasterizes, and the plane is the
        // size of what is being drawn on rather than of the page — which
        // is a distinction that only exists once a view does.
        let masked = add(
            &mut doc,
            filled(
                "masked",
                VectorShape::Rect {
                    width: 22.0,
                    height: 14.0,
                    radius: 0.0,
                },
                AuthoredColor::Srgb {
                    r: 0.55,
                    g: 0.95,
                    b: 0.4,
                    a: 1.0,
                },
            ),
            Transform::translation(30.0, 4.0),
        );
        doc.apply(Command::SetMask {
            id: masked,
            mask: Some(Box::new(chitrakar_doc::Mask {
                kind: chitrakar_doc::MaskKind::Vector {
                    shape: VectorShape::Ellipse { rx: 9.0, ry: 7.0 },
                    transform: Transform::translation(40.0, 11.0),
                },
                invert: false,
                feather: 1.5,
            })),
        })
        .unwrap();
        // The one it is held to has to be a plain drawn layer — the run
        // is held to an alpha read off the layer alone, which is only a
        // plain question when nothing was done to it on the way down.
        add(
            &mut doc,
            filled(
                "the one it is held to",
                VectorShape::Ellipse { rx: 8.0, ry: 8.0 },
                AuthoredColor::Srgb {
                    r: 0.2,
                    g: 0.3,
                    b: 0.5,
                    a: 1.0,
                },
            ),
            Transform::translation(14.0, 30.0),
        );
        let held = add(
            &mut doc,
            filled(
                "held to it",
                VectorShape::Rect {
                    width: 30.0,
                    height: 12.0,
                    radius: 0.0,
                },
                AuthoredColor::Srgb {
                    r: 0.85,
                    g: 0.4,
                    b: 0.85,
                    a: 1.0,
                },
            ),
            Transform::translation(2.0, 26.0),
        );
        doc.apply(Command::SetClipped {
            id: held,
            clipped: true,
        })
        .unwrap();
        // And one hanging off the page's corner, so that the page's own
        // edge is something the render has to say rather than something
        // the surface happens to enforce.
        add(
            &mut doc,
            filled(
                "over the edge",
                VectorShape::Rect {
                    width: 26.0,
                    height: 18.0,
                    radius: 0.0,
                },
                AuthoredColor::Srgb {
                    r: 0.9,
                    g: 0.3,
                    b: 0.45,
                    a: 1.0,
                },
            ),
            Transform::translation(-11.0, -7.0),
        );
        // And a vignette over all of it. A pointwise filter is the one
        // thing on a page that is a function of *where a pixel is*, so
        // it is the one thing a view can put in the wrong place — and
        // this page had none, which is why the backend measured both of
        // them from the surface for as long as it did. A vignette is
        // placed from the page's own middle in the page's own units, so
        // under a view that reaches past the page, or scales it, or puts
        // it in a corner of the surface, the surface's middle is not the
        // answer.
        let corners = doc.children_of(doc.root()).unwrap().len();
        doc.apply(Command::AddNode {
            parent: doc.root(),
            index: corners,
            node: Box::new(Node::filter(
                "corners",
                chitrakar_doc::Filter::Vignette {
                    amount: 0.8,
                    radius: 0.1,
                    softness: 0.5,
                },
            )),
        })
        .unwrap();

        let at = |scale: f32, x: f32, y: f32| Transform {
            a: scale,
            b: 0.0,
            c: 0.0,
            d: scale,
            e: x,
            f: y,
        };
        for (name, view, size) in [
            (
                "the page at its own size",
                at(1.0, 0.0, 0.0),
                (60u32, 40u32),
            ),
            ("twice as big", at(2.0, 0.0, 0.0), (120, 80)),
            // Small enough that the page is a patch in the middle, which
            // is what makes the page's own edge something to say rather
            // than something to assume.
            (
                "half size, with room around it",
                at(0.5, 20.0, 15.0),
                (80, 60),
            ),
            // A scale nothing lands on whole pixels at, panned so the
            // page's corner is inside the surface.
            (
                "panned, at an awkward scale",
                at(1.7, -13.5, -7.25),
                (70, 50),
            ),
        ] {
            assert!(
                GpuRenderer::can_render_view(&doc, view, size),
                "{name} is drawn rather than handed back"
            );
            let drawn = gpu.render_view(&doc, view, size).unwrap();
            assert_eq!(
                (drawn.width, drawn.height),
                size,
                "{name} is the size asked for"
            );
            let mut surface = Surface::new(size.0, size.1);
            let clip = surface.full_clip();
            chitrakar_render::render_region_at(&doc, &mut surface, clip, view).unwrap();
            let (mean, worst) = difference(&drawn, &surface);
            assert!(
                mean < 0.004,
                "{name}: mean channel difference {mean:.5} (worst {worst:.3})"
            );

            // And again with something that reads a neighbourhood over
            // it. A blur is where the page's edge stops being a
            // formality: the box passes would otherwise clamp at the
            // surface's edge, and where the page does not fill the
            // surface that is a different lane to run off the end of.
            let mut blurred = doc.clone();
            let root = blurred.root();
            let index = blurred.children_of(root).unwrap().len();
            blurred
                .apply(Command::AddNode {
                    parent: root,
                    index,
                    node: Box::new(Node::filter(
                        "soften",
                        chitrakar_doc::Filter::GaussianBlur { sigma: 2.5 },
                    )),
                })
                .unwrap();
            assert!(
                GpuRenderer::can_render_view(&blurred, view, size),
                "{name}, blurred, is drawn rather than handed back"
            );
            let drawn = gpu.render_view(&blurred, view, size).unwrap();
            let mut surface = Surface::new(size.0, size.1);
            let clip = surface.full_clip();
            chitrakar_render::render_region_at(&blurred, &mut surface, clip, view).unwrap();
            let (mean, worst) = difference(&drawn, &surface);
            assert!(
                mean < 0.004,
                "{name}, blurred: mean channel difference {mean:.5} (worst {worst:.3})"
            );
        }
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

    /// What the backend will draw, said as a table rather than left in
    /// prose scattered through the walk.
    ///
    /// Every kind of layer, each wearing each of the things a layer can
    /// wear, asked whether the page is accepted. A backend that hands a
    /// page back is doing the right thing where it cannot match the
    /// renderer that is the reference — but "cannot" should be a decision
    /// somebody made, not something nobody noticed, and a limit closed by
    /// accident should show up as plainly as one opened on purpose. So
    /// the table is asserted, and every `no` in it carries its reason:
    ///
    /// - An **adjustment** or a **filter** rewrites what is under it, so
    ///   there is nothing left over for a blend to work against; the CPU
    ///   renderer hands them their opacity and their mask and never looks
    ///   at the blend mode. A surface of its own would be a different
    ///   picture.
    /// - An effect grows from a layer's silhouette, and those two have
    ///   none: what they draw *is* what is under them. Nor has a **clone
    ///   layer**, which is never put on a surface of its own — on one
    ///   there would be nothing under it to paint with.
    /// - A **copy** is drawn by walking what it copies, and that walk
    ///   ends this one, so there is no surface of its own to fade, blend,
    ///   mask or cast a shadow from. This is the one row here that is a
    ///   gap rather than a decision, and it is the biggest thing left to
    ///   do in this backend.
    #[test]
    fn what_it_will_draw_is_written_down() {
        let square = VectorShape::Rect {
            width: 16.0,
            height: 12.0,
            radius: 0.0,
        };
        const KINDS: [&str; 9] = [
            "vector",
            "raster",
            "text",
            "group",
            "paint",
            "clone",
            "adjustment",
            "filter",
            "copy",
        ];
        const DRESS: [&str; 6] = ["plain", "faded", "blended", "masked", "clipped", "effects"];
        // What each kind can wear and still be drawn. Read across DRESS.
        const TABLE: [[bool; 6]; 9] = [
            [true, true, true, true, true, true],  // vector
            [true, true, true, true, true, true],  // raster
            [true, true, true, true, true, true],  // text
            [true, true, true, true, true, true],  // group
            [true, true, true, true, true, true],  // paint
            [true, true, true, true, true, false], // clone
            // An adjustment and a filter wear an effect and are still
            // drawn — by *ignoring* it, which is what the reference
            // renderer has always done with one: there is no silhouette
            // to build an effect from on a layer that rewrites what is
            // under it. Handing the page back for an effect that changes
            // nothing was a page declined for no reason.
            [true, true, true, true, true, true], // adjustment
            [true, true, true, true, true, true], // filter
            [true, true, true, true, true, true], // copy
        ];
        let mut wrong = Vec::new();
        for (k, kind) in KINDS.iter().enumerate() {
            for (d, dress) in DRESS.iter().enumerate() {
                let mut doc = Document::new(60, 40, ColorMode::Rgb);
                // A layer underneath, so clipping has something to be
                // held to and an adjustment something to change.
                add(
                    &mut doc,
                    filled(
                        "base",
                        VectorShape::Rect {
                            width: 60.0,
                            height: 40.0,
                            radius: 0.0,
                        },
                        RED,
                    ),
                    Transform::default(),
                );
                let node: Box<Node> = match *kind {
                    "vector" => filled("v", square.clone(), BLUE),
                    "raster" => {
                        let id = doc.add_resource(
                            2,
                            2,
                            vec![
                                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
                            ],
                        );
                        Box::new(Node::raster(
                            "r",
                            chitrakar_doc::RasterRef {
                                resource_id: id,
                                width: 10,
                                height: 8,
                            },
                        ))
                    }
                    "text" => Box::new(Node::text(
                        "t",
                        chitrakar_doc::TextSpec::new("Ab", 12.0, BLUE),
                    )),
                    "group" => Box::new(Node::group("g")),
                    "paint" | "clone" => {
                        let stroke = chitrakar_doc::PaintStroke {
                            points: vec![[2.0, 2.0], [14.0, 10.0]],
                            radii: vec![2.0],
                            color: BLUE,
                            softness: 0.0,
                            erase: false,
                            source: if *kind == "clone" {
                                [8.0, 6.0]
                            } else {
                                [0.0, 0.0]
                            },
                            heal: false,
                            clip: None,
                        };
                        let mut n = if *kind == "clone" {
                            Node::clone_layer("c")
                        } else {
                            Node::paint("p")
                        };
                        match &mut n.kind {
                            NodeKind::Paint { strokes } | NodeKind::Clone { strokes } => {
                                strokes.push(stroke)
                            }
                            _ => unreachable!(),
                        }
                        Box::new(n)
                    }
                    "adjustment" => Box::new(Node::adjustment(
                        "a",
                        chitrakar_doc::Adjustment::Exposure { stops: -0.5 },
                    )),
                    "filter" => Box::new(Node::filter(
                        "f",
                        chitrakar_doc::Filter::GaussianBlur { sigma: 1.2 },
                    )),
                    _ => {
                        let of = doc.children_of(doc.root()).unwrap()[0];
                        Box::new(Node::instance("copy", of))
                    }
                };
                let id = add(&mut doc, node, Transform::translation(20.0, 14.0));
                if *kind == "group" {
                    doc.apply(Command::AddNode {
                        parent: id,
                        index: 0,
                        node: filled("in", square.clone(), BLUE),
                    })
                    .unwrap();
                }
                match *dress {
                    "faded" => doc.apply(Command::SetOpacity { id, opacity: 0.6 }).unwrap(),
                    "blended" => doc
                        .apply(Command::SetBlendMode {
                            id,
                            blend: BlendMode::Multiply,
                        })
                        .unwrap(),
                    "masked" => doc
                        .apply(Command::SetMask {
                            id,
                            mask: Some(Box::new(chitrakar_doc::Mask {
                                kind: chitrakar_doc::MaskKind::Vector {
                                    shape: VectorShape::Ellipse { rx: 6.0, ry: 5.0 },
                                    transform: Transform::default(),
                                },
                                invert: false,
                                feather: 0.0,
                            })),
                        })
                        .unwrap(),
                    "clipped" => doc
                        .apply(Command::SetClipped { id, clipped: true })
                        .unwrap(),
                    "effects" => doc
                        .apply(Command::SetEffects {
                            id,
                            effects: vec![chitrakar_doc::Effect::DropShadow {
                                dx: 2.0,
                                dy: 2.0,
                                blur: 1.0,
                                color: BLUE,
                                opacity: 0.8,
                            }],
                        })
                        .unwrap(),
                    _ => Command::SetName {
                        id,
                        name: "unchanged".into(),
                    },
                };
                let drawn = GpuRenderer::can_render(&doc);
                if drawn != TABLE[k][d] {
                    wrong.push(format!(
                        "{kind} {dress}: the table says {} and it says {drawn}",
                        TABLE[k][d]
                    ));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "what the backend draws has changed; the table has to say so too:\n{}",
            wrong.join("\n")
        );
    }

    /// What a layer can be *held to*: anything that paints, and nothing
    /// that draws by reading what is under it.
    ///
    /// A clipped layer shows where the layer below it has alpha, and that
    /// alpha comes from the renderer being matched — the base drawn
    /// aside. For a shape, a picture, a block of text, a group, a brush
    /// layer, a copy or a frame that is a number both sides agree on to a
    /// ten-thousandth. A clone layer, an adjustment and a filter have no
    /// alpha of their own — what they draw *is* what is under them — and
    /// drawn aside they are nothing like what they are on the page: held
    /// to one of those, the two renderers come apart by a sixth of full
    /// scale. So those three hand the page back and the rest are bases.
    ///
    /// The line used to be drawn at shapes, pictures and text, which let
    /// four kinds that paint perfectly well decline for no reason.
    #[test]
    fn anything_that_paints_can_be_held_to() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let shape = VectorShape::Rect {
            width: 24.0,
            height: 18.0,
            radius: 0.0,
        };
        for (kind, paints) in [
            ("vector", true),
            ("raster", true),
            ("text", true),
            ("group", true),
            ("paint", true),
            ("copy", true),
            ("frame", true),
            ("clone", false),
            ("adjustment", false),
            ("filter", false),
        ] {
            let mut doc = Document::new(60, 40, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "ground",
                    VectorShape::Rect {
                        width: 60.0,
                        height: 40.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.85,
                        g: 0.8,
                        b: 0.2,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            let node: Box<Node> = match kind {
                "vector" => filled("base", shape.clone(), RED),
                "raster" => {
                    let id = doc.add_resource(
                        2,
                        2,
                        vec![
                            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
                        ],
                    );
                    Box::new(Node::raster(
                        "base",
                        chitrakar_doc::RasterRef {
                            resource_id: id,
                            width: 20,
                            height: 16,
                        },
                    ))
                }
                "text" => Box::new(Node::text(
                    "base",
                    chitrakar_doc::TextSpec::new("Abc", 18.0, RED),
                )),
                "group" => Box::new(Node::group("base")),
                "frame" => Box::new(Node::artboard("base", 24.0, 18.0, Some(RED))),
                "paint" | "clone" => {
                    let stroke = chitrakar_doc::PaintStroke {
                        points: vec![[2.0, 2.0], [20.0, 14.0]],
                        radii: vec![4.0],
                        color: RED,
                        softness: 0.0,
                        erase: false,
                        source: if kind == "clone" {
                            [8.0, 6.0]
                        } else {
                            [0.0, 0.0]
                        },
                        heal: false,
                        clip: None,
                    };
                    let mut n = if kind == "clone" {
                        Node::clone_layer("base")
                    } else {
                        Node::paint("base")
                    };
                    match &mut n.kind {
                        NodeKind::Paint { strokes } | NodeKind::Clone { strokes } => {
                            strokes.push(stroke)
                        }
                        _ => unreachable!(),
                    }
                    Box::new(n)
                }
                "adjustment" => Box::new(Node::adjustment(
                    "base",
                    chitrakar_doc::Adjustment::Exposure { stops: -0.6 },
                )),
                "copy" => {
                    let of = doc.children_of(doc.root()).unwrap()[0];
                    Box::new(Node::instance("base", of))
                }
                _ => Box::new(Node::filter(
                    "base",
                    chitrakar_doc::Filter::GaussianBlur { sigma: 1.0 },
                )),
            };
            let base = add(&mut doc, node, Transform::translation(8.0, 6.0));
            if kind == "group" {
                doc.apply(Command::AddNode {
                    parent: base,
                    index: 0,
                    node: filled("in", shape.clone(), RED),
                })
                .unwrap();
            }
            let over = add(
                &mut doc,
                filled(
                    "over",
                    VectorShape::Rect {
                        width: 30.0,
                        height: 26.0,
                        radius: 0.0,
                    },
                    BLUE,
                ),
                Transform::translation(14.0, 10.0),
            );
            doc.apply(Command::SetClipped {
                id: over,
                clipped: true,
            })
            .unwrap();
            assert_eq!(
                GpuRenderer::can_render(&doc),
                paints,
                "held to a {kind}: drawn should be {paints}"
            );
            if !paints {
                continue;
            }
            let (mean, worst) = difference(
                &gpu.render(&doc).unwrap(),
                &chitrakar_render::render(&doc).unwrap(),
            );
            assert!(
                mean < 0.004,
                "held to a {kind}: mean {mean:.5} (worst {worst:.3})"
            );
        }
    }

    /// A picture's border is the fraction of the pixel it covers.
    ///
    /// A raster is a quad, and a quad's edge on this backend is whatever
    /// the four-sample coverage mask caught: nothing, a quarter, a half.
    /// The renderer being matched computes the area exactly, so a picture
    /// placed six tenths of a pixel along read 0 where the reference read
    /// 0.18. The quad is drawn a device pixel wider than the box now and
    /// the fragment shader fades it by the area, which is exact for a
    /// picture square to the page — so the border is asked for the area
    /// arithmetic says it is, not merely for agreement with the reference.
    #[test]
    fn a_pictures_border_is_the_area_it_covers() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = Document::new(40, 30, ColorMode::Rgb);
        let id = doc.add_resource(2, 2, vec![255; 16]);
        // Twelve and a half times along, nine and a bit down, so that
        // every edge falls inside a pixel rather than between two.
        let (ox, oy, sx, sy) = (8.4f32, 6.3f32, 12.5f32, 9.75f32);
        add(
            &mut doc,
            Box::new(Node::raster(
                "a picture off the grid",
                chitrakar_doc::RasterRef {
                    resource_id: id,
                    width: 2,
                    height: 2,
                },
            )),
            Transform {
                a: sx,
                d: sy,
                e: ox,
                f: oy,
                ..Default::default()
            },
        );
        assert!(GpuRenderer::can_render(&doc));
        let mine = gpu.render(&doc).unwrap();
        let theirs = chitrakar_render::render(&doc).unwrap();
        // How much of pixel `at` the span `lo..hi` covers.
        let span = |lo: f32, hi: f32, at: usize| {
            (hi.min(at as f32 + 1.0) - lo.max(at as f32)).clamp(0.0, 1.0)
        };
        let (x1, y1) = (ox + 2.0 * sx, oy + 2.0 * sy);
        let mut checked = 0;
        for y in 5..27usize {
            for x in 7..35usize {
                let want = span(ox, x1, x) * span(oy, y1, y);
                // Only the border: inside is a flat white either way.
                if want > 0.999 || want == 0.0 {
                    continue;
                }
                let got = mine.pixels[y * 40 + x].a;
                let reference = theirs.pixels[y * 40 + x].a;
                assert!(
                    (reference - want).abs() < 0.01,
                    "the reference draws ({x},{y}) at {reference:.3}, not the {want:.3} it covers"
                );
                assert!(
                    (got - want).abs() < 0.01,
                    "({x},{y}) of the border: {got:.3}, not the {want:.3} it covers"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 88, "the border is 88 part-covered pixels");
        let (mean, worst) = difference(&mine, &theirs);
        assert!(
            mean < 0.0005 && worst < 0.01,
            "mean {mean:.5}, worst {worst:.3}"
        );
    }

    /// A shape's edge is within a twentieth of the area it really covers.
    ///
    /// The reference renderer's fills are exact — a rect's by a product
    /// of two 1-D overlaps, an ellipse's by the two roots of a quadratic
    /// — so what this measures is this backend's analytic coverage and
    /// nothing else. Two things were wrong and both were found by
    /// measuring rather than reading:
    ///
    /// `fwidth` is |dx| + |dy|, which is the rate a quantity changes
    /// across a pixel only when one of those is zero. On an edge at
    /// forty-five degrees the two differ by root two, so every ramp
    /// scaled by it came out that much too wide. It is the gradient's
    /// length now, which took a rounded rect's corners from a seventh
    /// out to a sixteenth and halved the average error along an
    /// ellipse's rim.
    ///
    /// And an ellipse's first-order distance is singular at its middle,
    /// where the gradient goes to nothing: a number that large has no
    /// meaningful rate of change across a pixel, and it left a stray
    /// pixel three-quarters covered in the middle of a solid disc. That
    /// was the worst disagreement anywhere between the two renderers and
    /// it was not on an edge at all, which is why looking for it as an
    /// edge problem had not found it. The distance is held to a few
    /// pixels either side of the rim now.
    #[test]
    fn a_shapes_edge_is_the_area_it_covers() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        for (what, shape, ceiling) in [
            (
                "a disc",
                VectorShape::Ellipse {
                    rx: 7.0788,
                    ry: 6.9928,
                },
                0.03f32,
            ),
            (
                "a squashed one",
                VectorShape::Ellipse { rx: 12.0, ry: 5.0 },
                0.07,
            ),
            (
                "a rounded rectangle",
                VectorShape::Rect {
                    width: 20.0,
                    height: 14.0,
                    radius: 4.0,
                },
                0.035,
            ),
            (
                "a square-cornered one",
                VectorShape::Rect {
                    width: 20.0,
                    height: 14.0,
                    radius: 0.0,
                },
                0.001,
            ),
        ] {
            let mut doc = Document::new(40, 30, ColorMode::Rgb);
            // Off the grid on both axes, so every edge falls inside a
            // pixel rather than between two.
            add(
                &mut doc,
                filled("s", shape, RED),
                Transform::translation(6.4, 5.3),
            );
            assert!(GpuRenderer::can_render(&doc), "{what} is drawn here");
            let mine = gpu.render(&doc).unwrap();
            let theirs = chitrakar_render::render(&doc).unwrap();
            let (mut worst, mut at) = (0.0f32, (0usize, 0usize));
            for (i, (a, b)) in mine.pixels.iter().zip(&theirs.pixels).enumerate() {
                let d = (a.a - b.a).abs();
                if d > worst {
                    worst = d;
                    at = (i % 40, i / 40);
                }
            }
            assert!(
                worst <= ceiling,
                "{what}: worst pixel off by {worst:.3} at {at:?}, past {ceiling:.3}"
            );
            // And the areas agree, which a shape drawn systematically
            // fat or thin would fail even with every single pixel inside
            // the ceiling above.
            let (ga, ca): (f64, f64) = (
                mine.pixels.iter().map(|p| p.a as f64).sum(),
                theirs.pixels.iter().map(|p| p.a as f64).sum(),
            );
            assert!(
                (ga - ca).abs() < 0.5,
                "{what}: {ga:.2} pixels of it against the reference's {ca:.2}"
            );
        }
    }

    /// A stroke's band is the area it covers too.
    ///
    /// A band is one outline less another, so measuring it by a distance
    /// is two roundings-off rather than one — and a rectangle whose
    /// *fill* is exact here had the worst band of any shape, a fifth of
    /// full scale. It need not be: the outline grown and the outline
    /// shrunk are both boxes, the shrunk one lies inside the grown one,
    /// and a box's coverage is exact, so the area between them is the
    /// difference of two exact answers.
    ///
    /// With one reservation, which is the reference renderer's too: a
    /// band is carried round a corner by the join, and what is drawn
    /// there is a quarter circle of the band's own reach rather than a
    /// square. Squaring it off cost far more than the distance had on an
    /// outside-aligned stroke, where the reach is the whole width —
    /// two thirds of full scale, measured. So the corner is taken as an
    /// arc where the arc is more than a pixel across, and as the box
    /// below that, where an arc and the corner it cuts are not
    /// distinguishable and the box is exact in every other respect.
    ///
    /// The two curved bands are here for the same reason a fill's rim is:
    /// neither of their outlines is a box, so neither is exact, and what
    /// holds them where they are is `half_plane` — the area a straight
    /// edge really lets through, which is the whole of what a distance
    /// can say about a curve to first order. It took a rounded rect's
    /// band from a tenth of full scale to a twentieth and a disc's from
    /// 0.115 to 0.088.
    #[test]
    fn a_strokes_band_is_the_area_it_covers() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let square = VectorShape::Rect {
            width: 20.0,
            height: 14.0,
            radius: 0.0,
        };
        for (what, shape, width, align, ceiling) in [
            ("a hairline", square.clone(), 1.0f32, None, 0.03f32),
            ("two wide", square.clone(), 2.0, None, 0.03),
            (
                "inside the shape",
                square.clone(),
                3.0,
                Some(chitrakar_doc::StrokeAlign::Inside),
                0.03,
            ),
            (
                "outside it",
                square.clone(),
                3.0,
                Some(chitrakar_doc::StrokeAlign::Outside),
                0.05,
            ),
            ("wider than the shape", square.clone(), 40.0, None, 0.03),
            // And the two whose outlines are curves, where neither is a
            // box and so neither is exact: what holds them where they are
            // is the area a straight edge really lets through, which is
            // all a distance can say about a curve to first order.
            (
                "round-cornered",
                VectorShape::Rect {
                    width: 20.0,
                    height: 14.0,
                    radius: 4.0,
                },
                2.0,
                None,
                0.06,
            ),
            (
                "round the rim of a disc",
                VectorShape::Ellipse {
                    rx: 7.0788,
                    ry: 6.9928,
                },
                2.0,
                None,
                0.10,
            ),
        ] {
            let mut doc = Document::new(40, 30, ColorMode::Rgb);
            let mut node = Node::vector("s", shape);
            if let NodeKind::Vector { fill, stroke, .. } = &mut node.kind {
                *fill = Some(RED);
                *stroke = Some(chitrakar_doc::Stroke {
                    color: AuthoredColor::Srgb {
                        r: 0.0,
                        g: 0.0,
                        b: 1.0,
                        a: 1.0,
                    },
                    width,
                    widths: Vec::new(),
                    dash: Vec::new(),
                    cap: Default::default(),
                    join: Default::default(),
                    align,
                    start_marker: Default::default(),
                    end_marker: Default::default(),
                });
            }
            add(&mut doc, Box::new(node), Transform::translation(6.4, 5.3));
            assert!(GpuRenderer::can_render(&doc), "{what} is drawn here");
            let mine = gpu.render(&doc).unwrap();
            let theirs = chitrakar_render::render(&doc).unwrap();
            let (mut worst, mut at) = (0.0f32, (0usize, 0usize));
            for (i, (a, b)) in mine.pixels.iter().zip(&theirs.pixels).enumerate() {
                let d = (a.r - b.r)
                    .abs()
                    .max((a.g - b.g).abs())
                    .max((a.b - b.b).abs())
                    .max((a.a - b.a).abs());
                if d > worst {
                    worst = d;
                    at = (i % 40, i / 40);
                }
            }
            assert!(
                worst <= ceiling,
                "{what}: worst pixel off by {worst:.3} at {at:?}, past {ceiling:.3} \
                 (gpu {:?} against {:?})",
                mine.pixels[at.1 * 40 + at.0].to_srgb8(),
                theirs.pixels[at.1 * 40 + at.0].to_srgb8()
            );
            // And the band is actually there, or a backend that drew no
            // stroke at all would pass every line above.
            let inked = mine
                .pixels
                .iter()
                .filter(|p| p.b > 0.5 && p.r < 0.5)
                .count();
            assert!(inked > 30, "{what}: only {inked} pixels of band drawn");
        }
    }

    /// How far apart two renderings are *inside* shapes, where neither
    /// has an edge to disagree about.
    ///
    /// An interior point is one whose eight neighbours the reference
    /// renderer draws in exactly its own colour: no edge passes through
    /// it, nothing is part-covered, and so the only thing that can differ
    /// is what the two renderers think the colour *is*. That leaves out
    /// exactly what is allowed to differ — antialiasing — and, unlike a
    /// page-wide mean, it grows rather than thins as a page gains
    /// elements.
    ///
    /// Gives the mean over those points, the worst of them, how many are
    /// past a fifth of full scale, and where the worst one is.
    fn interiors(mine: &Surface, reference: &Surface) -> (f32, f32, usize, (usize, usize)) {
        let (w, h) = (mine.width as usize, mine.height as usize);
        let same = |i: usize, j: usize| {
            let (a, b) = (&reference.pixels[i], &reference.pixels[j]);
            (a.r - b.r).abs() < 1e-4
                && (a.g - b.g).abs() < 1e-4
                && (a.b - b.b).abs() < 1e-4
                && (a.a - b.a).abs() < 1e-4
        };
        let (mut n, mut sum, mut worst, mut over, mut at) =
            (0usize, 0.0f64, 0.0f32, 0usize, (0, 0));
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                let i = y * w + x;
                if !(-1i32..=1)
                    .flat_map(|dy| (-1i32..=1).map(move |dx| (dx, dy)))
                    .all(|(dx, dy)| {
                        same(i, ((y as i32 + dy) as usize) * w + (x as i32 + dx) as usize)
                    })
                {
                    continue;
                }
                let (a, b) = (&mine.pixels[i], &reference.pixels[i]);
                // Measured against the value's own size once it is over
                // one. Light is not bounded here — an exposure of a couple
                // of stops puts a channel at three — and this backend
                // stores its surface as `Rgba16Float`, whose steps in
                // [2, 4) are about a five-hundredth. So an absolute
                // difference on a bright channel is reading the storage
                // rather than the drawing: seed 1589 is a ground under two
                // exposures, and its worst point is 3.0010 against 2.9961,
                // two of those steps and a sixth of a per cent. Below one,
                // where everything a screen shows lives, this is the plain
                // difference it looks like.
                let d = [(a.r, b.r), (a.g, b.g), (a.b, b.b), (a.a, b.a)]
                    .iter()
                    .map(|(u, v)| (u - v).abs() / v.abs().max(1.0))
                    .fold(0.0f32, f32::max);
                n += 1;
                sum += d as f64;
                if d > worst {
                    worst = d;
                    at = (x, y);
                }
                if d > 0.2 {
                    over += 1;
                }
            }
        }
        ((sum / n.max(1) as f64) as f32, worst, over, at)
    }

    /// How many of them are drawn. It was a hundred and twenty, and
    /// turning it up is the cheapest search there is: at two thousand it
    /// found four defects that a hundred and twenty never reached — a
    /// blend applied twice in a stroke band, a copy losing the blend of
    /// what it copies, a mask dropped inside a masked layer with an
    /// effect, and a copy's surface cut to a box that meant "nothing at
    /// all". All four are fixed or declined, so the dial stays where it
    /// paid rather than being turned back down. It costs about half a
    /// minute.
    const SEEDS_AUDITED: u64 = 2000;

    /// Pages nobody wrote, drawn both ways.
    ///
    /// The fixture audit asks this of a document with one of everything
    /// in it, and every combination in it had to be thought of to be put
    /// there. These are drawn from a seed instead — a blend under a mask
    /// inside a faded group, a copy of a layer held to the one under it,
    /// whatever the seed says — so the comparison reaches arrangements
    /// nobody chose. A failure names the seed that found it.
    ///
    /// Two readings, and the second is the one that means "correct".
    ///
    /// A page-wide mean is a poor thing to hold two rasterizers to, for
    /// the reason the export witnesses were rebuilt over: every edge
    /// costs it a little, so it rises as a page gains elements and the
    /// ceiling has to be loosened to let innocent additions through. It
    /// is kept, because how *rough* a page is worth knowing, but what it
    /// is now is a coarseness number.
    ///
    /// What is not allowed to differ is the inside of a shape — a pixel
    /// whose eight neighbours the reference renderer draws in its own
    /// colour, so neither side has an edge to disagree about there. Every
    /// defect this audit has ever found shows up there and none of the
    /// coarseness does: with the four above put back one at a time, the
    /// interiors name 786, 33, 7 and 111 points on their pages, and the
    /// fifth (a *copy* of a group holding text, where what was lost is
    /// glyph-sized and so nearly all edge) raises the interior mean
    /// fiftyfold without a single point crossing the ceiling, which is
    /// why both readings are taken. Meanwhile the two pages this audit
    /// still calls rough — an outline on a path, and a page that is
    /// nothing but edges — do not move the interiors at all.
    ///
    /// The point ceiling is a fifth of full scale, and the reason is the
    /// backend's own resolution: it multisamples four to a pixel, so a
    /// sub-pixel crack between two tessellated pieces can cost a quarter
    /// of one sample, and a quarter of full scale is what that is worth
    /// against a strong colour. Seed 283 is exactly that and sits at
    /// 0.164 — a single pixel of a stroke drawn at three quarters where
    /// the reference draws it whole. Under that ceiling is the backend
    /// being coarse; over it, something is drawn wrongly.
    ///
    /// Both readings measure a channel against its own size once it is
    /// over one, and that correction is worth more than it looks. Light
    /// is not bounded here — two stops of exposure put a channel at three
    /// — and this backend keeps its surface in `Rgba16Float`, whose steps
    /// in [2, 4) are about a five-hundredth. Taken absolutely, a bright
    /// page reads the *storage* rather than the drawing: seed 1589 is a
    /// ground under two exposures and its worst point is 3.0010 against
    /// 2.9961, which is two of those steps and a sixth of a per cent, and
    /// it was the worst interior mean of two thousand pages until it was
    /// measured properly. With the correction the worst mean anywhere is
    /// 0.00214 rather than 0.00496, which is what let the ceiling here be
    /// 0.003 instead of 0.006 — the same evidence, read for what it says.
    /// The two filters that are a function of *where a pixel is* draw
    /// that from the layer's own space, not from the surface.
    ///
    /// Everywhere else on a page the two are the same thing, which is why
    /// this went unnoticed: the backend read a page pixel and the
    /// reference renderer read `Inverse::of(view).at(...)`, and while the
    /// view is the identity those agree exactly. A **copy** is where they
    /// part company — it draws what it copies somewhere else entirely, so
    /// the view it is drawn under carries the copy's placement — and so
    /// is any viewport, which is what this backend exists to serve one
    /// day.
    ///
    /// Found by a page nobody wrote, at the seed where a copy of a noise
    /// filter first appeared: the reference moved the grain with the
    /// copy and the backend left it pinned to the page, and the two
    /// disagreed by a third of full scale at the worst pixel.
    ///
    /// The sharp assertion is not that the two renderers agree — they
    /// would agree on the grain being pinned if both pinned it. It is
    /// that on the reference renderer the copy's patch is the original's
    /// patch *moved*, which is what a copy means, and then that the
    /// backend draws that same page.
    #[test]
    fn a_copy_of_a_filter_carries_where_the_filter_is_measured_from() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        // A square of grain on a flat ground, and a copy of it a whole
        // number of pixels across so that the two patches can be read
        // against each other pixel for pixel.
        const OVER: f32 = 24.0;
        let build = |filter: chitrakar_doc::Filter, copy: bool| {
            let mut doc = Document::new(64, 48, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "ground",
                    VectorShape::Rect {
                        width: 64.0,
                        height: 48.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.45,
                        g: 0.5,
                        b: 0.55,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 1,
                node: Box::new(Node::filter("it", filter)),
            })
            .unwrap();
            let id = doc.children_of(root).unwrap()[1];
            doc.apply(Command::SetMask {
                id,
                mask: Some(Box::new(chitrakar_doc::Mask {
                    kind: chitrakar_doc::MaskKind::Vector {
                        shape: VectorShape::Rect {
                            width: 16.0,
                            height: 16.0,
                            radius: 0.0,
                        },
                        transform: Transform::translation(6.0, 8.0),
                    },
                    invert: false,
                    feather: 0.0,
                })),
            })
            .unwrap();
            if copy {
                doc.apply(Command::AddNode {
                    parent: root,
                    index: 2,
                    node: Box::new(Node::instance("again", id)),
                })
                .unwrap();
                let twin = doc.children_of(root).unwrap()[2];
                doc.apply(Command::SetTransform {
                    id: twin,
                    transform: Transform::translation(OVER, 0.0),
                })
                .unwrap();
            }
            doc
        };
        let grain = chitrakar_doc::Filter::Noise {
            amount: 0.6,
            grain: 2.0,
            mono: true,
            seed: 7717,
        };
        let coloured = chitrakar_doc::Filter::Noise {
            amount: 0.6,
            grain: 2.0,
            mono: false,
            seed: 7717,
        };
        let corners = chitrakar_doc::Filter::Vignette {
            amount: 0.7,
            radius: 0.15,
            softness: 0.4,
        };
        for (what, filter) in [
            ("grain", grain),
            ("grain in three colours", coloured),
            ("a vignette", corners),
        ] {
            let doc = build(filter.clone(), true);
            let theirs = chitrakar_render::render(&doc).unwrap();
            // The reference renderer's own answer first: the copy's
            // patch is the original's patch, moved. This is what says
            // which of the two readings is the right one, and it is a
            // statement about the renderer that is the reference rather
            // than about the two agreeing.
            let over = OVER as u32;
            let mut moved = 0usize;
            for y in 9..23u32 {
                for x in 7..21u32 {
                    let (a, b) = (theirs.get(x, y), theirs.get(x + over, y));
                    assert!(
                        (a.r - b.r).abs() < 0.002
                            && (a.g - b.g).abs() < 0.002
                            && (a.b - b.b).abs() < 0.002,
                        "{what}: the copy is what it copies, moved — \
                         {x},{y} {a:?} against {b:?}"
                    );
                    moved += 1;
                }
            }
            assert!(moved > 100, "{what}: {moved} points compared");
            // And the ground outside both patches is untouched, so what
            // was compared is the filter and not a flat colour.
            let bare = chitrakar_render::render(&build(filter.clone(), false)).unwrap();
            let mut busy = 0usize;
            for y in 9..23u32 {
                for x in 7..21u32 {
                    let (a, b) = (theirs.get(x, y), bare.get(x + over, y));
                    if (a.r - b.r).abs() > 0.01 {
                        busy += 1;
                    }
                }
            }
            assert!(
                busy > 50,
                "{what}: the copy puts {busy} points on a page that had none there"
            );
            // Then the backend, which used to measure both of these from
            // the surface and so drew the copy's patch as whatever the
            // page held at those coordinates.
            assert!(GpuRenderer::can_render(&doc), "{what} is drawn");
            let mine = gpu.render(&doc).unwrap();
            let (mean, worst) = difference(&mine, &theirs);
            assert!(mean < 0.004, "{what}: mean {mean:.5}, worst {worst:.3}");
            for y in 9..23u32 {
                for x in 7..21u32 {
                    for at in [x, x + over] {
                        let (a, b) = (mine.get(at, y), theirs.get(at, y));
                        assert!(
                            (a.r - b.r).abs() < 0.02
                                && (a.g - b.g).abs() < 0.02
                                && (a.b - b.b).abs() < 0.02,
                            "{what} at {at},{y}: {a:?} against {b:?}"
                        );
                    }
                }
            }
        }
    }

    /// A copy of an adjustment held to the layer below goes back to the
    /// reference renderer.
    ///
    /// Being held to something is a coverage, and this backend's masks
    /// ride the one coverage slot a layer already uses, so there is no
    /// pass here for it — the same reason a mask or a fade on such a
    /// copy is handed back. Drawn anyway, what the copy copies was given
    /// a transparent page to change and came back with nothing, so the
    /// copy vanished; and the reference renderer lost it the same way,
    /// so the two agreed and the cross-renderer audits said nothing. It
    /// is fixed there and declined here.
    #[test]
    fn a_copy_of_an_adjustment_held_to_a_shape_goes_back() {
        use chitrakar_doc::{Adjustment, Command, Node, Transform, VectorShape};
        let rect = |name: &str, w: f32, h: f32| {
            let mut n = Node::vector(
                name,
                VectorShape::Rect {
                    width: w,
                    height: h,
                    radius: 0.0,
                },
            );
            if let NodeKind::Vector { fill, .. } = &mut n.kind {
                *fill = Some(chitrakar_color::AuthoredColor::Srgb {
                    r: 0.6,
                    g: 0.5,
                    b: 0.4,
                    a: 1.0,
                });
            }
            Box::new(n)
        };
        let page = |held_copy: bool| {
            let mut doc = chitrakar_doc::Document::new(40, 30, chitrakar_color::ColorMode::Rgb);
            let root = doc.root();
            doc.apply(Command::AddNode {
                parent: root,
                index: 0,
                node: Box::new(Node::adjustment(
                    "the original",
                    Adjustment::BrightnessContrast {
                        brightness: 0.25,
                        contrast: 0.09,
                    },
                )),
            })
            .unwrap();
            let orig = doc.children_of(root).unwrap()[0];
            doc.apply(Command::AddNode {
                parent: root,
                index: 1,
                node: rect("base", 20.0, 20.0),
            })
            .unwrap();
            let mut c = Node::vector(
                "a copy of it",
                VectorShape::Rect {
                    width: 1.0,
                    height: 1.0,
                    radius: 0.0,
                },
            );
            c.kind = NodeKind::Instance {
                of: orig,
                replaces: Vec::new(),
            };
            c.clipped = held_copy;
            c.transform = Transform::translation(2.0, 2.0);
            doc.apply(Command::AddNode {
                parent: root,
                index: 2,
                node: Box::new(c),
            })
            .unwrap();
            doc
        };
        assert!(
            !GpuRenderer::can_render(&page(true)),
            "a copy of an adjustment held to a shape goes back"
        );
        // And the same page with the copy not held is still drawn, so
        // this is not declining copies of adjustments wholesale.
        assert!(
            GpuRenderer::can_render(&page(false)),
            "one that is not held is still drawn"
        );
    }

    /// A layer held to the one below it keeps its *effects* inside it
    /// too.
    ///
    /// A layer's own mask and what it is held to ride the one coverage
    /// slot here, and both were folded into the drawing and then left
    /// off the pass that lays the surface down — right for a mask,
    /// whose job is to shape what the effects grow from (a shadow of a
    /// masked shape falls outside the mask, as it should), and wrong
    /// for a clip, which cuts what the layer lays down with its effects
    /// in it. So a held layer drew its outline outside the layer it was
    /// held to, where the reference renderer draws none. The lay-down
    /// keeps a coverage of its own now, built from what holds the layer
    /// back and not from its mask.
    ///
    /// Found on a page nobody wrote, once those pages began turning
    /// layers — though nothing here is turned: the turn only reshuffled
    /// the seeds until a held layer with an effect on it came up.
    #[test]
    fn a_held_layer_keeps_its_effects_inside_what_holds_it() {
        use chitrakar_doc::{Command, Effect, Transform, VectorShape};
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let mut doc = chitrakar_doc::Document::new(60, 40, chitrakar_color::ColorMode::Rgb);
        let root = doc.root();
        let rect = |name: &str, w: f32, h: f32, c: [f32; 3]| {
            let mut n = chitrakar_doc::Node::vector(
                name,
                VectorShape::Rect {
                    width: w,
                    height: h,
                    radius: 0.0,
                },
            );
            if let NodeKind::Vector { fill, .. } = &mut n.kind {
                *fill = Some(chitrakar_color::AuthoredColor::Srgb {
                    r: c[0],
                    g: c[1],
                    b: c[2],
                    a: 1.0,
                });
            }
            Box::new(n)
        };
        doc.apply(Command::AddNode {
            parent: root,
            index: 0,
            node: rect("ground", 60.0, 40.0, [0.1, 0.1, 0.12]),
        })
        .unwrap();
        // A base at x 10..30, and a layer held to it running x 20..40, so
        // half of what is held hangs off the side of what holds it.
        doc.apply(Command::AddNode {
            parent: root,
            index: 1,
            node: rect("base", 20.0, 24.0, [0.2, 0.35, 0.8]),
        })
        .unwrap();
        let base = doc.children_of(root).unwrap()[1];
        doc.apply(Command::SetTransform {
            id: base,
            transform: Transform::translation(10.0, 8.0),
        })
        .unwrap();
        doc.apply(Command::AddNode {
            parent: root,
            index: 2,
            node: rect("held", 20.0, 12.0, [0.95, 0.85, 0.2]),
        })
        .unwrap();
        let held = doc.children_of(root).unwrap()[2];
        doc.apply(Command::SetTransform {
            id: held,
            transform: Transform::translation(20.0, 14.0),
        })
        .unwrap();
        doc.apply(Command::SetClipped {
            id: held,
            clipped: true,
        })
        .unwrap();
        doc.apply(Command::SetEffects {
            id: held,
            effects: vec![Effect::Outline {
                width: 3.0,
                color: chitrakar_color::AuthoredColor::Srgb {
                    r: 0.1,
                    g: 0.9,
                    b: 0.2,
                    a: 1.0,
                },
                opacity: 1.0,
            }],
        })
        .unwrap();
        assert!(
            GpuRenderer::can_render(&doc),
            "this backend draws a held layer with an effect"
        );
        let mine = gpu.render(&doc).expect("can_render said it would");
        let reference = chitrakar_render::render(&doc).unwrap();
        let green = |s: &Surface, x: u32, y: u32| {
            let p = s.pixels[(y * s.width + x) as usize];
            p.g > 0.4 && p.r < 0.3
        };
        // Non-vacuity: the outline has to be drawn at all, and inside
        // what holds the layer, or the question below asks nothing.
        assert!(
            (8..32).any(|y| green(&reference, 21, y)) || green(&reference, 21, 20),
            "the outline is drawn inside what holds the layer"
        );
        for (name, surf) in [
            ("the reference renderer", &reference),
            ("this backend", &mine),
        ] {
            for (x, y) in [(31u32, 20u32), (33, 20), (31, 26), (35, 20)] {
                assert!(
                    !green(surf, x, y),
                    "{name} keeps the outline inside what holds the layer, and \
                     does not draw it at {x},{y}"
                );
            }
        }
        // And the page at large, read the way the audits over these two
        // renderers read one: the mean, and the inside of every shape,
        // where neither side has an edge to disagree about. A hard clip
        // edge is a pixel the two antialias their own way, which is what
        // the interior measure exists to look past.
        let (mean, _) = difference(&mine, &reference);
        let (_, in_worst, over, at) = interiors(&mine, &reference);
        assert!(
            mean < 0.002 && over == 0,
            "and draws the page the reference renderer draws (mean {mean:.5}, \
             {over} interior points off, the worst by {in_worst:.3} at {at:?})"
        );
    }

    #[test]
    fn pages_nobody_wrote_are_drawn_the_way_the_cpu_draws_them() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let (mut drawn, mut declined) = (0usize, 0usize);
        let mut worst_seen = 0.0f64;
        let mut worst_interior = 0.0f32;
        let mut rough: Vec<u64> = Vec::new();
        // Pages whose mean is over the level this audit used to refuse
        // outright. There is exactly one, and the count is asserted
        // below rather than the level alone — see the note there.
        let mut drifted: Vec<(u64, f64)> = Vec::new();
        for seed in 0..SEEDS_AUDITED {
            let doc = chitrakar_doc::fixture::page(seed);
            if !GpuRenderer::can_render(&doc) {
                declined += 1;
                continue;
            }
            let mine = gpu.render(&doc).expect("can_render said it would");
            let reference = chitrakar_render::render(&doc).unwrap();
            let (mean, worst) = difference(&mine, &reference);
            worst_seen = worst_seen.max(worst);
            if worst > 0.05 {
                rough.push(seed);
            }
            if mean >= 0.004 {
                drifted.push((seed, mean));
            }
            assert!(
                mean < 0.01,
                "seed {seed}: mean {mean:.5}, worst pixel off by {worst:.3}"
            );
            // And the inside of every shape, where neither side has an
            // edge to disagree about.
            let (in_mean, in_worst, over, at) = interiors(&mine, &reference);
            assert!(
                over == 0,
                "seed {seed}: {over} points inside a shape are drawn differently, \
                 the worst by {in_worst:.3} at {at:?} — which is past what four \
                 samples a pixel can account for, so something is drawn wrongly \
                 rather than coarsely (gpu {:?} against {:?})",
                mine.pixels[at.1 * mine.width as usize + at.0].to_srgb8(),
                reference.pixels[at.1 * mine.width as usize + at.0].to_srgb8()
            );
            assert!(
                in_mean < 0.003,
                "seed {seed}: the insides of its shapes are {in_mean:.5} apart on \
                 average, worst {in_worst:.3} at {at:?}. No single point need be \
                 far out for a layer to be losing part of itself — a copy of a \
                 group holding text did exactly that at 0.00891, with nothing \
                 over the point ceiling at all."
            );
            worst_interior = worst_interior.max(in_worst);
            drawn += 1;
        }
        // The page mean is a coarse net and it drifts, which is the same
        // thing the SVG witness says about its own: a page that amplifies
        // its edges pushes it up without anything being drawn wrongly.
        // Exactly one page of these two thousand does — seed 1529, at
        // 0.00737 — and it was worth running to ground rather than
        // tolerating. It holds a self-intersecting path whose long thin
        // wedge is nearly all edge, drawn under a Difference blend
        // against a strongly contrasting ground, then sharpened (which
        // multiplies a difference by one and a half) and then given more
        // contrast. That path *alone* on the same ground reads 0.00103.
        // So what the number says is four samples a pixel against an
        // exact area, put through three amplifiers — not a layer drawn
        // in the wrong place, which is what the interiors above are for
        // and which they say nothing about on any of these pages.
        //
        // So the level moved and the *count* is what is held, which is
        // the stronger of the two: a change that makes twenty pages
        // drift a little is caught here even though each of them stays
        // well inside the level.
        assert!(
            drifted.len() <= 2,
            "{} pages are over 0.004: {:?} — the level is a coarse net and \
             one page of two thousand sits above it; several would mean \
             something changed rather than one page amplifying its edges",
            drifted.len(),
            &drifted[..drifted.len().min(8)]
        );
        // A backend that declined everything would pass without drawing
        // a thing, and one that drew only the empty pages would too.
        assert!(
            drawn > 25,
            "the backend drew {drawn} of them and declined {declined}"
        );
        // A ratchet on what is left rather than a tolerance, and what is
        // left is an edge drawn two ways. Many of these pages hold
        // a *path*, which this backend fills through a stencil and
        // antialiases by multisampling it — four samples a pixel, which
        // is the only count WebGPU makes every adapter offer and the only
        // one this one accepts (eight and sixteen are refused outright).
        // So a path's edge comes out in quarters here, where the renderer
        // being matched is exact across a row and sixteen deep down it.
        // Closing that means supersampling the whole page or handing the
        // backend the other's answer, and neither is a small change.
        // The rest are a *curved* edge, which is as good as a distance
        // can make it: a straight edge's exact area, taken at the
        // distance and facing of the curve, which is first-order right
        // and no more (`a_shapes_edge_is_the_area_it_covers`,
        // `a_strokes_band_is_the_area_it_covers` hold the numbers).
        //
        // And the worst pixel is a poor headline for this, which was
        // worth finding out. It sat at 0.358 through three rounds of
        // real improvement without moving, so it was chased: one page,
        // one layer, undressed a thing at a time. It is a stroked disc
        // whose *blend* is Difference — take the blend off and the same
        // layer is 0.032, put it back and it is 0.330, and its mask has
        // nothing to do with it. A blend that is not Normal reads an
        // unpremultiplied colour, which divides by the coverage, so a
        // thirtieth of a pixel of disagreement at a barely-covered edge
        // comes out a third of full scale. Both renderers do the same
        // arithmetic; the amplification is not a defect in either. So
        // the number to read here is how many pages are rough and the
        // mean inside each, not the worst pixel on the worst page.
        //
        // A shape's *fill* used to be here and is not any more, and
        // neither of the two things wrong with it was what it looked
        // like. `fwidth` is |dx| + |dy| where the rate of change across
        // a pixel is the gradient's length: the same number only when
        // one partial is zero, and root two out on an edge at forty-five
        // degrees, so every ramp scaled by it was that much too wide.
        // And an ellipse's first-order distance is singular at its
        // middle, which left a stray pixel three-quarters covered in the
        // middle of a solid disc — the worst disagreement anywhere
        // between the two renderers, not on an edge at all, and so
        // invisible to every way of looking for it as one. Nine of the
        // rough pages came clean with the two of them
        // (`a_shapes_edge_is_the_area_it_covers`). A picture's border used to be a third
        // cause and is not any more: it was the *same* quantization, the
        // quad's own edge caught by four samples, and it went once the
        // quad grew a device pixel and the shader faded it by the area
        // instead — eight of the rough pages came clean with it, 42 to
        // 34, which is what says the cause was really that.
        //
        // The numbers go *up* when the backend learns something, because
        // learning it means more pages are compared rather than declined.
        // Five passes of asking which single layer, taken away, makes a
        // declined page drawable took the pages drawn from 33 to 88 of
        // 120: isolating a copy, ignoring a blend on an adjustment rather
        // than refusing the page, letting a clipped layer be held to a
        // base that is faded, masked or blended, letting it be held to
        // anything that *paints* rather than only a shape, a picture or a
        // block of text, and drawing a copy of a clipped layer whole the
        // way the reference does. Twenty-three more rough pages came with
        // those fifty-five, and every one of them carries one of the three
        // causes above — checked rather than assumed: take the blend back
        // off, undress the base, or let the clip go, and the worst pixel
        // is unchanged to three decimal places, so the roughness was
        // already in the page and only the comparison is new. (On one of
        // them letting the clip go made it *worse*, the clip having been
        // hiding rough pixels, which is the same answer said louder.) A
        // rise here is good news when what is drawn rises with it and bad
        // news otherwise, which is why the two are printed together.
        // Both of these were counts taken over a hundred and twenty pages,
        // and a count is not a property of the renderers — it is a
        // property of how many pages were looked at. With the dial at two
        // thousand, "how many pages are rough" has to be a *proportion* or
        // it says nothing, and "the worst pixel anywhere" is a maximum
        // over sixteen times as many samples and so is naturally larger:
        // 0.37 over a hundred and twenty, 0.607 over two thousand. Neither
        // number got worse; both were re-based, and saying so is the point,
        // because a loosened ceiling that is not explained is exactly how
        // an audit quietly stops catching things.
        //
        // What makes the re-basing safe rather than a retreat is that the
        // claim these two used to carry has moved to the interiors above,
        // where it is stated per point and does not drift with the page
        // count at all. These two are the coarseness of an edge drawn two
        // ways: worth watching, not worth calling correctness. A rise here
        // is good news when what is drawn rises with it and bad news
        // otherwise, which is why the two are printed together.
        // The worst pixel moved from 0.607 to 0.659 when these pages
        // gained the four filters they had never drawn, and it is the
        // same page and the same reason as the drift note above: seed
        // 1529's self-intersecting path, nearly all edge, under a
        // Difference blend and then a sharpen. A maximum over two
        // thousand pages is the one reading that a single amplifying
        // page owns outright, which is why it is watched rather than
        // trusted.
        let rough_pct = rough.len() * 100 / drawn.max(1);
        assert!(
            rough_pct <= 27 && worst_seen < 0.70,
            "{} of {drawn} pages have a pixel more than a twentieth off ({rough_pct}%), \
             worst {worst_seen:.3}",
            rough.len()
        );
        eprintln!(
            "gpu drew {drawn} random pages, declined {declined}; rough {} ({rough_pct}%), \
             worst pixel {worst_seen:.3}, worst inside a shape {worst_interior:.4}",
            rough.len()
        );
    }

    /// A copy standing in for a layer of a group that reads what is
    /// under it goes back, and the same page without the reading is
    /// drawn the way the CPU renderer draws it.
    ///
    /// What a copy with stand-ins draws is a list of layers rather than
    /// the group, and a group holding an adjustment is isolated so that
    /// the adjustment reaches its neighbours and nothing beneath. This
    /// pass lays that list straight down, which is a different picture,
    /// so the one it cannot draw it says so about.
    #[test]
    fn a_copy_standing_in_where_the_group_reads_the_backdrop_goes_back() {
        let square = VectorShape::Rect {
            width: 20.0,
            height: 20.0,
            radius: 0.0,
        };
        let green = AuthoredColor::Srgb {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        };
        // A ground, a group of one mark, and a copy of it standing in for
        // that mark with one of its own.
        let page = |reading: bool| {
            let mut doc = Document::new(120, 50, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "ground",
                    VectorShape::Rect {
                        width: 120.0,
                        height: 50.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.6,
                        g: 0.6,
                        b: 0.6,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            let master = add(
                &mut doc,
                Box::new(Node::group("badge")),
                Transform::translation(10.0, 10.0),
            );
            doc.apply(Command::AddNode {
                parent: master,
                index: 0,
                node: filled("mark", square.clone(), RED),
            })
            .unwrap();
            if reading {
                doc.apply(Command::AddNode {
                    parent: master,
                    index: 1,
                    node: Box::new(Node::adjustment(
                        "two stops down",
                        chitrakar_doc::Adjustment::Exposure { stops: -2.0 },
                    )),
                })
                .unwrap();
            }
            let copy = add(
                &mut doc,
                Box::new(Node::instance("a copy that differs", master)),
                Transform::translation(70.0, 10.0),
            );
            doc.apply(Command::AddNode {
                parent: copy,
                index: 0,
                node: filled("its own mark", square.clone(), green.clone()),
            })
            .unwrap();
            doc.apply(Command::SetKind {
                id: copy,
                kind: Box::new(NodeKind::Instance {
                    of: master,
                    replaces: vec![doc.children_of(master).unwrap()[0]],
                }),
            })
            .unwrap();
            doc
        };
        assert!(
            !GpuRenderer::can_render(&page(true)),
            "a copy standing in where the group reads what is under it"
        );
        let plain = page(false);
        assert!(
            GpuRenderer::can_render(&plain),
            "and without the reading it is an ordinary copy"
        );
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let (mean, worst) = difference(
            &gpu.render(&plain).unwrap(),
            &chitrakar_render::render(&plain).unwrap(),
        );
        assert!(mean < 0.001, "a copy that differs: {mean}, worst {worst:?}");
    }

    /// Ink authored for a press, in every place a colour can be written
    /// on a page: a fill, a stroke, a stop in a gradient, a block of
    /// text, the tint of a shadow, and a swatch whose name means an ink.
    ///
    /// This backend used to hand the whole page back for any of them,
    /// and the reason written down was that such ink "resolves through
    /// the document's profile, which is the CPU's business". The first
    /// half is true and the second does not follow. Resolving an
    /// authored colour is one question per colour with one answer per
    /// document — a fill is one colour, and a gradient's stops are
    /// resolved once into a ramp either way — so it happens on the CPU
    /// in *both* renderers, and the way to keep the two from drifting is
    /// for both to call the same function. `chitrakar_render::resolve_color`
    /// is that function, and this backend now asks it rather than
    /// declining.
    ///
    /// Asked twice: once with no press profile, where both fall back to
    /// the device formulas, and once with a real one, where the ICC
    /// transform decides. The second run asserts the profile *moved* the
    /// picture before it asserts the two renderers agree about it —
    /// otherwise a backend quietly ignoring the profile would pass, the
    /// two device-formula answers being identical.
    #[test]
    fn ink_authored_for_a_press_lands_where_the_cpu_lands_it() {
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        let ink = |c, m, y, k| AuthoredColor::Cmyk { c, m, y, k, a: 1.0 };
        let rect = |w: f32, h: f32| VectorShape::Rect {
            width: w,
            height: h,
            radius: 0.0,
        };
        let build = || {
            let mut doc = Document::new(120, 80, ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "ground",
                    rect(120.0, 80.0),
                    AuthoredColor::Srgb {
                        r: 0.55,
                        g: 0.55,
                        b: 0.55,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            // A fill and a stroke, both in ink.
            let mut inked = Node::vector("inked", rect(30.0, 24.0));
            if let NodeKind::Vector { fill, stroke, .. } = &mut inked.kind {
                *fill = Some(ink(0.85, 0.2, 0.1, 0.0));
                *stroke = Some(chitrakar_doc::Stroke {
                    color: ink(0.0, 0.9, 0.85, 0.05),
                    width: 3.0,
                    widths: Vec::new(),
                    dash: Vec::new(),
                    cap: Default::default(),
                    join: Default::default(),
                    align: None,
                    start_marker: Default::default(),
                    end_marker: Default::default(),
                });
            }
            add(&mut doc, Box::new(inked), Transform::translation(6.0, 6.0));
            // A ramp with one plain stop and one in ink.
            let mut ramped = Node::vector("ramped", rect(30.0, 24.0));
            if let NodeKind::Vector { gradient, .. } = &mut ramped.kind {
                *gradient = Some(chitrakar_doc::Gradient::Linear {
                    from: [0.0, 0.0],
                    to: [1.0, 0.0],
                    stops: ramp(&[(0.0, RED), (1.0, ink(0.9, 0.7, 0.0, 0.1))]),
                });
            }
            add(
                &mut doc,
                Box::new(ramped),
                Transform::translation(44.0, 6.0),
            );
            // Text set in ink.
            add(
                &mut doc,
                texted("words", "Press", 22.0, |spec| {
                    spec.fill = ink(0.1, 0.95, 0.9, 0.0);
                }),
                Transform::translation(6.0, 38.0),
            );
            // A shadow tinted with ink, which is the other place a colour
            // reaches the page — through an effect rather than a layer.
            let lit = add(
                &mut doc,
                filled("lit", rect(20.0, 16.0), RED),
                Transform::translation(70.0, 44.0),
            );
            doc.apply(Command::SetEffects {
                id: lit,
                effects: vec![chitrakar_doc::Effect::DropShadow {
                    dx: 4.0,
                    dy: 4.0,
                    blur: 1.5,
                    color: ink(0.95, 0.85, 0.0, 0.0),
                    opacity: 0.9,
                }],
            })
            .unwrap();
            // And a swatch whose name means an ink: a colour is reached
            // for by name here, and reading past the name is the same
            // question one layer down.
            add(
                &mut doc,
                filled(
                    "spot",
                    rect(18.0, 14.0),
                    ink(0.05, 0.15, 0.95, 0.0).standing_for("spot"),
                ),
                Transform::translation(96.0, 6.0),
            );
            doc
        };

        // Somewhere inside each of the six, and one patch of bare ground
        // to say the page is not simply all one colour.
        let probes = [
            ("fill", 18u32, 16u32),
            ("stroke", 7, 16),
            ("ramp", 70, 16),
            ("text", 14, 48),
            ("shadow", 92, 62),
            ("swatch", 104, 12),
        ];
        let ground = AuthoredColor::Srgb {
            r: 0.55,
            g: 0.55,
            b: 0.55,
            a: 1.0,
        };
        let bare = chitrakar_color::to_working(&ground);

        let doc = build();
        assert!(
            GpuRenderer::can_render(&doc),
            "ink authored for a press is drawn rather than handed back"
        );
        let plain_cpu = chitrakar_render::render(&doc).unwrap();
        let plain_gpu = gpu.render(&doc).unwrap();
        for (what, x, y) in probes {
            let px = plain_cpu.get(x, y);
            assert!(
                (px.r - bare.r).abs() + (px.g - bare.g).abs() + (px.b - bare.b).abs() > 0.02,
                "{what} put something on the page at {x},{y}: {px:?}"
            );
            let mine = plain_gpu.get(x, y);
            assert!(
                (mine.r - px.r).abs() < 0.02
                    && (mine.g - px.g).abs() < 0.02
                    && (mine.b - px.b).abs() < 0.02,
                "{what} with no profile at {x},{y}: {mine:?} against {px:?}"
            );
        }
        let (mean, worst) = difference(&plain_gpu, &plain_cpu);
        assert!(
            mean < 0.004,
            "ink with no profile: mean {mean:.5} (worst {worst:.3})"
        );

        // And again through a real press profile, where the ICC
        // transform rather than the device formula says what the ink is.
        let Ok(path) = std::env::var("CHITRAKAR_TEST_CMYK_ICC") else {
            eprintln!("skipped the profiled half: set CHITRAKAR_TEST_CMYK_ICC to run it");
            return;
        };
        let icc = std::fs::read(path).expect("the profile named by CHITRAKAR_TEST_CMYK_ICC");
        let mut pressed = build();
        pressed.set_cmyk_profile(icc).unwrap();
        assert!(GpuRenderer::can_render(&pressed));
        let press_cpu = chitrakar_render::render(&pressed).unwrap();
        let press_gpu = gpu.render(&pressed).unwrap();
        // The profile has to be deciding something, or the run below
        // proves nothing: a backend ignoring it would agree with a CPU
        // that was also ignoring it.
        let (moved, _) = difference(&press_cpu, &plain_cpu);
        assert!(
            moved > 0.01,
            "the press profile moved the picture: mean {moved:.5}"
        );
        for (what, x, y) in probes {
            let px = press_cpu.get(x, y);
            let mine = press_gpu.get(x, y);
            assert!(
                (mine.r - px.r).abs() < 0.02
                    && (mine.g - px.g).abs() < 0.02
                    && (mine.b - px.b).abs() < 0.02,
                "{what} through the profile at {x},{y}: {mine:?} against {px:?}"
            );
        }
        let (mean, worst) = difference(&press_gpu, &press_cpu);
        assert!(
            mean < 0.004,
            "ink through a press profile: mean {mean:.5} (worst {worst:.3})"
        );
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

        // An effect asking for more than a pass will do, or ink authored
        // for a press: either on its own is enough to hand the page
        // back. A stroke is not — that one it draws, and nor is a blend
        // mode any more, nor a shadow, nor an outline of a width two
        // passes can measure out.
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

        let outlined = |width: f32| {
            let mut doc = doc.clone();
            doc.apply(Command::SetEffects {
                id,
                effects: vec![chitrakar_doc::Effect::Outline {
                    color: BLUE,
                    width,
                    opacity: 1.0,
                }],
            })
            .unwrap();
            doc
        };
        assert!(GpuRenderer::can_render(&outlined(2.0)));
        // Wider than either of the band's passes will walk, which is the
        // cap a smear is held to said in the same taps.
        assert!(!GpuRenderer::can_render(&outlined(400.0)));

        // A layer held to the one under it is drawn, since that layer's
        // own alpha is a coverage like a mask's — and that reading comes
        // from the renderer being matched, so the base's own opacity,
        // mask and blend are already in the number both sides read.
        // Faded, masked or blended, the base is fine.
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
        for dress in [
            Command::SetOpacity { id, opacity: 0.5 },
            Command::SetBlendMode {
                id,
                blend: BlendMode::Multiply,
            },
        ] {
            let mut dressed = held.clone();
            let what = format!("{dress:?}");
            dressed.apply(dress).unwrap();
            assert!(
                GpuRenderer::can_render(&dressed),
                "held to a base wearing {what}"
            );
        }
        // An *effect* on the base is the one that still goes back: the
        // coverage then carries the shadow the base casts, and what the
        // CPU renderer holds the layer above to does not.
        let mut lit = held.clone();
        lit.apply(Command::SetEffects {
            id,
            effects: vec![chitrakar_doc::Effect::DropShadow {
                dx: 2.0,
                dy: 2.0,
                blur: 1.0,
                color: BLUE,
                opacity: 0.9,
            }],
        })
        .unwrap();
        assert!(
            !GpuRenderer::can_render(&lit),
            "held to a base that casts a shadow"
        );

        // Ink authored for a press is drawn now rather than handed back
        // — a colour is resolved once, on the CPU, by the function the
        // reference renderer resolves its own with, so there is nothing
        // for a second renderer to guess at. See
        // `ink_authored_for_a_press_lands_where_the_cpu_lands_it`.
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
        assert!(GpuRenderer::can_render(&pressed));

        // A hidden layer it cannot draw is no obstacle: it is not drawn.
        let mut hidden = outlined(400.0);
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

    /// A layer's own mask and a mask inside it want the same slot, and
    /// where an effect is asking for the silhouette they cannot share it.
    ///
    /// One coverage texture. An effect is built from the layer's
    /// silhouette and the layer's own mask is what decides that
    /// silhouette, so the mask rides the slot for the silhouette pass; a
    /// mask on something *inside* the layer wants the same slot and does
    /// not get it. What that cost was not a softer shadow but the child's
    /// mask dropped altogether — this backend's answer for such a page was
    /// the reference renderer's answer for the same page *with the child's
    /// mask removed*, pixel for pixel.
    ///
    /// So the page goes back, which is the same answer a stroke carrying a
    /// region gets on a layer whose own mask is already on that slot.
    ///
    /// Three things are asserted and the second two are what keep the limit
    /// honest: that the page is refused; that either mask *alone* is still
    /// drawn, and drawn the way the reference renderer draws it, so the
    /// refusal is no wider than the collision; and that the two pictures
    /// really are different, so what is being declined is a wrong answer
    /// rather than a scruple. Found at seed 4898 of the random pages, which
    /// is three of them.
    #[test]
    fn a_mask_inside_a_masked_layer_with_effects_goes_back() {
        let at = Transform::translation(14.0, 10.0);
        let region = |w: f32, h: f32, x: f32, y: f32| chitrakar_doc::Mask {
            kind: chitrakar_doc::MaskKind::Vector {
                shape: VectorShape::Rect {
                    width: w,
                    height: h,
                    radius: 0.0,
                },
                transform: Transform::translation(x, y),
            },
            invert: false,
            feather: 0.0,
        };
        // `inside` masks the child, `outside` masks the group holding it.
        let build = |inside: bool, outside: bool| {
            let mut doc = Document::new(48, 36, chitrakar_color::ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "ground",
                    VectorShape::Rect {
                        width: 48.0,
                        height: 36.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.2,
                        g: 0.6,
                        b: 0.75,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            let group = add(
                &mut doc,
                Box::new(Node::group("holds it")),
                Transform::default(),
            );
            let mut child = filled(
                "child",
                VectorShape::Ellipse { rx: 9.0, ry: 6.0 },
                AuthoredColor::Srgb {
                    r: 0.85,
                    g: 0.35,
                    b: 0.2,
                    a: 1.0,
                },
            );
            if inside {
                child.mask = Some(region(14.0, 30.0, 8.0, 4.0));
            }
            doc.apply(Command::AddNode {
                parent: group,
                index: 0,
                node: child,
            })
            .unwrap();
            let kid = doc.children_of(group).unwrap()[0];
            doc.apply(Command::SetTransform {
                id: kid,
                transform: at,
            })
            .unwrap();
            if outside {
                doc.apply(Command::SetMask {
                    id: group,
                    mask: Some(Box::new(region(30.0, 14.0, 4.0, 12.0))),
                })
                .unwrap();
            }
            // The shadow is what asks for the silhouette, and so what
            // turns two masks into a collision.
            doc.apply(Command::SetEffects {
                id: group,
                effects: vec![chitrakar_doc::Effect::DropShadow {
                    dx: 4.0,
                    dy: 3.0,
                    blur: 2.5,
                    color: AuthoredColor::Srgb {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    },
                    opacity: 0.85,
                }],
            })
            .unwrap();
            doc
        };

        let both = build(true, true);
        assert!(
            !GpuRenderer::can_render(&both),
            "a mask inside a masked layer with an effect has to go back: \
             one coverage texture, and the child's mask is what gets dropped"
        );
        // Declining is only worth anything because the two pictures differ.
        // What this backend drew was the page without the child's mask, so
        // that is what the difference is measured against.
        let as_if_dropped = build(false, true);
        let (mean, worst) = difference(
            &chitrakar_render::render(&both).unwrap(),
            &chitrakar_render::render(&as_if_dropped).unwrap(),
        );
        assert!(
            mean > 0.01 && worst > 0.5,
            "the child's mask has to matter to the picture or there is \
             nothing to decline: mean {mean:.5}, worst {worst:.3}"
        );

        // And no wider than the collision: either mask alone is drawn, and
        // drawn the way the reference renderer draws it.
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        for (inside, outside, what) in [
            (true, false, "a mask inside it and none of its own"),
            (false, true, "a mask of its own and none inside it"),
        ] {
            let doc = build(inside, outside);
            assert!(
                GpuRenderer::can_render(&doc),
                "a layer with an effect and {what} is still drawn"
            );
            let (mean, worst) = difference(
                &gpu.render(&doc).unwrap(),
                &chitrakar_render::render(&doc).unwrap(),
            );
            assert!(
                mean < 0.004,
                "with {what} it draws what the reference draws: \
                 mean {mean:.5}, worst {worst:.3}"
            );
        }
    }

    /// A frame casts the shadow of its own rectangle, on both sides.
    ///
    /// A frame with an effect used to be handed back, and the reason
    /// written down was that the silhouette here would be "the surface
    /// uncut" — a child sticking out past the frame's edge casting a
    /// shadow the frame's own rectangle would not. That reason was
    /// wrong. A frame's contents are collected with its rectangle as
    /// their bound, so the surface holds them already cut, and the
    /// silhouette is the frame.
    ///
    /// What was really wrong was plainer: the frame arm returned before
    /// the end of the pass, and the surface is brought down after it. So
    /// a frame with an effect opened a surface nothing ever closed —
    /// no shadow anywhere, and the frame's own pixels wrong besides.
    /// Nothing was ever wrong on a page, because the page was declined;
    /// but it was declined for a reason nobody had checked.
    ///
    /// The overhang is the assertion that matters. With a child half
    /// again the frame's size inside it, the shadow has to be the same
    /// shadow as with no child at all — on *both* renderers, which is
    /// what says the two agree about the frame being the silhouette
    /// rather than agreeing about something else.
    #[test]
    fn a_frame_casts_the_shadow_of_its_own_rectangle() {
        let build = |overhang: bool, cast: bool| {
            let mut doc = Document::new(64, 48, chitrakar_color::ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "ground",
                    VectorShape::Rect {
                        width: 64.0,
                        height: 48.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.85,
                        g: 0.87,
                        b: 0.9,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            let frame = add(
                &mut doc,
                Box::new(Node::artboard(
                    "frame",
                    20.0,
                    14.0,
                    Some(AuthoredColor::Srgb {
                        r: 0.95,
                        g: 0.95,
                        b: 0.95,
                        a: 1.0,
                    }),
                )),
                Transform::translation(10.0, 10.0),
            );
            if overhang {
                doc.apply(Command::AddNode {
                    parent: frame,
                    index: 0,
                    node: filled(
                        "sticks out",
                        VectorShape::Rect {
                            width: 40.0,
                            height: 30.0,
                            radius: 0.0,
                        },
                        AuthoredColor::Srgb {
                            r: 0.2,
                            g: 0.4,
                            b: 0.9,
                            a: 1.0,
                        },
                    ),
                })
                .unwrap();
            }
            if cast {
                doc.apply(Command::SetEffects {
                    id: frame,
                    effects: vec![chitrakar_doc::Effect::DropShadow {
                        dx: 5.0,
                        dy: 5.0,
                        blur: 1.0,
                        color: AuthoredColor::Srgb {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        },
                        opacity: 1.0,
                    }],
                })
                .unwrap();
            }
            doc
        };
        // Where the effect put ink, as a set of pixels, so two shadows
        // can be compared as shapes rather than as pages.
        let cast_of = |lit: &Surface, plain: &Surface| -> Vec<bool> {
            lit.pixels
                .iter()
                .zip(&plain.pixels)
                .map(|(p, q)| {
                    (p.r - q.r)
                        .abs()
                        .max((p.g - q.g).abs())
                        .max((p.b - q.b).abs())
                        .max((p.a - q.a).abs())
                        > 0.01
                })
                .collect()
        };

        let by_cpu = |overhang: bool| {
            cast_of(
                &chitrakar_render::render(&build(overhang, true)).unwrap(),
                &chitrakar_render::render(&build(overhang, false)).unwrap(),
            )
        };
        let (bare, over) = (by_cpu(false), by_cpu(true));
        let inked = bare.iter().filter(|b| **b).count();
        assert!(
            inked > 100,
            "the shadow has to be worth comparing: {inked} pixels"
        );
        assert_eq!(
            bare, over,
            "a child bigger than the frame changed the frame's shadow. A \
             frame cuts its contents to itself before anything is made of \
             them, so its silhouette is its own rectangle."
        );

        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        for overhang in [false, true] {
            let (lit, plain) = (build(overhang, true), build(overhang, false));
            assert!(
                GpuRenderer::can_render(&lit),
                "a frame with an effect is drawn (overhang {overhang})"
            );
            let mine = cast_of(&gpu.render(&lit).unwrap(), &gpu.render(&plain).unwrap());
            let theirs = if overhang { &over } else { &bare };
            let apart = mine.iter().zip(theirs).filter(|(a, b)| a != b).count();
            assert!(
                apart <= 2,
                "with overhang {overhang} the backend's shadow differs from \
                 the reference's in {apart} pixels"
            );
            let (mean, worst) = difference(
                &gpu.render(&lit).unwrap(),
                &chitrakar_render::render(&lit).unwrap(),
            );
            assert!(
                mean < 0.004,
                "overhang {overhang}: mean {mean:.5}, worst {worst:.3}"
            );
        }
    }

    /// An effect on a clone layer goes back, and now it has to.
    ///
    /// A clone layer paints with what is under it, so it is never on a
    /// surface of its own, and an effect is built from a silhouette this
    /// backend has no pass to make for one. That was always a safe
    /// answer. It was not, until now, a *necessary* one: the reference
    /// renderer was dropping the effect in silence too, so both drew the
    /// same page and the refusal cost a page it could have drawn for
    /// nothing. It builds the silhouette out of the strokes the layer
    /// lays now, so there is a shadow here to disagree about.
    ///
    /// Three things, and the last two are what keep the limit honest:
    /// that the page goes back; that the same layer without the effect is
    /// still drawn, and drawn the reference's way, so the refusal is no
    /// wider than it needs; and that the effect really changes the
    /// picture, so what is declined is a disagreement rather than a
    /// scruple.
    #[test]
    fn an_effect_on_a_clone_layer_goes_back() {
        let build = |cast: bool| {
            let mut doc = Document::new(48, 36, chitrakar_color::ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "ground",
                    VectorShape::Rect {
                        width: 48.0,
                        height: 36.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.75,
                        g: 0.8,
                        b: 0.85,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            // Something worth lifting: a clone of a uniform ground puts
            // back what was already there and has no silhouette at all.
            add(
                &mut doc,
                filled(
                    "patch",
                    VectorShape::Rect {
                        width: 20.0,
                        height: 12.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.9,
                        g: 0.25,
                        b: 0.15,
                        a: 1.0,
                    },
                ),
                Transform::translation(4.0, 20.0),
            );
            let lifting = add(
                &mut doc,
                Box::new(Node::clone_layer("borrowed")),
                Transform::default(),
            );
            doc.apply(Command::AddStroke {
                id: lifting,
                index: 0,
                on_mask: false,
                stroke: Box::new(chitrakar_doc::PaintStroke {
                    points: vec![[10.0, 10.0], [26.0, 16.0]],
                    radii: vec![4.0],
                    color: AuthoredColor::Srgb {
                        r: 0.1,
                        g: 0.1,
                        b: 0.1,
                        a: 1.0,
                    },
                    softness: 0.0,
                    erase: false,
                    source: [4.0, 14.0],
                    heal: false,
                    clip: None,
                }),
            })
            .unwrap();
            if cast {
                doc.apply(Command::SetEffects {
                    id: lifting,
                    effects: vec![chitrakar_doc::Effect::DropShadow {
                        dx: 4.0,
                        dy: 4.0,
                        blur: 2.0,
                        color: AuthoredColor::Srgb {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        },
                        opacity: 1.0,
                    }],
                })
                .unwrap();
            }
            doc
        };

        let casting = build(true);
        assert!(
            !GpuRenderer::can_render(&casting),
            "an effect on a clone layer has to go back: there is no \
             surface to take its silhouette from here"
        );

        // Declining is worth something only because the effect changes
        // the picture, which is asked of the renderer that draws it.
        let plain = build(false);
        let (mean, worst) = difference(
            &chitrakar_render::render(&casting).unwrap(),
            &chitrakar_render::render(&plain).unwrap(),
        );
        assert!(
            mean > 0.005,
            "the shadow has to be worth declining a page over: mean \
             {mean:.5}, worst {worst:.4} against the same layer bare"
        );

        // And no wider than it needs: bare, the layer is drawn, the way
        // the reference renderer draws it.
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        assert!(
            GpuRenderer::can_render(&plain),
            "a clone layer wearing no effect is still drawn"
        );
        let (mean, worst) = difference(
            &gpu.render(&plain).unwrap(),
            &chitrakar_render::render(&plain).unwrap(),
        );
        assert!(
            mean < 0.004,
            "bare it draws what the reference draws: mean {mean:.5}, \
             worst {worst:.3}"
        );
    }

    /// A copy of a blended layer wearing a mask goes back.
    ///
    /// The blend belongs to what the copy draws, not to the copy. On a
    /// surface of its own it meets a transparent page, and every
    /// separable blend collapses to Normal against nothing — `ab` is
    /// zero, so the blended term drops out — which spends the blend and
    /// never asks for it again. A mask is what sends a copy to a surface
    /// here, so a masked copy of a blended layer loses the blend.
    ///
    /// The reference renderer hands the copy's mask down as a coverage
    /// and draws what it copies where it stands, so the blend meets the
    /// page really under it. That is a pass this backend has no shape
    /// for, so the page goes back — the same answer, and the same
    /// reason, as a copy of an adjustment wearing one.
    ///
    /// Found by a mask that hides nothing changing six of six hundred
    /// random pages.
    #[test]
    fn a_copy_of_a_blend_wearing_a_mask_goes_back() {
        let build = |masked: bool| {
            let mut doc = Document::new(48, 36, chitrakar_color::ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "ground",
                    VectorShape::Rect {
                        width: 48.0,
                        height: 36.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.30,
                        g: 0.55,
                        b: 0.70,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            let lit = add(
                &mut doc,
                filled(
                    "lit",
                    VectorShape::Rect {
                        width: 24.0,
                        height: 18.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.85,
                        g: 0.40,
                        b: 0.20,
                        a: 1.0,
                    },
                ),
                Transform::translation(4.0, 4.0),
            );
            doc.apply(Command::SetBlendMode {
                id: lit,
                blend: BlendMode::Multiply,
            })
            .unwrap();
            let copy = add(
                &mut doc,
                Box::new(Node::instance("a copy of it", lit)),
                Transform::translation(16.0, 10.0),
            );
            if masked {
                doc.apply(Command::SetMask {
                    id: copy,
                    mask: Some(Box::new(chitrakar_doc::Mask {
                        kind: chitrakar_doc::MaskKind::Vector {
                            shape: VectorShape::Rect {
                                width: 480.0,
                                height: 360.0,
                                radius: 0.0,
                            },
                            transform: Transform::translation(-240.0, -180.0),
                        },
                        invert: false,
                        feather: 0.0,
                    })),
                })
                .unwrap();
            }
            (doc, copy)
        };

        let (masked, _) = build(true);
        assert!(
            !GpuRenderer::can_render(&masked),
            "a copy of a blended layer wearing a mask has to go back: the \
             mask puts it on a surface of its own, where the blend it copies \
             meets nothing"
        );

        // Declining is worth something only because the two draw
        // differently — and the mask here hides nothing, so on the
        // renderer that is right they must not.
        let (plain, _) = build(false);
        let (mean, worst) = difference(
            &chitrakar_render::render(&masked).unwrap(),
            &chitrakar_render::render(&plain).unwrap(),
        );
        assert!(
            mean < 1e-6,
            "the reference draws the masked copy as the unmasked one — the \
             mask hides nothing: mean {mean:.6}, worst {worst:.4}"
        );

        // And no wider than it needs: unmasked, it is drawn, the way the
        // reference renderer draws it.
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        assert!(
            GpuRenderer::can_render(&plain),
            "a copy of a blended layer wearing nothing is still drawn"
        );
        let (mean, worst) = difference(
            &gpu.render(&plain).unwrap(),
            &chitrakar_render::render(&plain).unwrap(),
        );
        assert!(
            mean < 0.004,
            "unmasked it draws what the reference draws: mean {mean:.5}, \
             worst {worst:.3}"
        );
    }

    /// A copy of an adjustment wearing a mask or a fade goes back.
    ///
    /// What such a copy draws is a change to what is under it, so it can
    /// never go on a surface of its own — there is nothing under a fresh
    /// surface to change. A mask or an opacity below one is exactly what
    /// sends a copy to a surface here, and drawing it anyway is how this
    /// backend used to lose the layer altogether: masked by a mask that
    /// hides nothing, the copy drew what hiding it drew.
    ///
    /// The renderer being matched hands the copy's mask and opacity down
    /// as a coverage instead, which is a pass this one has no shape for —
    /// its own masks ride the single coverage slot a layer already uses.
    /// So the page goes back, which is the same answer a mask inside a
    /// masked layer with an effect gets, and for the same reason.
    ///
    /// Three things asserted, and the last two are what keep the limit
    /// honest: that the page is refused; that the *unworn* copy is still
    /// drawn and drawn the reference renderer's way, so the refusal is no
    /// wider than it needs to be; and that the two pictures really differ,
    /// so what is declined is a wrong answer rather than a scruple.
    #[test]
    fn a_copy_of_an_adjustment_wearing_something_goes_back() {
        let build = |dressed: bool| {
            let mut doc = Document::new(48, 36, chitrakar_color::ColorMode::Rgb);
            add(
                &mut doc,
                filled(
                    "ground",
                    VectorShape::Rect {
                        width: 48.0,
                        height: 36.0,
                        radius: 0.0,
                    },
                    AuthoredColor::Srgb {
                        r: 0.30,
                        g: 0.55,
                        b: 0.70,
                        a: 1.0,
                    },
                ),
                Transform::default(),
            );
            let adj = add(
                &mut doc,
                Box::new(Node::adjustment(
                    "darker",
                    chitrakar_doc::Adjustment::Exposure { stops: -1.5 },
                )),
                Transform::default(),
            );
            let copy = add(
                &mut doc,
                Box::new(Node::instance("a copy of it", adj)),
                Transform::default(),
            );
            if dressed {
                doc.apply(Command::SetMask {
                    id: copy,
                    mask: Some(Box::new(chitrakar_doc::Mask {
                        kind: chitrakar_doc::MaskKind::Vector {
                            shape: VectorShape::Rect {
                                width: 20.0,
                                height: 20.0,
                                radius: 0.0,
                            },
                            transform: Transform::translation(8.0, 6.0),
                        },
                        invert: false,
                        feather: 0.0,
                    })),
                })
                .unwrap();
            }
            (doc, copy)
        };

        let (dressed, _) = build(true);
        assert!(
            !GpuRenderer::can_render(&dressed),
            "a copy of an adjustment wearing a mask has to go back: it cannot \
             be put on a surface of its own, and a mask is what would put it \
             there"
        );

        // Declining is only worth something because the mask changes the
        // picture, which is asked of the renderer that draws it correctly.
        let (plain, copy) = build(false);
        let mut hidden = dressed.clone();
        hidden
            .apply(Command::SetVisible {
                id: copy,
                visible: false,
            })
            .unwrap();
        let (mean, _) = difference(
            &chitrakar_render::render(&dressed).unwrap(),
            &chitrakar_render::render(&hidden).unwrap(),
        );
        assert!(
            mean > 0.01,
            "the mask has to leave something of the copy showing or there is \
             nothing to decline: {mean:.5} against the copy hidden"
        );

        // And no wider than it needs: undressed, it is drawn, the way the
        // reference renderer draws it.
        let Some(gpu) = gpu_or_skip() else {
            return;
        };
        assert!(
            GpuRenderer::can_render(&plain),
            "a copy of an adjustment wearing nothing is still drawn"
        );
        let (mean, worst) = difference(
            &gpu.render(&plain).unwrap(),
            &chitrakar_render::render(&plain).unwrap(),
        );
        assert!(
            mean < 0.004,
            "undressed it draws what the reference draws: mean {mean:.5}, \
             worst {worst:.3}"
        );
    }
}
