use byteorder::{LittleEndian, WriteBytesExt};
use log::info;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, EventTarget, HtmlCanvasElement, MouseEvent, WebGlRenderingContext as GL};

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::gl::*;

include!("nickmass-com-text.rs");

pub fn create_header(document: Document) -> Option<()> {
    let window = document
        .default_view()
        .expect("unable to get window from document");

    let canvas = document
        .query_selector("canvas#header-canvas")
        .unwrap_or(None)
        .and_then(|e| e.dyn_into::<HtmlCanvasElement>().ok())?;

    let header = Header::new(&canvas);
    if let Some(mut header) = header {
        header.initialize();
        let header_func_inner = Rc::new(RefCell::new(None));
        let header_func = header_func_inner.clone();

        let window_inner = window.clone();

        let was_resized = Rc::new(Cell::new(true));
        let was_resized_inner = was_resized.clone();

        let mouse_pos = Rc::new(Cell::new(Option::<(f32, f32)>::None));
        let mouse_move_pos = mouse_pos.clone();
        let mouse_leave_pos = mouse_pos.clone();

        *header_func.borrow_mut() = Some(Closure::wrap(Box::new(move || {
            if was_resized_inner.get() {
                was_resized_inner.set(false);
                header.resize();
            }
            header.tick(mouse_pos.get());
            if header.is_alive() {
                let header_fun = header_func_inner.borrow();
                let cb: &Closure<FnMut()> = header_fun.as_ref().unwrap();
                let _ = window_inner.request_animation_frame(cb.as_ref().unchecked_ref());
            } else {
                *header_func_inner.borrow_mut() = None;
            }
        }) as Box<FnMut()>));

        let header_fun = header_func.borrow();
        let cb = header_fun.as_ref().unwrap();
        let _ = window.request_animation_frame(cb.as_ref().unchecked_ref());

        let resize_et: &EventTarget = window.as_ref();
        let resize_cb = Closure::wrap(Box::new(move || {
            was_resized.set(true);
        }) as Box<FnMut()>);
        let _ = resize_et
            .add_event_listener_with_callback("resize", resize_cb.as_ref().unchecked_ref())
            .unwrap();
        resize_cb.forget();

        let canvas_et: &EventTarget = canvas.as_ref();

        let mouse_move_cb = Closure::wrap(Box::new(move |e: MouseEvent| {
            mouse_move_pos.set(Some((e.offset_x() as f32, e.offset_y() as f32)));
        }) as Box<FnMut(MouseEvent)>);
        let _ = canvas_et
            .add_event_listener_with_callback("mousemove", mouse_move_cb.as_ref().unchecked_ref())
            .unwrap();
        mouse_move_cb.forget();

        let mouse_leave_cb = Closure::wrap(Box::new(move || {
            mouse_leave_pos.set(None);
        }) as Box<FnMut()>);
        let _ = canvas_et
            .add_event_listener_with_callback("mouseout", mouse_leave_cb.as_ref().unchecked_ref())
            .unwrap();
        mouse_leave_cb.forget();
    } else {
        info!("Unable to initialize header context");
        return None;
    }

    Some(())
}

struct Header {
    canvas: HtmlCanvasElement,
    gl: GL,
    count: u32,
    alive: bool,
    color_cycle: ColorCycle,
    circles: CircleCollection,
    mouse_circle: MouseCircle,
    logo: Logo,
    width: f32,
    height: f32,
    buffer_width: f32,
    buffer_height: f32,
    frame_buffer: GlFramebuffer,
}

impl Header {
    fn new(canvas: &HtmlCanvasElement) -> Option<Header> {
        let gl = canvas
            .get_context("webgl")
            .unwrap_or(None)
            .and_then(|e| e.dyn_into::<GL>().ok())?;

        let cycle_gl = gl.clone();
        let inner_gl = gl.clone();

        let buffer_width = gl.drawing_buffer_width() as f32;
        let buffer_height = gl.drawing_buffer_height() as f32;

        let width = canvas.client_width() as f32;
        let height = canvas.client_height() as f32;

        let frame_buffer =
            GlFramebuffer::new(gl.clone(), buffer_width as u32, buffer_height as u32);

        Some(Header {
            canvas: canvas.clone(),
            gl,
            count: 0,
            alive: true,
            color_cycle: ColorCycle::new(cycle_gl),
            circles: CircleCollection::new(&inner_gl),
            mouse_circle: MouseCircle::new(&inner_gl, width, height),
            logo: Logo::new(&inner_gl, width, height),
            width,
            height,
            buffer_width,
            buffer_height,
            frame_buffer,
        })
    }

    fn initialize(&mut self) {
        self.gl
            .viewport(0, 0, self.buffer_width as i32, self.buffer_height as i32);
        self.gl.color_mask(true, true, true, true);
    }

    fn matrix(&self) -> [f32; 9] {
        [
            1.0,
            0.0,
            0.0,
            0.0,
            self.width / self.height,
            0.0,
            0.0,
            0.0,
            1.0,
        ]
    }

    fn tick(&mut self, mouse_pos: Option<(f32, f32)>) {
        self.count += 1;

        self.color_cycle.tick();
        self.circles.tick();
        self.mouse_circle.tick(mouse_pos);

        self.frame_buffer.bind();
        self.gl.enable(GL::BLEND);

        self.color_cycle.draw();
        self.gl
            .blend_func_separate(GL::ONE, GL::ONE, GL::ZERO, GL::ZERO);
        self.gl.blend_equation(GL::FUNC_REVERSE_SUBTRACT);
        self.mouse_circle.draw(self.matrix());
        self.gl
            .blend_func_separate(GL::ONE, GL::ONE, GL::ONE, GL::ONE);
        self.gl.blend_equation(GL::FUNC_ADD);
        self.circles.draw(self.matrix());

        self.gl.disable(GL::BLEND);
        self.frame_buffer.unbind();

        self.logo.draw(self.matrix(), self.frame_buffer.texture());

        self.gl.finish();
    }

    fn resize(&mut self) {
        self.width = self.canvas.client_width() as f32;
        self.height = self.canvas.client_height() as f32;
        self.mouse_circle.resize(self.width, self.height);
        self.logo.resize(self.width, self.height);
    }

    fn is_alive(&self) -> bool {
        self.alive
    }
}

struct ColorCycle {
    gl: GL,
    r: f32,
    g: f32,
    b: f32,
    increment: f32,
}

impl ColorCycle {
    fn new(gl: GL) -> ColorCycle {
        let offset = rand::random::<f32>() * 2.0;
        ColorCycle {
            gl,
            r: -1.0 + offset,
            g: -0.33333333 + offset,
            b: 0.33333333 + offset,
            increment: 0.01,
        }
    }

    fn tick(&mut self) {
        self.r = if self.r < 1.0 {
            self.r + self.increment
        } else {
            self.r - 2.0
        };
        self.g = if self.g < 1.0 {
            self.g + self.increment
        } else {
            self.g - 2.0
        };
        self.b = if self.b < 1.0 {
            self.b + self.increment
        } else {
            self.b - 2.0
        };
    }

    fn draw(&mut self) {
        self.gl
            .clear_color(self.r.abs(), self.g.abs(), self.b.abs(), 1.0);
        self.gl.clear(GL::COLOR_BUFFER_BIT);
    }
}

struct Circle {
    center: (f32, f32),
    radius: f32,
    color: (f32, f32, f32),
    speed: f32,
}

impl Circle {
    fn new() -> Circle {
        let center: (f32, f32) = rand::random();

        Circle {
            center: (center.0 * 2.0 - 1.0, center.1 * 4.0 - 2.0),
            radius: rand::random::<f32>() / 2.0,
            color: rand::random(),
            speed: rand::random::<f32>() * 0.01,
        }
    }

    fn tick(&mut self) {
        self.center.1 = if self.center.1 > 2.0 {
            -2.0
        } else {
            self.center.1 + self.speed
        };
    }

    fn matrix(&self) -> [f32; 9] {
        [
            self.radius,
            0.0,
            self.center.0,
            0.0,
            self.radius,
            self.center.1,
            0.0,
            0.0,
            1.0,
        ]
    }

    fn set_center(&mut self, x: f32, y: f32) {
        self.center = (x, y);
    }

    fn color(&self) -> [f32; 3] {
        [self.color.0, self.color.1, self.color.2]
    }

    fn model() -> Vec<CircleVertex> {
        let mut count: f32 = 0.0;
        let inc = std::f32::consts::PI * 2.0 / (CIRCLE_TRI_COUNT - 2) as f32;

        let mut v = Vec::new();

        v.push(CircleVertex {
            x: 0.0,
            y: 0.0,
            alpha: 1.0,
        });

        for _ in 0..(CIRCLE_TRI_COUNT - 1) {
            v.push(CircleVertex {
                x: count.sin(),
                y: count.cos(),
                alpha: 0.0,
            });
            count += inc;
        }

        v
    }
}

struct CircleCollection {
    circles: Vec<Circle>,
    circle_model: GlModel<CircleVertex>,
    circle_program: GlProgram,
}

impl CircleCollection {
    fn new(gl: &GL) -> CircleCollection {
        let mut circles = Vec::new();

        for _ in 0..150 {
            circles.push(Circle::new());
        }

        let circle_model = GlModel::new(gl.clone(), Circle::model());
        let circle_program = GlProgram::new(gl.clone(), CIRCLE_VERT, CIRCLE_FRAG);

        CircleCollection {
            circles,
            circle_model,
            circle_program,
        }
    }

    fn tick(&mut self) {
        for circle in &mut self.circles {
            circle.tick();
        }
    }

    fn draw(&mut self, view_matrix: [f32; 9]) {
        for circle in &self.circles {
            let mut uniforms = GlUniformCollection::new();

            let c_matrix = circle.matrix();
            let c_color = circle.color();

            uniforms
                .add("u_view_matrix", &view_matrix)
                .add("u_model_matrix", &c_matrix)
                .add("u_color", &c_color)
                .add("u_alpha", &1.0);

            self.circle_program.draw(&self.circle_model, &uniforms);
        }
    }
}
struct MouseCircle {
    circle: Circle,
    pos_x: f32,
    pos_y: f32,
    in_bounds: bool,
    width: f32,
    height: f32,
    circle_model: GlModel<CircleVertex>,
    circle_program: GlProgram,
    count: f32,
}

impl MouseCircle {
    fn new(gl: &GL, width: f32, height: f32) -> MouseCircle {
        let circle_model = GlModel::new(gl.clone(), Circle::model());
        let circle_program = GlProgram::new(gl.clone(), CIRCLE_VERT, CIRCLE_FRAG);
        MouseCircle {
            circle: Circle::new(),
            pos_x: 0.0,
            pos_y: 0.0,
            in_bounds: false,
            width,
            height,
            circle_model,
            circle_program,
            count: 0.0,
        }
    }

    fn tick(&mut self, pos: Option<(f32, f32)>) {
        if let Some(pos) = pos {
            self.count = if self.count > 1.0 {
                self.count - 2.0
            } else {
                self.count + 0.03
            };

            self.circle.radius = 0.1;
            self.pos_x = pos.0;
            self.pos_y = pos.1;
            let offset = (self.width - self.height) / 2.0;
            self.in_bounds = true;
            self.circle.set_center(
                self.pos_x / self.width * 2.0 - 1.0,
                (self.pos_y + offset) / self.width * 2.0 - 1.0,
            );
        } else {
            self.in_bounds = false;
        }
    }

    fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    fn draw(&mut self, view_matrix: [f32; 9]) {
        if !self.in_bounds {
            return;
        }

        let mut uniforms = GlUniformCollection::new();

        let c_matrix = self.circle.matrix();
        let c_color: [f32; 3] = [1.0, 1.0, 1.0];
        let c_alpha = self.count.abs();

        uniforms
            .add("u_view_matrix", &view_matrix)
            .add("u_model_matrix", &c_matrix)
            .add("u_color", &c_color)
            .add("u_alpha", &c_alpha);

        self.circle_program.draw(&self.circle_model, &uniforms);
    }
}

struct Logo {
    quad_model: GlModel<QuadVertex>,
    quad_program: GlProgram,
    logo_model: GlModel<SimpleVertex>,
    logo_program: GlProgram,
    width: f32,
    height: f32,
}
impl Logo {
    fn new(gl: &GL, width: f32, height: f32) -> Logo {
        let quad_model = GlModel::new(gl.clone(), QuadVertex::model());
        let quad_program = GlProgram::new(gl.clone(), QUAD_VERT, QUAD_FRAG);

        let logo_model = GlModel::new(gl.clone(), SimpleVertex::logo_text());
        let logo_program = GlProgram::new(gl.clone(), LOGO_VERT, LOGO_FRAG);

        Logo {
            quad_model,
            quad_program,
            logo_model,
            logo_program,
            width,
            height,
        }
    }

    fn matrix(&self) -> [f32; 9] {
        let ratio = self.height / self.width;
        let scale = if ratio > 0.15 { 0.75 } else { ratio * 5.0 };
        let offset = (ratio * ratio) * 0.8;
        [scale, 0.0, 0.0, 0.0, scale, offset, 0.0, 0.0, 1.0]
    }

    fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    fn draw(&mut self, view_matrix: [f32; 9], circle_tex: &GlTexture) {
        let mut uniforms = GlUniformCollection::new();
        uniforms
            .add("u_view_matrix", &view_matrix)
            .add("u_tex_sampler", circle_tex);

        self.quad_program.draw(&self.quad_model, &uniforms);

        let matrix = self.matrix();
        let mut uniforms = GlUniformCollection::new();
        uniforms
            .add("u_view_matrix", &view_matrix)
            .add("u_model_matrix", &matrix)
            .add("u_frame_sampler", circle_tex)
            .add("u_alpha", &1.0);

        self.logo_program.draw(&self.logo_model, &uniforms);
    }
}

struct CircleVertex {
    x: f32,
    y: f32,
    alpha: f32,
}

impl AsGlVertex for CircleVertex {
    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.x);
        let _ = buf.write_f32::<LittleEndian>(self.y);
        let _ = buf.write_f32::<LittleEndian>(self.alpha);
    }

    fn bind_attrs(gl: &GL) {
        gl.vertex_attrib_pointer_with_i32(0, 2, GL::FLOAT, false, 12, 0);
        gl.enable_vertex_attrib_array(0);

        gl.vertex_attrib_pointer_with_i32(1, 1, GL::FLOAT, false, 12, 8);
        gl.enable_vertex_attrib_array(1);
    }

    fn unbind_attrs(gl: &GL) {
        gl.disable_vertex_attrib_array(0);
        gl.disable_vertex_attrib_array(1);
    }

    fn poly_info(len: usize) -> (u32, i32) {
        (GL::TRIANGLE_FAN, len as i32)
    }
}

struct QuadVertex {
    position: (f32, f32),
    uv: (f32, f32),
}

impl AsGlVertex for QuadVertex {
    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.0);
        let _ = buf.write_f32::<LittleEndian>(self.position.1);
        let _ = buf.write_f32::<LittleEndian>(self.uv.0);
        let _ = buf.write_f32::<LittleEndian>(self.uv.1);
    }

    fn bind_attrs(gl: &GL) {
        gl.vertex_attrib_pointer_with_i32(0, 2, GL::FLOAT, false, 16, 0);
        gl.enable_vertex_attrib_array(0);

        gl.vertex_attrib_pointer_with_i32(1, 2, GL::FLOAT, false, 16, 8);
        gl.enable_vertex_attrib_array(1);
    }

    fn unbind_attrs(gl: &GL) {
        gl.disable_vertex_attrib_array(0);
        gl.disable_vertex_attrib_array(1);
    }

    fn poly_info(len: usize) -> (u32, i32) {
        (GL::TRIANGLE_STRIP, len as i32)
    }
}

impl QuadVertex {
    fn model() -> Vec<QuadVertex> {
        let mut model = Vec::new();

        model.push(QuadVertex {
            position: (-1.0, -1.0),
            uv: (0.0, 1.0),
        });
        model.push(QuadVertex {
            position: (1.0, -1.0),
            uv: (1.0, 1.0),
        });
        model.push(QuadVertex {
            position: (-1.0, 1.0),
            uv: (0.0, 0.0),
        });
        model.push(QuadVertex {
            position: (1.0, 1.0),
            uv: (1.0, 0.0),
        });

        model
    }
}

struct SimpleVertex {
    position: (f32, f32),
}

impl AsGlVertex for SimpleVertex {
    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.0);
        let _ = buf.write_f32::<LittleEndian>(self.position.1);
    }

    fn bind_attrs(gl: &GL) {
        gl.vertex_attrib_pointer_with_i32(0, 2, GL::FLOAT, false, 8, 0);
        gl.enable_vertex_attrib_array(0);
    }

    fn unbind_attrs(gl: &GL) {
        gl.disable_vertex_attrib_array(0);
    }

    fn poly_info(len: usize) -> (u32, i32) {
        (GL::TRIANGLES, len as i32)
    }
}

impl SimpleVertex {
    fn logo_text() -> Vec<SimpleVertex> {
        let mut model = Vec::new();
        for v in LOGO_TEXT.chunks(2) {
            model.push(SimpleVertex {
                position: (v[0], -v[1]),
            })
        }

        model
    }
}

const CIRCLE_TRI_COUNT: usize = 32;

const CIRCLE_VERT: &str = r#"
precision highp float;

attribute vec2 a_position;
attribute float a_alpha;

varying float v_alpha;

uniform mat3 u_view_matrix;
uniform mat3 u_model_matrix;

void main() {
  vec3 pos = vec3(a_position, 1.0) * u_model_matrix * u_view_matrix;
  v_alpha = a_alpha;
  gl_Position = vec4(pos.xy / pos.z, 1.0, 1.0);
}
"#;

const CIRCLE_FRAG: &str = r#"
precision highp float;

varying float v_alpha;

uniform vec3 u_color;
uniform float u_alpha;

//uniform sampler2D u_logo_sampler;

void main() {
  gl_FragColor = vec4(u_color * v_alpha * u_alpha, v_alpha * u_alpha);
}
"#;

const QUAD_VERT: &str = r#"
precision highp float;

attribute vec2 a_position;
attribute vec2 a_uv;

uniform mat3 u_view_matrix;

varying vec2 v_uv;

void main() {
  vec3 pos = vec3(a_position, 1.0) * u_view_matrix;
  v_uv = (vec2(pos.x / pos.z, pos.y / pos.z * -1.0) + 1.0) / 2.0;
  gl_Position = vec4(pos.xy / pos.z, 1.0, 1.0);
}
"#;

const QUAD_FRAG: &str = r#"
precision highp float;

varying vec2 v_uv;

uniform sampler2D u_tex_sampler;

void main() {
  vec4 color = texture2D(u_tex_sampler, v_uv);
  gl_FragColor = vec4(color.xyz, 1.0);
}
"#;

const LOGO_VERT: &str = r#"
precision highp float;

attribute vec2 a_position;

varying vec2 v_uv;

uniform mat3 u_view_matrix;
uniform mat3 u_model_matrix;

void main() {
  vec3 pos = vec3(a_position, 1.0) * u_model_matrix * u_view_matrix;
  v_uv = (vec2(pos.x / pos.z, pos.y / pos.z * -1.0) + 1.0) / 2.0;
  gl_Position = vec4(pos.xy / pos.z, 1.0, 1.0);
}
"#;

const LOGO_FRAG: &str = r#"
precision highp float;

varying vec2 v_uv;

uniform sampler2D u_frame_sampler;

void main() {
  vec4 frame_color = texture2D(u_frame_sampler, v_uv);
  vec4 logo_color = (1.0 - frame_color) * 1.8;
  gl_FragColor = vec4(logo_color.xyz, 1.0);
}
"#;
