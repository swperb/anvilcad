//! OpenGL renderer for the shaded model and its edges.

use anvil_kernel::{FaceId, Mesh, Vec3};
use eframe::glow::{self, HasContext};

const VS: &str = r#"
in vec3 a_pos;
in vec3 a_normal;
in vec3 a_color;
uniform mat4 u_mvp;
out vec3 v_normal;
out vec3 v_color;
void main() {
    gl_Position = u_mvp * vec4(a_pos, 1.0);
    v_normal = a_normal;
    v_color = a_color;
}
"#;

const FS: &str = r#"
precision mediump float;
in vec3 v_normal;
in vec3 v_color;
uniform vec3 u_light;
uniform vec3 u_view;
uniform float u_flat;
out vec4 out_color;
void main() {
    if (u_flat > 0.5) {
        out_color = vec4(v_color, 1.0);
        return;
    }
    vec3 n = normalize(v_normal);
    if (dot(n, u_view) < 0.0) n = -n;
    float diff = max(dot(n, u_light), 0.0);
    vec3 h = normalize(u_light + u_view);
    float spec = pow(max(dot(n, h), 0.0), 48.0) * 0.25;
    out_color = vec4(v_color * (0.38 + 0.62 * diff) + vec3(spec), 1.0);
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
    pub fn build(mesh: &Mesh, edges: &[[Vec3; 2]], hover: Option<FaceId>, selected: &[FaceId]) -> SceneData {
        let mut tris = Vec::with_capacity(mesh.indices.len() * 27);
        for t in &mesh.indices {
            for &i in t {
                let i = i as usize;
                let face = mesh.faces[i];
                let c = if selected.contains(&face) {
                    SELECT_COLOR
                } else if hover == Some(face) {
                    HOVER_COLOR
                } else {
                    BODY_COLOR
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
                e.extend_from_slice(&EDGE_COLOR);
            }
        }
        SceneData { tris, edges: e }
    }
}

pub struct Renderer {
    program: Option<glow::Program>,
    vao: Option<glow::VertexArray>,
    tri_buf: Option<glow::Buffer>,
    edge_buf: Option<glow::Buffer>,
    tri_count: i32,
    edge_count: i32,
    pending: Option<SceneData>,
    pub error: Option<String>,
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer {
            program: None,
            vao: None,
            tri_buf: None,
            edge_buf: None,
            tri_count: 0,
            edge_count: 0,
            pending: None,
            error: None,
        }
    }

    pub fn set_scene(&mut self, data: SceneData) {
        self.pending = Some(data);
    }

    unsafe fn init(&mut self, gl: &glow::Context) -> Result<(), String> {
        let version = eframe::egui_glow::ShaderVersion::get(gl);
        let header = version.version_declaration();
        let program = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
            let sh = gl.create_shader(kind)?;
            gl.shader_source(sh, &format!("{header}\n{src}"));
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) {
                return Err(gl.get_shader_info_log(sh));
            }
            gl.attach_shader(program, sh);
            shaders.push(sh);
        }
        gl.bind_attrib_location(program, 0, "a_pos");
        gl.bind_attrib_location(program, 1, "a_normal");
        gl.bind_attrib_location(program, 2, "a_color");
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        for sh in shaders {
            gl.detach_shader(program, sh);
            gl.delete_shader(sh);
        }
        self.program = Some(program);
        self.vao = Some(gl.create_vertex_array()?);
        self.tri_buf = Some(gl.create_buffer()?);
        self.edge_buf = Some(gl.create_buffer()?);
        Ok(())
    }

    unsafe fn bind_layout(&self, gl: &glow::Context, buf: glow::Buffer) {
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(buf));
        let stride = 9 * 4;
        for (i, off) in [(0u32, 0i32), (1, 12), (2, 24)] {
            gl.enable_vertex_attrib_array(i);
            gl.vertex_attrib_pointer_f32(i, 3, glow::FLOAT, false, stride, off);
        }
    }

    /// Draws into the current viewport. Called from the egui paint callback.
    pub fn paint(&mut self, gl: &glow::Context, mvp: &[f32; 16], view_dir: Vec3, light: Vec3, edges: bool) {
        unsafe {
            if self.program.is_none() && self.error.is_none() {
                if let Err(e) = self.init(gl) {
                    self.error = Some(e);
                }
            }
            let (Some(program), Some(vao), Some(tb), Some(eb)) =
                (self.program, self.vao, self.tri_buf, self.edge_buf)
            else {
                return;
            };
            gl.bind_vertex_array(Some(vao));
            if let Some(data) = self.pending.take() {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(tb));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    bytemuck::cast_slice(&data.tris),
                    glow::DYNAMIC_DRAW,
                );
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(eb));
                gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    bytemuck::cast_slice(&data.edges),
                    glow::DYNAMIC_DRAW,
                );
                self.tri_count = (data.tris.len() / 9) as i32;
                self.edge_count = (data.edges.len() / 9) as i32;
            }

            gl.use_program(Some(program));
            let loc = |n: &str| gl.get_uniform_location(program, n);
            gl.uniform_matrix_4_f32_slice(loc("u_mvp").as_ref(), false, mvp);
            let v = view_dir.to_f32();
            let l = light.normalized().to_f32();
            gl.uniform_3_f32(loc("u_view").as_ref(), v[0], v[1], v[2]);
            gl.uniform_3_f32(loc("u_light").as_ref(), l[0], l[1], l[2]);

            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.depth_mask(true);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::BLEND);

            if self.tri_count > 0 {
                gl.enable(glow::POLYGON_OFFSET_FILL);
                gl.polygon_offset(1.0, 1.0);
                gl.uniform_1_f32(loc("u_flat").as_ref(), 0.0);
                self.bind_layout(gl, tb);
                gl.draw_arrays(glow::TRIANGLES, 0, self.tri_count);
                gl.disable(glow::POLYGON_OFFSET_FILL);
            }
            if edges && self.edge_count > 0 {
                gl.uniform_1_f32(loc("u_flat").as_ref(), 1.0);
                self.bind_layout(gl, eb);
                gl.draw_arrays(glow::LINES, 0, self.edge_count);
            }

            gl.disable(glow::DEPTH_TEST);
            gl.enable(glow::BLEND);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.use_program(None);
        }
    }

    pub fn destroy(&mut self, gl: &glow::Context) {
        unsafe {
            if let Some(p) = self.program.take() {
                gl.delete_program(p);
            }
            if let Some(v) = self.vao.take() {
                gl.delete_vertex_array(v);
            }
            for b in [self.tri_buf.take(), self.edge_buf.take()].into_iter().flatten() {
                gl.delete_buffer(b);
            }
        }
    }
}
