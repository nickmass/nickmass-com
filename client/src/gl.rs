use log::info;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    WebGlBuffer, WebGlFramebuffer, WebGlProgram, WebGlRenderingContext as GL, WebGlShader,
    WebGlTexture,
};

use std::cell::Cell;

pub struct GlProgram {
    gl: GL,
    program: WebGlProgram,
    vertex_shader: WebGlShader,
    fragment_shader: WebGlShader,
    texture_unit: Cell<u32>,
}

impl GlProgram {
    pub fn new(
        gl: GL,
        vertex_shader: impl AsRef<str>,
        fragment_shader: impl AsRef<str>,
    ) -> GlProgram {
        let shader_vert = gl
            .create_shader(GL::VERTEX_SHADER)
            .expect("Valid Vertex Shader");
        gl.shader_source(&shader_vert, vertex_shader.as_ref());
        gl.compile_shader(&shader_vert);
        let info = gl.get_shader_info_log(&shader_vert);
        if let Some(info) = info {
            if info.len() > 0 {
                info!("Vertex Shader: {}", info);
            }
        }

        let shader_frag = gl
            .create_shader(GL::FRAGMENT_SHADER)
            .expect("Valid Fragment Shader");
        gl.shader_source(&shader_frag, fragment_shader.as_ref());
        gl.compile_shader(&shader_frag);
        let info = gl.get_shader_info_log(&shader_frag);
        if let Some(info) = info {
            if info.len() > 0 {
                info!("Fragment Shader: {}", info);
            }
        }

        let prog = gl.create_program().expect("Create GL Program");
        gl.attach_shader(&prog, &shader_vert);
        gl.attach_shader(&prog, &shader_frag);
        gl.link_program(&prog);

        let info = gl.get_program_info_log(&prog);
        if let Some(info) = info {
            if info.len() > 0 {
                info!("Progam: {}", info);
            }
        }

        GlProgram {
            gl,
            program: prog,
            texture_unit: Cell::new(0),
            vertex_shader: shader_vert,
            fragment_shader: shader_frag,
        }
    }

    pub fn draw<V>(&self, model: &GlModel<V>, uniforms: &GlUniformCollection)
    where
        V: AsGlVertex,
    {
        self.gl.use_program(Some(&self.program));
        uniforms.bind(&self.gl, &self);
        model.draw(&self);
        self.reset_texture_unit();
    }

    fn next_texture_unit(&self) -> u32 {
        let r = self.texture_unit.get();
        self.texture_unit.set(r + 1);
        r
    }

    fn reset_texture_unit(&self) {
        self.texture_unit.set(0)
    }
}

pub struct GlInstancedProgram {
    gl: GL,
    program: GlProgram,
}

impl GlInstancedProgram {
    pub fn new(
        gl: GL,
        vertex_shader: impl AsRef<str>,
        fragment_shader: impl AsRef<str>,
    ) -> GlInstancedProgram {
        let program = GlProgram::new(gl.clone(), vertex_shader, fragment_shader);
        GlInstancedProgram { gl, program }
    }

    pub fn draw<V, I: AsGlVertex>(
        &self,
        model: &GlInstancedModel<V, I>,
        instanced_data: impl IntoIterator<Item = I>,
        uniforms: &GlUniformCollection,
    ) where
        V: AsGlVertex,
    {
        self.gl.use_program(Some(&self.program.program));
        uniforms.bind(&self.gl, &self.program);
        model.draw(&self.program, instanced_data);
        self.program.reset_texture_unit();
    }
}

impl Drop for GlProgram {
    fn drop(&mut self) {
        self.gl.detach_shader(&self.program, &self.vertex_shader);
        self.gl.detach_shader(&self.program, &self.fragment_shader);

        self.gl.delete_shader(Some(&self.vertex_shader));
        self.gl.delete_shader(Some(&self.fragment_shader));

        self.gl.delete_program(Some(&self.program));
    }
}

pub struct GlUniformCollection<'a> {
    uniforms: fxhash::FxHashMap<&'static str, &'a dyn AsGlUniform>,
}

impl<'a> GlUniformCollection<'a> {
    pub fn new() -> GlUniformCollection<'a> {
        GlUniformCollection {
            uniforms: Default::default(),
        }
    }

    pub fn add(&mut self, name: &'static str, uniform: &'a dyn AsGlUniform) -> &mut Self {
        self.uniforms.insert(name, uniform);

        self
    }

    fn bind(&self, gl: &GL, program: &GlProgram) {
        for (k, v) in &self.uniforms {
            v.bind(gl, program, k);
        }
    }
}

pub trait AsGlUniform {
    fn bind(&self, gl: &GL, program: &GlProgram, name: &'static str);
}

impl AsGlUniform for f32 {
    fn bind(&self, gl: &GL, program: &GlProgram, name: &'static str) {
        let location = gl.get_uniform_location(&program.program, name);
        gl.uniform1f(location.as_ref(), *self);
    }
}

impl AsGlUniform for [f32; 2] {
    fn bind(&self, gl: &GL, program: &GlProgram, name: &'static str) {
        let location = gl.get_uniform_location(&program.program, name);
        gl.uniform2fv_with_f32_array(location.as_ref(), &self[..]);
    }
}

impl AsGlUniform for [f32; 3] {
    fn bind(&self, gl: &GL, program: &GlProgram, name: &'static str) {
        let location = gl.get_uniform_location(&program.program, name);
        gl.uniform3fv_with_f32_array(location.as_ref(), &self[..]);
    }
}

impl AsGlUniform for [f32; 9] {
    fn bind(&self, gl: &GL, program: &GlProgram, name: &'static str) {
        let location = gl.get_uniform_location(&program.program, name);
        gl.uniform_matrix3fv_with_f32_array(location.as_ref(), false, &self[..]);
    }
}

pub struct GlModel<V: AsGlVertex> {
    buffer: WebGlBuffer,
    gl: GL,
    poly_type: u32,
    poly_count: i32,
    _marker: std::marker::PhantomData<V>,
}

impl<V: AsGlVertex> GlModel<V> {
    pub fn new(gl: GL, vertexes: impl IntoIterator<Item = V>) -> GlModel<V> {
        let buffer = gl.create_buffer().expect("Gl Buffer");
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&buffer));
        let mut data = Vec::new();

        let mut count = 0;
        for v in vertexes {
            v.write(&mut data);
            count += 1;
        }

        gl.buffer_data_with_u8_array(GL::ARRAY_BUFFER, data.as_slice(), GL::STATIC_DRAW);

        let (poly_type, poly_count) = (V::POLY_TYPE, count);

        GlModel {
            buffer,
            gl,
            poly_type,
            poly_count,
            _marker: Default::default(),
        }
    }

    fn draw(&self, program: &GlProgram) {
        self.gl.bind_buffer(GL::ARRAY_BUFFER, Some(&self.buffer));
        let stride = V::ATTRIBUTES.iter().map(|a| a.1.size()).sum();
        let mut offset = 0;
        for (name, vtype) in V::ATTRIBUTES {
            let location = self.gl.get_attrib_location(&program.program, name);
            if location < 0 {
                continue;
            }
            let location = location as u32;
            vtype.layout(&self.gl, location, stride, offset);
            vtype.enable(&self.gl, location);
            offset += vtype.size();
        }

        self.gl.draw_arrays(self.poly_type, 0, self.poly_count);

        for (name, vtype) in V::ATTRIBUTES {
            let location = self.gl.get_attrib_location(&program.program, name);
            if location < 0 {
                continue;
            }
            let location = location as u32;
            vtype.disable(&self.gl, location);
        }
    }
}

impl<V: AsGlVertex> Drop for GlModel<V> {
    fn drop(&mut self) {
        self.gl.delete_buffer(Some(&self.buffer));
    }
}

pub trait AsGlVertex {
    const ATTRIBUTES: &'static [(&'static str, GlValueType)];
    const POLY_TYPE: u32;

    fn write(&self, buf: impl std::io::Write);
}

pub enum GlValueType {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Mat3,
    Mat4,
}

impl GlValueType {
    fn size(&self) -> i32 {
        match self {
            GlValueType::Float => 4,
            GlValueType::Vec2 => 8,
            GlValueType::Vec3 => 12,
            GlValueType::Vec4 => 16,
            GlValueType::Mat3 => 36,
            GlValueType::Mat4 => 64,
        }
    }

    fn elements(&self) -> u32 {
        match self {
            GlValueType::Mat3 => 3,
            GlValueType::Mat4 => 4,
            _ => 1,
        }
    }

    fn enable(&self, gl: &GL, location: u32) {
        match self {
            GlValueType::Mat3 => {
                gl.enable_vertex_attrib_array(location);
                gl.enable_vertex_attrib_array(location + 1);
                gl.enable_vertex_attrib_array(location + 2);
            }
            GlValueType::Mat4 => {
                gl.enable_vertex_attrib_array(location);
                gl.enable_vertex_attrib_array(location + 1);
                gl.enable_vertex_attrib_array(location + 2);
                gl.enable_vertex_attrib_array(location + 3);
            }
            _ => gl.enable_vertex_attrib_array(location),
        }
    }

    fn disable(&self, gl: &GL, location: u32) {
        match self {
            GlValueType::Mat3 => {
                gl.disable_vertex_attrib_array(location);
                gl.disable_vertex_attrib_array(location + 1);
                gl.disable_vertex_attrib_array(location + 2);
            }
            GlValueType::Mat4 => {
                gl.disable_vertex_attrib_array(location);
                gl.disable_vertex_attrib_array(location + 1);
                gl.disable_vertex_attrib_array(location + 2);
                gl.disable_vertex_attrib_array(location + 3);
            }
            _ => gl.disable_vertex_attrib_array(location),
        }
    }

    fn layout(&self, gl: &GL, location: u32, stride: i32, offset: i32) {
        match self {
            GlValueType::Float => {
                gl.vertex_attrib_pointer_with_i32(location, 1, GL::FLOAT, false, stride, offset);
            }
            GlValueType::Vec2 => {
                gl.vertex_attrib_pointer_with_i32(location, 2, GL::FLOAT, false, stride, offset);
            }
            GlValueType::Vec3 => {
                gl.vertex_attrib_pointer_with_i32(location, 3, GL::FLOAT, false, stride, offset);
            }
            GlValueType::Vec4 => {
                gl.vertex_attrib_pointer_with_i32(location, 4, GL::FLOAT, false, stride, offset);
            }
            GlValueType::Mat3 => {
                gl.vertex_attrib_pointer_with_i32(location, 3, GL::FLOAT, false, stride, offset);
                gl.vertex_attrib_pointer_with_i32(
                    location + 1,
                    3,
                    GL::FLOAT,
                    false,
                    stride,
                    offset + 12,
                );
                gl.vertex_attrib_pointer_with_i32(
                    location + 2,
                    3,
                    GL::FLOAT,
                    false,
                    stride,
                    offset + 24,
                );
            }
            GlValueType::Mat4 => {
                gl.vertex_attrib_pointer_with_i32(location, 4, GL::FLOAT, false, stride, offset);
                gl.vertex_attrib_pointer_with_i32(
                    location + 1,
                    4,
                    GL::FLOAT,
                    false,
                    stride,
                    offset + 16,
                );
                gl.vertex_attrib_pointer_with_i32(
                    location + 2,
                    4,
                    GL::FLOAT,
                    false,
                    stride,
                    offset + 32,
                );
                gl.vertex_attrib_pointer_with_i32(
                    location + 3,
                    4,
                    GL::FLOAT,
                    false,
                    stride,
                    offset + 48,
                );
            }
        }
    }
}

pub struct GlInstancedModel<V: AsGlVertex, I: AsGlVertex> {
    buffer: WebGlBuffer,
    gl: GL,
    poly_type: u32,
    poly_count: i32,
    ext: AngleInstancedArrays,
    _v_marker: std::marker::PhantomData<V>,
    _i_marker: std::marker::PhantomData<I>,
}
impl<V: AsGlVertex, I: AsGlVertex> GlInstancedModel<V, I> {
    pub fn new(gl: GL, vertexes: impl IntoIterator<Item = V>) -> GlInstancedModel<V, I> {
        let buffer = gl.create_buffer().expect("Gl Buffer");
        gl.bind_buffer(GL::ARRAY_BUFFER, Some(&buffer));
        let mut data = Vec::new();

        let mut count = 0;
        for v in vertexes {
            v.write(&mut data);
            count += 1;
        }

        gl.buffer_data_with_u8_array(GL::ARRAY_BUFFER, data.as_slice(), GL::STATIC_DRAW);

        let (poly_type, poly_count) = (V::POLY_TYPE, count);

        let ext = gl
            .get_extension("ANGLE_instanced_arrays")
            .transpose()
            .and_then(|r| r.ok())
            .expect("Angle extension");
        let ext = ext.unchecked_into::<AngleInstancedArrays>();

        GlInstancedModel {
            buffer,
            gl,
            poly_type,
            poly_count,
            ext,
            _v_marker: Default::default(),
            _i_marker: Default::default(),
        }
    }

    fn draw(&self, program: &GlProgram, instance_vertexes: impl IntoIterator<Item = I>) {
        self.gl.bind_buffer(GL::ARRAY_BUFFER, Some(&self.buffer));
        let stride = V::ATTRIBUTES.iter().map(|a| a.1.size()).sum();
        let mut offset = 0;
        for (name, vtype) in V::ATTRIBUTES {
            let location = self.gl.get_attrib_location(&program.program, name);
            if location < 0 {
                continue;
            }
            let location = location as u32;
            for i in 0..vtype.elements() {
                self.ext.vertex_attrib_divisor_angle(location + i, 0);
            }
            vtype.layout(&self.gl, location, stride, offset);
            vtype.enable(&self.gl, location);
            offset += vtype.size();
        }

        let instance_buffer = self.gl.create_buffer().expect("Gl Instance Buffer");
        self.gl
            .bind_buffer(GL::ARRAY_BUFFER, Some(&instance_buffer));
        let mut data = Vec::new();

        let mut count = 0;
        for v in instance_vertexes {
            v.write(&mut data);
            count += 1;
        }

        self.gl
            .buffer_data_with_u8_array(GL::ARRAY_BUFFER, data.as_slice(), GL::STATIC_DRAW);

        let stride = I::ATTRIBUTES.iter().map(|a| a.1.size()).sum();
        let mut offset = 0;
        for (name, vtype) in I::ATTRIBUTES {
            let location = self.gl.get_attrib_location(&program.program, name);
            if location < 0 {
                continue;
            }
            let location = location as u32;
            for i in 0..vtype.elements() {
                self.ext.vertex_attrib_divisor_angle(location + i, 1);
            }
            vtype.layout(&self.gl, location, stride, offset);
            vtype.enable(&self.gl, location);
            offset += vtype.size();
        }

        self.ext
            .draw_arrays_instanced_angle(self.poly_type, 0, self.poly_count, count)
            .expect("Instanced Draw");

        for (name, vtype) in I::ATTRIBUTES {
            let location = self.gl.get_attrib_location(&program.program, name);
            if location < 0 {
                continue;
            }
            let location = location as u32;
            for i in 0..vtype.elements() {
                self.ext.vertex_attrib_divisor_angle(location + i, 0);
            }
            vtype.disable(&self.gl, location);
        }

        self.gl.delete_buffer(Some(&instance_buffer));

        for (name, vtype) in V::ATTRIBUTES {
            let location = self.gl.get_attrib_location(&program.program, name);
            if location < 0 {
                continue;
            }
            let location = location as u32;
            for i in 0..vtype.elements() {
                self.ext.vertex_attrib_divisor_angle(location + i, 0);
            }
            vtype.disable(&self.gl, location);
        }
    }
}

impl<V: AsGlVertex, I: AsGlVertex> Drop for GlInstancedModel<V, I> {
    fn drop(&mut self) {
        self.gl.delete_buffer(Some(&self.buffer));
    }
}

pub struct GlTexture {
    gl: GL,
    texture: WebGlTexture,
}

impl GlTexture {
    pub fn new(gl: GL, width: u32, height: u32) -> GlTexture {
        let texture = gl.create_texture().expect("Create Texture");
        let buf = vec![0; width as usize * height as usize * 4];

        gl.active_texture(GL::TEXTURE0);
        gl.bind_texture(GL::TEXTURE_2D, Some(&texture));
        gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
            GL::TEXTURE_2D,
            0,
            GL::RGBA as i32,
            width as i32,
            height as i32,
            0,
            GL::RGBA,
            GL::UNSIGNED_BYTE,
            Some(&buf),
        )
        .expect("Assign Texture");
        gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_WRAP_S, GL::CLAMP_TO_EDGE as i32);
        gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_WRAP_T, GL::CLAMP_TO_EDGE as i32);
        gl.tex_parameteri(GL::TEXTURE_2D, GL::TEXTURE_MIN_FILTER, GL::LINEAR as i32);

        GlTexture { gl, texture }
    }
}

impl Drop for GlTexture {
    fn drop(&mut self) {
        self.gl.delete_texture(Some(&self.texture));
    }
}

impl AsGlUniform for GlTexture {
    fn bind(&self, gl: &GL, program: &GlProgram, name: &'static str) {
        let texture_unit = program.next_texture_unit();
        gl.active_texture(GL::TEXTURE0 + texture_unit);
        gl.bind_texture(GL::TEXTURE_2D, Some(&self.texture));

        let location = gl.get_uniform_location(&program.program, name);
        gl.uniform1i(location.as_ref(), texture_unit as i32);
    }
}

pub struct GlFramebuffer {
    texture: GlTexture,
    frame_buffer: WebGlFramebuffer,
    width: u32,
    height: u32,
    gl: GL,
}

impl GlFramebuffer {
    pub fn new(gl: GL, width: u32, height: u32) -> GlFramebuffer {
        let texture = GlTexture::new(gl.clone(), width, height);
        let frame_buffer = gl.create_framebuffer().expect("Create FrameBuffer");
        gl.bind_framebuffer(GL::FRAMEBUFFER, Some(&frame_buffer));
        gl.framebuffer_texture_2d(
            GL::FRAMEBUFFER,
            GL::COLOR_ATTACHMENT0,
            GL::TEXTURE_2D,
            Some(&texture.texture),
            0,
        );
        gl.bind_framebuffer(GL::FRAMEBUFFER, None);

        GlFramebuffer {
            frame_buffer,
            texture,
            width,
            height,
            gl,
        }
    }

    pub fn bind(&self) {
        self.gl
            .viewport(0, 0, self.width as i32, self.height as i32);
        self.gl
            .bind_framebuffer(GL::FRAMEBUFFER, Some(&self.frame_buffer));
    }

    pub fn unbind(&self) {
        self.gl.bind_framebuffer(GL::FRAMEBUFFER, None);
    }

    pub fn texture(&self) -> &GlTexture {
        &self.texture
    }
}

impl Drop for GlFramebuffer {
    fn drop(&mut self) {
        self.gl.delete_framebuffer(Some(&self.frame_buffer));
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = ANGLEInstancedArrays)]
    type AngleInstancedArrays;

    #[wasm_bindgen(method, getter, js_name = VERTEX_ATTRIB_ARRAY_DIVISOR_ANGLE)]
    fn vertex_attrib_array_divisor_angle(this: &AngleInstancedArrays) -> i32;

    #[wasm_bindgen(method, catch, js_name = drawArraysInstancedANGLE)]
    fn draw_arrays_instanced_angle(
        this: &AngleInstancedArrays,
        mode: u32,
        first: i32,
        count: i32,
        primcount: i32,
    ) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = drawElementsInstancedANGLE)]
    fn draw_elements_instanced_angle(
        this: &AngleInstancedArrays,
        mode: u32,
        count: i32,
        type_: u32,
        offset: i32,
        primcount: i32,
    ) -> Result<(), JsValue>;

    #[wasm_bindgen(method, js_name = vertexAttribDivisorANGLE)]
    fn vertex_attrib_divisor_angle(this: &AngleInstancedArrays, index: u32, divisor: u32);
}
