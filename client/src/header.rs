use byteorder::{LittleEndian, WriteBytesExt};
use gloo_events::EventListener;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, EventTarget, HtmlCanvasElement, MouseEvent, WebGlRenderingContext as GL};

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::gl::*;
use crate::shaders::*;

include!("nickmass-com-text.rs");

static mut GL_CONTEXT: Option<GlContext> = None;

#[allow(unused)]
struct Runner<H: SiteHeader> {
    was_resized: Rc<Cell<bool>>,
    mouse_position: Rc<Cell<Option<(f32, f32)>>>,
    resize: EventListener,
    mouse_move: EventListener,
    mouse_out: EventListener,
    _header: std::marker::PhantomData<H>,
}

impl<H: SiteHeader> Runner<H> {
    pub fn new(mut header: H) -> Self {
        let window = web_sys::window().unwrap();
        let window_et: &EventTarget = window.as_ref();
        let header_et = header.event_target();

        let was_resized = Rc::new(Cell::new(true));
        let resize = EventListener::new(window_et, "resize", {
            let was_resized = was_resized.clone();
            move |_event| {
                was_resized.set(true);
            }
        });

        let mouse_position = Rc::new(Cell::new(None));
        let mouse_move = EventListener::new(header_et, "mousemove", {
            let mouse_position = mouse_position.clone();
            move |event| {
                let mouse_event = event.dyn_ref::<MouseEvent>().unwrap();
                mouse_position.set(Some((
                    mouse_event.offset_x() as f32,
                    mouse_event.offset_y() as f32,
                )));
            }
        });

        let mouse_out = EventListener::new(header_et, "mouseout", {
            let mouse_position = mouse_position.clone();
            move |_event| {
                mouse_position.set(None);
            }
        });

        let tick_cb = Rc::new(RefCell::new(None));

        *tick_cb.borrow_mut() = Some(Closure::wrap(Box::new({
            let window = window.clone();
            let tick_cb = tick_cb.clone();
            let was_resized = was_resized.clone();
            let mouse_position = mouse_position.clone();
            move |time: f64| {
                if was_resized.get() {
                    was_resized.set(false);
                    header.resize();
                }

                let is_alive = header.tick(time, mouse_position.get());
                if is_alive {
                    let tick = tick_cb.borrow();
                    let cb: &Closure<dyn FnMut(f64)> = tick.as_ref().unwrap();
                    let _ = window.request_animation_frame(cb.as_ref().unchecked_ref());
                } else {
                    *tick_cb.borrow_mut() = None;
                }
            }
        }) as Box<dyn FnMut(f64)>));

        let tick = tick_cb.borrow();
        let cb = tick.as_ref().unwrap();
        let _ = window.request_animation_frame(cb.as_ref().unchecked_ref());

        Self {
            was_resized,
            mouse_position,
            resize,
            mouse_move,
            mouse_out,
            _header: Default::default(),
        }
    }

    pub fn forget(self) {
        std::mem::forget(self)
    }
}

trait SiteHeader: 'static {
    fn event_target(&self) -> &EventTarget;
    fn resize(&mut self);
    fn tick(&mut self, time: f64, mouse_position: Option<(f32, f32)>) -> bool;
}

impl SiteHeader for Header<'static> {
    fn event_target(&self) -> &EventTarget {
        self.gl.canvas().as_ref()
    }

    fn resize(&mut self) {
        Header::resize(self);
    }

    fn tick(&mut self, time: f64, mouse_position: Option<(f32, f32)>) -> bool {
        Header::tick(self, time, mouse_position);
        self.is_alive()
    }
}

pub fn create_header(document: &Document) -> Option<()> {
    let canvas = document
        .query_selector("canvas#header-canvas")
        .unwrap_or(None)
        .and_then(|e| e.dyn_into::<HtmlCanvasElement>().ok())?;

    let header = unsafe {
        if GL_CONTEXT.is_some() {
            panic!("Context already initialized");
        }
        GL_CONTEXT = Some(GlContext::new(canvas));
        Header::new(GL_CONTEXT.as_ref().unwrap())
    };

    let runner = Runner::new(header);
    runner.forget();

    Some(())
}

struct Header<'ctx> {
    gl: &'ctx GlContext,
    count: u32,
    alive: bool,
    color_cycle: ColorCycle<'ctx>,
    circles: CircleCollection<'ctx>,
    mouse_circle: MouseCircle<'ctx>,
    logo: Logo<'ctx>,
    width: f32,
    height: f32,
    frame_buffer: GlFramebuffer<'ctx>,
}

impl<'ctx> Header<'ctx> {
    fn new(gl: &'ctx GlContext) -> Self {
        let buffer_width = gl.drawing_buffer_width() as f32;
        let buffer_height = gl.drawing_buffer_height() as f32;

        let width = gl.canvas().client_width() as f32;
        let height = gl.canvas().client_height() as f32;

        let frame_buffer = GlFramebuffer::new(gl, buffer_width as u32, buffer_height as u32);

        gl.viewport(0, 0, buffer_width as i32, buffer_height as i32);
        gl.color_mask(true, true, true, true);

        Self {
            gl,
            count: 0,
            alive: true,
            color_cycle: ColorCycle::new(gl),
            circles: CircleCollection::new(&gl),
            mouse_circle: MouseCircle::new(&gl, width, height),
            logo: Logo::new(gl, width, height),
            width,
            height,
            frame_buffer,
        }
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

    fn tick(&mut self, _time: f64, mouse_pos: Option<(f32, f32)>) {
        self.count += 1;

        self.color_cycle.tick();
        self.circles.tick();
        self.mouse_circle.tick(mouse_pos);
        self.logo.tick(mouse_pos);

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
        self.width = self.gl.canvas().client_width() as f32;
        self.height = self.gl.canvas().client_height() as f32;
        self.mouse_circle.resize(self.width, self.height);
        self.logo.resize(self.width, self.height);
    }

    fn is_alive(&self) -> bool {
        self.alive
    }
}

struct ColorCycle<'ctx> {
    gl: &'ctx GlContext,
    r: f32,
    g: f32,
    b: f32,
    increment: f32,
}

impl<'ctx> ColorCycle<'ctx> {
    fn new(gl: &'ctx GlContext) -> ColorCycle<'ctx> {
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

struct CircleInstance {
    matrix: [f32; 9],
    color: [f32; 3],
}

impl AsGlVertex for CircleInstance {
    const ATTRIBUTES: &'static [(&'static str, GlValueType)] = &[
        ("a_model_matrix", GlValueType::Mat3),
        ("a_color", GlValueType::Vec3),
    ];
    const POLY_TYPE: u32 = GL::TRIANGLE_FAN;
    const SIZE: usize = 48;
    fn write(&self, mut buf: impl std::io::Write) {
        for f in &self.matrix {
            let _ = buf.write_f32::<LittleEndian>(*f);
        }

        for f in &self.color {
            let _ = buf.write_f32::<LittleEndian>(*f);
        }
    }
}

struct CircleCollection<'ctx> {
    circles: Vec<Circle>,
    circle_model: GlModel<'ctx, CircleVertex>,
    circle_program: GlProgram<'ctx>,
}

impl<'ctx> CircleCollection<'ctx> {
    fn new(gl: &'ctx GlContext) -> CircleCollection {
        let mut circles = Vec::new();

        for _ in 0..150 {
            circles.push(Circle::new());
        }

        let circle_model = GlModel::new(gl, Circle::model());
        let circle_program = GlProgram::with_shader::<CircleShader>(gl);

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
        let instance_verts = self.circles.iter().map(|c| CircleInstance {
            matrix: c.matrix(),
            color: c.color(),
        });
        let mut uniforms = GlUniformCollection::new();

        uniforms
            .add("u_view_matrix", &view_matrix)
            .add("u_alpha", &1.0);

        self.circle_program
            .draw_instanced(&self.circle_model, instance_verts, &uniforms);
    }
}

struct MouseCircle<'ctx> {
    circle: Circle,
    pos_x: f32,
    pos_y: f32,
    in_bounds: bool,
    width: f32,
    height: f32,
    circle_model: GlModel<'ctx, CircleVertex>,
    circle_program: GlProgram<'ctx>,
    count: f32,
}

impl<'ctx> MouseCircle<'ctx> {
    fn new(gl: &'ctx GlContext, width: f32, height: f32) -> MouseCircle<'ctx> {
        let circle_model = GlModel::new(gl, Circle::model());
        let circle_program = GlProgram::with_shader::<CircleShader>(gl);
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

        let c_alpha = self.count.abs();

        uniforms
            .add("u_view_matrix", &view_matrix)
            .add("u_alpha", &c_alpha);

        let instance = CircleInstance {
            matrix: self.circle.matrix(),
            color: [1.0, 1.0, 1.0],
        };

        self.circle_program.draw_instanced(
            &self.circle_model,
            std::iter::once(instance),
            &uniforms,
        );
    }
}

struct Logo<'ctx> {
    quad_model: GlModel<'ctx, QuadVertex>,
    quad_program: GlProgram<'ctx>,
    logo_model: GlModel<'ctx, SimpleVertex>,
    logo_program: GlProgram<'ctx>,
    width: f32,
    height: f32,
    mouse_pos: (f32, f32),
}

impl<'ctx> Logo<'ctx> {
    fn new(gl: &'ctx GlContext, width: f32, height: f32) -> Logo {
        let quad_model = GlModel::new(gl, QuadVertex::model());
        let quad_program = GlProgram::with_shader::<QuadShader>(gl);

        let logo_model = GlModel::new(gl, SimpleVertex::logo_text());
        let logo_program = GlProgram::with_shader::<LogoShader>(gl);

        Logo {
            quad_model,
            quad_program,
            logo_model,
            logo_program,
            width,
            height,
            mouse_pos: (0.0, 0.0),
        }
    }

    fn matrix(&self) -> [f32; 9] {
        let ratio = self.height / self.width;
        let scale = if ratio > 0.15 { 0.75 } else { ratio * 5.0 };
        let offset = (ratio * ratio) * 0.8;
        [
            scale,
            0.0,
            0.0,
            0.0,
            scale,
            offset,
            self.mouse_pos.0 / 2.0,
            self.mouse_pos.1 / 2.0,
            1.0,
        ]
    }

    fn tick(&mut self, mouse_pos: Option<(f32, f32)>) {
        if let Some(mouse) = mouse_pos {
            let mouse = (
                mouse.0 / self.width * 2.0 - 1.0,
                mouse.1 / self.height * -2.0 + 1.0,
            );
            let diff_x = (mouse.0 - self.mouse_pos.0) / 5.0;
            let diff_y = (mouse.1 - self.mouse_pos.1) / 5.0;
            self.mouse_pos = (self.mouse_pos.0 + diff_x, self.mouse_pos.1 + diff_y);
        } else {
            let diff_x = (0.0 - self.mouse_pos.0) / 20.0;
            let diff_y = (0.0 - self.mouse_pos.1) / 20.0;
            self.mouse_pos = (self.mouse_pos.0 + diff_x, self.mouse_pos.1 + diff_y);
        }
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
    const ATTRIBUTES: &'static [(&'static str, GlValueType)] = &[
        ("a_position", GlValueType::Vec2),
        ("a_alpha", GlValueType::Float),
    ];
    const POLY_TYPE: u32 = GL::TRIANGLE_FAN;
    const SIZE: usize = 12;
    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.x);
        let _ = buf.write_f32::<LittleEndian>(self.y);
        let _ = buf.write_f32::<LittleEndian>(self.alpha);
    }
}

struct QuadVertex {
    position: (f32, f32),
    uv: (f32, f32),
}

impl AsGlVertex for QuadVertex {
    const ATTRIBUTES: &'static [(&'static str, GlValueType)] = &[
        ("a_position", GlValueType::Vec2),
        ("a_uv", GlValueType::Vec2),
    ];
    const POLY_TYPE: u32 = GL::TRIANGLE_STRIP;
    const SIZE: usize = 16;
    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.0);
        let _ = buf.write_f32::<LittleEndian>(self.position.1);
        let _ = buf.write_f32::<LittleEndian>(self.uv.0);
        let _ = buf.write_f32::<LittleEndian>(self.uv.1);
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
    const ATTRIBUTES: &'static [(&'static str, GlValueType)] = &[("a_position", GlValueType::Vec2)];
    const POLY_TYPE: u32 = GL::TRIANGLES;
    const SIZE: usize = 8;
    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.0);
        let _ = buf.write_f32::<LittleEndian>(self.position.1);
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
