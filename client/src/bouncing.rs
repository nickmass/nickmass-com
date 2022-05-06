use byteorder::{LittleEndian, WriteBytesExt};
use lyon::tessellation::*;
use rand::distributions::{Distribution, Standard};
use web_sys::{EventTarget, WebGlRenderingContext as GL};

use crate::gl::*;
use crate::header::*;
use crate::shaders::*;

#[derive(Copy, Clone, Debug)]
struct Vector2d<T> {
    x: T,
    y: T,
}

impl<T> Vector2d<T> {
    fn new(x: T, y: T) -> Self {
        Self { x, y }
    }
}

impl Vector2d<f32> {
    fn distance(&self, other: Vector2d<f32>) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        ((dx * dx) + (dy * dy)).sqrt()
    }
}

impl From<Vector2d<f32>> for lyon::math::Point {
    fn from(other: Vector2d<f32>) -> Self {
        lyon::math::point(other.x, other.y)
    }
}

impl From<&Vector2d<f32>> for lyon::math::Point {
    fn from(other: &Vector2d<f32>) -> Self {
        lyon::math::point(other.x, other.y)
    }
}

impl From<lyon::math::Point> for Vector2d<f32> {
    fn from(other: lyon::math::Point) -> Self {
        Vector2d::new(other.x, other.y)
    }
}

impl From<lyon::math::Vector> for Vector2d<f32> {
    fn from(other: lyon::math::Vector) -> Self {
        Vector2d::new(other.x, other.y)
    }
}

impl<T> Distribution<Vector2d<T>> for Standard
where
    Standard: Distribution<T>,
{
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Vector2d<T> {
        Vector2d::new(rng.gen(), rng.gen())
    }
}

impl<T: Copy> std::ops::Mul<T> for Vector2d<T>
where
    T: std::ops::Mul<Output = T>,
{
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        Vector2d::new(self.x * rhs, self.y * rhs)
    }
}

impl<T> std::ops::Add for Vector2d<T>
where
    T: std::ops::Add<Output = T>,
{
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Vector2d::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl<T: Copy> std::ops::Add<T> for Vector2d<T>
where
    T: std::ops::Add<Output = T>,
{
    type Output = Self;

    fn add(self, rhs: T) -> Self {
        Vector2d::new(self.x + rhs, self.y + rhs)
    }
}

struct Bounds<T> {
    x: T,
    y: T,
    width: T,
    height: T,
}

impl<T> Bounds<T> {
    fn new(x: T, y: T, width: T, height: T) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

impl<T: std::ops::Add<Output = T> + std::ops::Mul<Output = T> + std::cmp::PartialOrd + Copy>
    Bounds<T>
{
    fn in_x_bounds(&self, v: Vector2d<T>) -> bool {
        v.x >= self.x && (v.x <= self.x + self.width)
    }

    fn in_y_bounds(&self, v: Vector2d<T>) -> bool {
        v.y >= self.y && (v.y <= self.y + self.height)
    }

    fn in_bounds(&self, v: Vector2d<T>) -> bool {
        self.in_x_bounds(v) && self.in_y_bounds(v)
    }

    fn to_bounds_space(&self, v: Vector2d<T>) -> Vector2d<T> {
        let x = v.x * self.width + self.x;
        let y = v.y * self.height + self.y;
        Vector2d::new(x, y)
    }
}

const POINT_COUNT: usize = 100;

pub struct BouncingHeader<'ctx> {
    gl: &'ctx GlContext,
    model: GlModel<'ctx, Vertex>,
    index_buffer: GlIndexBuffer<'ctx>,
    alive: bool,
    balls: Vec<Ball>,
    width: f32,
    height: f32,
    timestamp: f64,
    vertex_buffers: VertexBuffers<Vertex, u16>,
    stroke_options: StrokeOptions,
    program: GlProgram<'ctx>,
    bounds: Bounds<f32>,
    logo: Logo<'ctx>,
    ping_pong_buffer: PingPongBuffer<'ctx>,
    blur_program: GlProgram<'ctx>,
    gaussian: Vec<f32>,
    circle_model: GlModel<'ctx, BallVertex>,
    ball_program: GlProgram<'ctx>,
}

impl<'ctx> BouncingHeader<'ctx> {
    pub fn new(gl: &'ctx GlContext) -> Self {
        let gaussian = calculate_gaussian(8.0);
        assert_eq!(gaussian.len(), 49);

        let buffer_width = gl.drawing_buffer_width();
        let buffer_height = gl.drawing_buffer_height();

        let width = gl.canvas().client_width() as f32;
        let height = gl.canvas().client_height() as f32;

        let bounds = Bounds::new(0., 0., width, height);

        let mut balls = Vec::with_capacity(POINT_COUNT);
        for _ in 0..POINT_COUNT {
            balls.push(Ball::new(&bounds));
        }

        let vertex_buffers = VertexBuffers::<_, u16>::new();
        let stroke_options = StrokeOptions::default().dont_apply_line_width();

        let program = GlProgram::with_shader::<BouncingShader>(gl);
        let model = GlModel::empty(gl);
        let index_buffer = GlIndexBuffer::empty(gl);
        let ping_pong_buffer = PingPongBuffer::new(gl, buffer_width as u32, buffer_height as u32);
        let blur_program = GlProgram::with_shader::<BlurShader>(gl);

        let circle = circle_model(32);
        let circle_model = GlModel::new(gl, circle);
        let ball_program = GlProgram::with_shader::<BallShader>(gl);

        let logo = Logo::new(gl, width, height);

        gl.viewport(0, 0, buffer_width, buffer_height);
        gl.color_mask(true, true, true, true);
        gl.clear_color(1., 1., 1., 1.);

        Self {
            gl,
            alive: true,
            width,
            height,
            balls,
            timestamp: 0.,
            vertex_buffers,
            stroke_options,
            program,
            model,
            index_buffer,
            bounds,
            logo,
            ping_pong_buffer,
            gaussian,
            blur_program,
            circle_model,
            ball_program,
        }
    }

    fn matrix(&self) -> [f32; 9] {
        [
            2.0 / self.width,
            0.0,
            -1.0,
            0.0,
            -2.0 / self.height,
            1.0,
            0.0,
            0.0,
            1.0,
        ]
    }
}

struct StrokeVertexCtor(f32);

impl StrokeVertexConstructor<Vertex> for StrokeVertexCtor {
    fn new_vertex(&mut self, point: lyon::math::Point, attributes: StrokeAttributes) -> Vertex {
        Vertex {
            position: point.into(),
            normal: attributes.normal().into(),
            line_width: 3.5,
            alpha: self.0,
        }
    }
}

impl SiteHeader for BouncingHeader<'static> {
    fn event_target(&self) -> &EventTarget {
        self.gl.canvas().as_ref()
    }

    fn resize(&mut self) {
        self.width = self.gl.canvas().client_width() as f32;
        self.height = self.gl.canvas().client_height() as f32;
        self.bounds = Bounds::new(0., 0., self.width, self.height);
        self.logo.resize(self.width, self.height);
    }

    fn tick(&mut self, timestamp: f64, mouse_position: Option<(f32, f32)>) -> bool {
        let d_timestamp = timestamp - self.timestamp;

        let dt = (d_timestamp * 60.0 / 1000.0) as f32;

        self.timestamp = timestamp;

        if d_timestamp > 1000.0 {
            return self.alive;
        }

        let bounds = &self.bounds;
        let matrix = self.matrix();

        for b in &mut self.balls {
            b.tick(dt, bounds);
        }

        self.logo.tick(dt, mouse_position);

        let max_distance = 100.0;
        let mouse = mouse_position.unwrap_or((max_distance * -2.0, max_distance * -2.0));

        self.balls
            .get_mut(0)
            .map(|b| b.location = Vector2d::new(mouse.0, mouse.1));

        self.vertex_buffers.vertices.clear();
        self.vertex_buffers.indices.clear();

        for a in 0..self.balls.len() {
            let location_a = self.balls[a].location;
            if !bounds.in_bounds(location_a) {
                continue;
            }
            let mut count = 0;
            for b in a + 1..self.balls.len() {
                let location_b = self.balls[b].location;
                if !bounds.in_bounds(location_b) {
                    continue;
                }
                let distance = location_a.distance(location_b);
                if distance > max_distance {
                    continue;
                }

                let closeness = distance / max_distance;

                let points = [location_a, location_b];
                basic_shapes::stroke_polyline(
                    points.iter().map(|l| l.into()),
                    false,
                    &self.stroke_options,
                    &mut BuffersBuilder::new(&mut self.vertex_buffers, StrokeVertexCtor(closeness)),
                )
                .expect("stroke polyline");

                count += 1;
                if count >= 2 {
                    break;
                }
            }
        }

        self.ping_pong_buffer.reset();
        self.ping_pong_buffer.bind();
        self.gl.clear(GL::COLOR_BUFFER_BIT);

        self.model.fill(&self.vertex_buffers.vertices);
        self.index_buffer.fill(&self.vertex_buffers.indices);

        let mut line_uniforms = GlUniformCollection::new();
        line_uniforms.add("u_view_matrix", &matrix);

        self.gl.enable(GL::BLEND);
        self.gl.blend_func(GL::SRC_ALPHA, GL::ONE_MINUS_SRC_ALPHA);
        self.program
            .draw(&self.model, &line_uniforms, Some(&self.index_buffer));

        let ball_instances = self
            .balls
            .iter()
            .map(|b| BallInstance { matrix: b.matrix() });

        let mut ball_uniforms = GlUniformCollection::new();
        ball_uniforms.add("u_view_matrix", &matrix);

        self.ball_program
            .draw_instanced(&self.circle_model, ball_instances, &ball_uniforms);
        self.gl.disable(GL::BLEND);

        let view_matrix = [
            1.0,
            0.0,
            0.0,
            0.0,
            self.width / self.height,
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        let logo_matrix = self.logo.matrix();

        self.ping_pong_buffer.flip();
        self.ping_pong_buffer.bind();

        self.logo.draw(view_matrix, &self.ping_pong_buffer.back());

        self.ping_pong_buffer.flip();
        self.ping_pong_buffer.bind();

        let mut quad_uniforms = GlUniformCollection::with_capacity(2);
        quad_uniforms
            .add("u_view_matrix", &view_matrix)
            .add("u_tex_sampler", self.ping_pong_buffer.back());

        self.logo
            .quad_program
            .draw(&self.logo.quad_model, &quad_uniforms, None);

        let mut blur_uniforms = GlUniformCollection::with_capacity(6);
        blur_uniforms
            .add("u_source", self.ping_pong_buffer.back())
            .add("u_dimensions", &(2048.0, 512.0))
            .add("u_view_matrix", &view_matrix)
            .add("u_gaussian", &self.gaussian)
            .add("u_blur_vert", &false)
            .add("u_model_matrix", &logo_matrix);

        self.blur_program
            .draw(&self.logo.logo_model, &blur_uniforms, None);

        self.ping_pong_buffer.unbind();

        quad_uniforms.add("u_tex_sampler", self.ping_pong_buffer.front());

        self.logo
            .quad_program
            .draw(&self.logo.quad_model, &quad_uniforms, None);

        blur_uniforms
            .add("u_source", self.ping_pong_buffer.front())
            .add("u_blur_vert", &true);

        self.blur_program
            .draw(&self.logo.logo_model, &blur_uniforms, None);

        self.alive
    }
}

struct PingPongBuffer<'ctx> {
    left: GlFrameBuffer<'ctx>,
    right: GlFrameBuffer<'ctx>,
    flipped: bool,
}

impl<'ctx> PingPongBuffer<'ctx> {
    fn new(gl: &'ctx GlContext, width: u32, height: u32) -> Self {
        Self {
            left: GlFrameBuffer::new(gl, width, height),
            right: GlFrameBuffer::new(gl, width, height),
            flipped: false,
        }
    }

    fn bind(&self) {
        if self.flipped {
            self.right.bind();
        } else {
            self.left.bind();
        }
    }

    fn flip(&mut self) {
        self.flipped = !self.flipped
    }

    fn reset(&mut self) {
        self.flipped = false;
        self.unbind();
    }

    fn front(&self) -> &GlTexture {
        if self.flipped {
            self.right.texture()
        } else {
            self.left.texture()
        }
    }

    fn back(&self) -> &GlTexture {
        if self.flipped {
            self.left.texture()
        } else {
            self.right.texture()
        }
    }

    fn unbind(&self) {
        if self.flipped {
            self.right.unbind()
        } else {
            self.left.unbind()
        }
    }
}

struct Ball {
    location: Vector2d<f32>,
    direction: Vector2d<f32>,
    radius: f32,
    speed: f32,
}

impl Ball {
    fn new(bounds: &Bounds<f32>) -> Self {
        let dir = rand::random::<f32>() * 2.0 * 3.14159;
        let location = bounds.to_bounds_space((rand::random::<Vector2d<f32>>() * 0.9) + 0.05);

        let radius = 6.0;
        let speed = (rand::random::<f32>() * 1.2) + 0.2;

        Self {
            location,
            radius,
            speed,
            direction: Vector2d::new(dir.sin(), dir.cos()),
        }
    }

    fn matrix(&self) -> [f32; 9] {
        [
            self.radius,
            0.0,
            self.location.x,
            0.0,
            self.radius,
            self.location.y,
            0.0,
            0.0,
            1.0,
        ]
    }

    fn tick(&mut self, dt: f32, bounds: &Bounds<f32>) {
        let new_location = self.location + (self.direction * self.speed * dt);

        if !bounds.in_x_bounds(new_location) {
            self.direction.x *= -1.0;
        }
        if !bounds.in_y_bounds(new_location) {
            self.direction.y *= -1.0;
        }

        self.location = new_location;
    }
}

#[derive(Clone, Debug)]
struct Vertex {
    position: Vector2d<f32>,
    normal: Vector2d<f32>,
    line_width: f32,
    alpha: f32,
}

impl AsGlVertex for Vertex {
    const ATTRIBUTES: &'static [(&'static str, GlValueType)] = &[
        ("a_position", GlValueType::Vec2),
        ("a_normal", GlValueType::Vec2),
        ("a_line_width", GlValueType::Float),
        ("a_alpha", GlValueType::Float),
    ];
    const POLY_TYPE: u32 = GL::TRIANGLES;
    const SIZE: usize = 20;
    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.x);
        let _ = buf.write_f32::<LittleEndian>(self.position.y);
        let _ = buf.write_f32::<LittleEndian>(self.normal.x);
        let _ = buf.write_f32::<LittleEndian>(self.normal.y);
        let _ = buf.write_f32::<LittleEndian>(self.line_width);
        let _ = buf.write_f32::<LittleEndian>(self.alpha);
    }
}

struct BallInstance {
    matrix: [f32; 9],
}

impl AsGlVertex for BallInstance {
    const ATTRIBUTES: &'static [(&'static str, GlValueType)] =
        &[("a_model_matrix", GlValueType::Mat3)];
    const POLY_TYPE: u32 = GL::TRIANGLE_FAN;
    const SIZE: usize = 36;
    fn write(&self, mut buf: impl std::io::Write) {
        for f in &self.matrix {
            let _ = buf.write_f32::<LittleEndian>(*f);
        }
    }
}

#[derive(Clone, Debug)]
struct BallVertex {
    position: Vector2d<f32>,
    offset: f32,
}

impl AsGlVertex for BallVertex {
    const ATTRIBUTES: &'static [(&'static str, GlValueType)] = &[
        ("a_position", GlValueType::Vec2),
        ("a_offset", GlValueType::Float),
    ];
    const POLY_TYPE: u32 = GL::TRIANGLE_FAN;
    const SIZE: usize = 12;
    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.x);
        let _ = buf.write_f32::<LittleEndian>(self.position.y);
        let _ = buf.write_f32::<LittleEndian>(self.offset);
    }
}

fn circle_model(tris: u32) -> Vec<BallVertex> {
    let mut count: f32 = 0.0;
    let inc = std::f32::consts::PI * 2.0 / (tris - 2) as f32;

    let mut v = Vec::new();

    v.push(BallVertex {
        position: Vector2d::new(0.0, 0.0),
        offset: 0.0,
    });

    for _ in 0..(tris - 1) {
        v.push(BallVertex {
            position: Vector2d::new(count.sin(), count.cos()),
            offset: 1.0,
        });
        count += inc;
    }

    v
}

fn calculate_gaussian(stdev: f32) -> Vec<f32> {
    let dims = stdev.ceil() * 6.0;
    let mut count = dims.floor() as usize;
    count += if count % 2 == 0 { 1 } else { 0 };

    let mut v = Vec::with_capacity(count);

    let mut x = 0.0 - (count / 2) as f32;

    for _ in 0..count {
        let g = (1.0 / (2.0 * std::f32::consts::PI * stdev.powi(2)))
            * (std::f32::consts::E.powf(-1.0 * ((x * x) / (2.0 * stdev.powi(2)))));
        x += 1.0;
        v.push(g);
    }

    let mag = v.iter().map(|x| x).sum::<f32>();
    v.iter().map(|x| x / mag).collect()
}
