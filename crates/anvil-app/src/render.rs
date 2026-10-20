//! wgpu renderer for the shaded model and its edges. wgpu runs on Metal (macOS), Vulkan
//! or OpenGL (Linux) and DX12 or Vulkan (Windows).
//!
//! The app keeps a [`Renderer`] holding the latest scene. Each frame it hands egui a
//! paint callback; the GPU side ([`Gpu`]) lives in egui's callback resources, uploads a
//! new scene when its version changes, and draws into egui's own render pass. Without a
//! GPU (the UI tests) the callback is recorded but never run; the headless harness draws
//! the same callback into an offscreen texture.

use anvil_kernel::{FaceId, Mesh, Vec3};
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use std::sync::Arc;
use wgpu::util::DeviceExt;

const SHADER: &str = r#"
struct Uniforms {
    mvp: mat4x4<f32>,
    view: vec4<f32>,
    light: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    var c = u.mvp * vec4<f32>(v.pos, 1.0);
    // The camera produces OpenGL clip space (depth -w..w); wgpu wants 0..w.
    c.z = (c.z + c.w) * 0.5;
    o.clip = c;
    o.normal = v.normal;
    o.color = v.color;
    return o;
}

@fragment
fn fs_shaded(i: VOut) -> @location(0) vec4<f32> {
    var n = normalize(i.normal);
    let view = normalize(u.view.xyz);
    if (dot(n, view) < 0.0) {
        n = -n;
    }
    let light = normalize(u.light.xyz);
    let diff = max(dot(n, light), 0.0);
    let h = normalize(light + view);
    let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.25;
    return vec4<f32>(i.color * (0.38 + 0.62 * diff) + vec3<f32>(spec), 1.0);
}

@fragment
fn fs_flat(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color, 1.0);
}
"#;

pub const BODY_COLOR: [f32; 3] = [0.72, 0.75, 0.80];
pub const HOVER_COLOR: [f32; 3] = [0.95, 0.75, 0.35];
pub const SELECT_COLOR: [f32; 3] = [0.30, 0.60, 1.00];
pub const EDGE_COLOR: [f32; 3] = [0.08, 0.08, 0.10];

/// CPU-side geometry waiting to be uploaded.
#[derive(Default)]
pub struct SceneData {
    pub tris: Vec<f32>,
    pub edges: Vec<f32>,
}

impl SceneData {
    pub fn build(
        mesh: &Mesh,
        edges: &[[Vec3; 2]],
        hover: Option<FaceId>,
        selected: &[FaceId],
        colors: &crate::theme::SceneColors,
    ) -> SceneData {
        let mut tris = Vec::with_capacity(mesh.indices.len() * 27);
        for t in &mesh.indices {
            for &i in t {
                let i = i as usize;
                let face = mesh.faces[i];
                let c = if selected.contains(&face) {
                    colors.select
                } else if hover == Some(face) {
                    colors.hover
                } else {
                    colors.body
                };
                tris.extend_from_slice(&mesh.positions[i].to_f32());
                tris.extend_from_slice(&mesh.normals[i].to_f32());
                tris.extend_from_slice(&c);
            }
        }
        let mut e = Vec::with_capacity(edges.len() * 18);
        for seg in edges {
            for p in seg {
                e.extend_from_slice(&p.to_f32());
                e.extend_from_slice(&[0.0, 0.0, 1.0]);
                e.extend_from_slice(&colors.edge);
            }
        }
        SceneData { tris, edges: e }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    mvp: [f32; 16],
    view: [f32; 4],
    light: [f32; 4],
}

/// The app-side handle: the latest scene and its version.
#[derive(Default)]
pub struct Renderer {
    scene: Option<Arc<SceneData>>,
    version: u64,
    /// Shown in the viewport if drawing is impossible. wgpu reports pipeline errors
    /// through the device, so this stays empty in normal use.
    pub error: Option<String>,
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer::default()
    }

    pub fn set_scene(&mut self, data: SceneData) {
        self.scene = Some(Arc::new(data));
        self.version += 1;
    }

    /// A paint callback drawing the scene into `rect` with the given camera.
    pub fn callback(
        &self,
        rect: egui::Rect,
        mvp: [f32; 16],
        view_dir: Vec3,
        light: Vec3,
        edges: bool,
    ) -> egui::PaintCallback {
        let v = view_dir.to_f32();
        let l = light.normalized().to_f32();
        egui_wgpu::Callback::new_paint_callback(
            rect,
            Draw {
                scene: self.scene.clone(),
                version: self.version,
                uniforms: Uniforms { mvp, view: [v[0], v[1], v[2], 0.0], light: [l[0], l[1], l[2], 0.0] },
                edges,
            },
        )
    }
}

struct Draw {
    scene: Option<Arc<SceneData>>,
    version: u64,
    uniforms: Uniforms,
    edges: bool,
}

/// GPU resources, stored in egui's callback resources.
pub struct Gpu {
    shaded: wgpu::RenderPipeline,
    flat: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    tris: Option<(wgpu::Buffer, u32)>,
    edges: Option<(wgpu::Buffer, u32)>,
    version: u64,
}

/// Creates the GPU resources. `msaa` and `depth` must match egui's render pass.
pub fn install(rs: &egui_wgpu::RenderState, msaa: u32, depth: Option<wgpu::TextureFormat>) {
    install_on(&rs.device, &mut rs.renderer.write(), rs.target_format, msaa, depth);
}

/// Creates the GPU resources on an egui renderer that draws to a `format` target, such as
/// the headless harness's offscreen texture.
pub fn install_on(
    device: &wgpu::Device,
    renderer: &mut egui_wgpu::Renderer,
    format: wgpu::TextureFormat,
    msaa: u32,
    depth: Option<wgpu::TextureFormat>,
) {
    renderer.callback_resources.insert(Gpu::new(device, format, msaa, depth));
}

impl Gpu {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        msaa: u32,
        depth: Option<wgpu::TextureFormat>,
    ) -> Gpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("anvilcad model"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("anvilcad uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("anvilcad uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("anvilcad uniforms"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("anvilcad"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];
        let pipeline = |fs: &str, topology, bias: wgpu::DepthBiasState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 9 * 4,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attrs,
                    }],
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: depth.map(|format| wgpu::DepthStencilState {
                    format,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState { count: msaa.max(1), ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };
        // Faces sit slightly behind their edges so the edges always win the depth test.
        let face_bias = wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 };
        Gpu {
            shaded: pipeline("fs_shaded", wgpu::PrimitiveTopology::TriangleList, face_bias),
            flat: pipeline("fs_flat", wgpu::PrimitiveTopology::LineList, Default::default()),
            uniforms,
            bind_group,
            tris: None,
            edges: None,
            version: u64::MAX,
        }
    }

    fn upload(&mut self, device: &wgpu::Device, scene: Option<&SceneData>, version: u64) {
        let buffer = |data: &[f32]| {
            (!data.is_empty()).then(|| {
                let b = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("anvilcad scene"),
                    contents: bytemuck::cast_slice(data),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                (b, (data.len() / 9) as u32)
            })
        };
        self.tris = scene.and_then(|s| buffer(&s.tris));
        self.edges = scene.and_then(|s| buffer(&s.edges));
        self.version = version;
    }
}

impl egui_wgpu::CallbackTrait for Draw {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get_mut::<Gpu>() {
            if gpu.version != self.version {
                gpu.upload(device, self.scene.as_deref(), self.version);
            }
            queue.write_buffer(&gpu.uniforms, 0, bytemuck::bytes_of(&self.uniforms));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(gpu) = resources.get::<Gpu>() else { return };
        pass.set_bind_group(0, &gpu.bind_group, &[]);
        if let Some((buf, n)) = &gpu.tris {
            pass.set_pipeline(&gpu.shaded);
            pass.set_vertex_buffer(0, buf.slice(..));
            pass.draw(0..*n, 0..1);
        }
        if self.edges {
            if let Some((buf, n)) = &gpu.edges {
                pass.set_pipeline(&gpu.flat);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..*n, 0..1);
            }
        }
    }
}
