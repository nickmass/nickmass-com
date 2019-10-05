use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    HtmlCanvasElement, WebGlBuffer, WebGlFramebuffer, WebGlProgram, WebGlRenderingContext as GL,
    WebGlRenderingContext, WebGlShader, WebGlTexture, WebGlUniformLocation,
};

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

pub struct GlContext<C = HtmlCanvasElement> {
    gl: WebGlRenderingContext,
    canvas: C,
    ext_map: RefCell<HashMap<TypeId, Option<Box<dyn Any>>>>,
}

impl GlContext {
    pub fn new(canvas: HtmlCanvasElement) -> Self {
        let gl = canvas
            .get_context("webgl")
            .unwrap_or(None)
            .and_then(|e| e.dyn_into::<WebGlRenderingContext>().ok())
            .unwrap();
        GlContext::with_gl(canvas, gl)
    }
}

impl<C> GlContext<C> {
    pub fn with_gl(canvas: C, gl: WebGlRenderingContext) -> Self {
        GlContext {
            gl,
            canvas,
            ext_map: RefCell::new(HashMap::new()),
        }
    }

    pub fn canvas(&self) -> &C {
        &self.canvas
    }

    pub fn load_extension<E: GlExtension>(&self) -> Option<E> {
        let key = TypeId::of::<E>();
        let mut map = self.ext_map.borrow_mut();
        let entry = map.entry(key).or_insert_with(|| {
            self.gl
                .get_extension(E::EXT_NAME)
                .transpose()
                .and_then(|r| r.ok())
                .map(|e| Box::new(e.unchecked_into::<E>()) as Box<dyn Any>)
        });

        entry.as_ref().and_then(|e| e.downcast_ref::<E>()).cloned()
    }
}

impl<C> std::ops::Deref for GlContext<C> {
    type Target = WebGlRenderingContext;

    fn deref(&self) -> &Self::Target {
        &self.gl
    }
}

pub trait GlExtension: Any + Clone + JsCast {
    const EXT_NAME: &'static str;
}

impl GlExtension for OesVertexArrayObject {
    const EXT_NAME: &'static str = "OES_vertex_array_object";
}

impl GlExtension for AngleInstancedArrays {
    const EXT_NAME: &'static str = "ANGLE_instanced_arrays";
}

pub struct GlProgram<'ctx> {
    gl: &'ctx GlContext,
    program: WebGlProgram,
    vertex_shader: WebGlShader,
    fragment_shader: WebGlShader,
    texture_unit: Cell<u32>,
    vao_ext: OesVertexArrayObject,
    vao_map: HashMap<(u64, TypeId, TypeId), WebGlVertexArrayObject>,
    uniform_map: HashMap<&'static str, Option<WebGlUniformLocation>>,
}

impl<'ctx> GlProgram<'ctx> {
    pub fn new(
        gl: &'ctx GlContext,
        vertex_shader: impl AsRef<str>,
        fragment_shader: impl AsRef<str>,
    ) -> GlProgram<'ctx> {
        let shader_vert = gl
            .create_shader(GL::VERTEX_SHADER)
            .expect("Valid Vertex Shader");
        gl.shader_source(&shader_vert, vertex_shader.as_ref());
        gl.compile_shader(&shader_vert);
        let info = gl.get_shader_info_log(&shader_vert);
        if let Some(info) = info {
            if info.len() > 0 {
                log::warn!("Vertex Shader: {}\n{}", info, vertex_shader.as_ref());
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
                log::warn!("Fragment Shader: {}\n{}", info, fragment_shader.as_ref());
            }
        }

        let prog = gl.create_program().expect("Create GL Program");
        gl.attach_shader(&prog, &shader_vert);
        gl.attach_shader(&prog, &shader_frag);
        gl.link_program(&prog);

        let info = gl.get_program_info_log(&prog);
        if let Some(info) = info {
            if info.len() > 0 {
                log::warn!(
                    "Program Shader: {} {} {}",
                    info,
                    vertex_shader.as_ref(),
                    fragment_shader.as_ref()
                );
            }
        }

        let vao_ext = gl.load_extension().expect("Oes VAO extension");

        GlProgram {
            gl,
            program: prog,
            texture_unit: Cell::new(0),
            vertex_shader: shader_vert,
            fragment_shader: shader_frag,
            vao_ext,
            vao_map: HashMap::new(),
            uniform_map: HashMap::new(),
        }
    }

    pub fn draw<V>(&mut self, model: &GlModel<V>, uniforms: &GlUniformCollection)
    where
        V: AsGlVertex,
    {
        self.gl.use_program(Some(&self.program));

        let key = (model.id, TypeId::of::<V>(), TypeId::of::<()>());
        if let Some(vao) = self.vao_map.get(&key) {
            self.vao_ext
                .bind_vertex_array_oes(Some(vao))
                .expect("Bind vao");
        } else {
            let vao = self.vao_ext.create_vertex_array_oes().expect("Create vao");
            self.vao_map.insert(key, vao);
            let vao = self.vao_map.get(&key).unwrap();

            self.vao_ext
                .bind_vertex_array_oes(Some(vao))
                .expect("Bind vao");
            model.fill_vao(&self);
        }

        self.bind_uniforms(uniforms);
        model.draw();

        self.vao_ext
            .bind_vertex_array_oes(None)
            .expect("Unbind vao");
        self.reset_texture_unit();
    }

    pub fn draw_instanced<V, I>(
        &mut self,
        model: &GlModel<V>,
        instanced_data: impl IntoIterator<Item = I, IntoIter = impl ExactSizeIterator<Item = I>>,
        uniforms: &GlUniformCollection,
    ) where
        V: AsGlVertex,
        I: AsGlVertex,
    {
        self.gl.use_program(Some(&self.program));

        let key = (model.id, TypeId::of::<V>(), TypeId::of::<I>());
        if let Some(vao) = self.vao_map.get(&key) {
            self.vao_ext
                .bind_vertex_array_oes(Some(vao))
                .expect("Bind vao");
        } else {
            let vao = self.vao_ext.create_vertex_array_oes().expect("Create vao");
            self.vao_map.insert(key, vao);
            let vao = self.vao_map.get(&key).unwrap();

            self.vao_ext
                .bind_vertex_array_oes(Some(vao))
                .expect("Bind vao");
            model.fill_vao_instanced::<I>(&self);
        }

        self.bind_uniforms(uniforms);
        model.draw_instanced(instanced_data);

        self.vao_ext
            .bind_vertex_array_oes(None)
            .expect("Unbind vao");
        self.reset_texture_unit();
    }

    fn bind_uniforms(&mut self, uniforms: &GlUniformCollection) {
        for (k, v) in &uniforms.uniforms {
            let location = if let Some(location) = self.uniform_map.get(k) {
                location
            } else {
                let location = self.gl.get_uniform_location(&self.program, k);
                self.uniform_map.insert(k, location);
                self.uniform_map.get(k).unwrap()
            };
            if location.is_some() {
                v.bind(&self.gl, &self, location.as_ref());
            }
        }
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

impl<'ctx> Drop for GlProgram<'ctx> {
    fn drop(&mut self) {
        for (_k, v) in self.vao_map.drain() {
            let _ = self.vao_ext.delete_vertex_array_oes(v);
        }

        self.gl.detach_shader(&self.program, &self.vertex_shader);
        self.gl.detach_shader(&self.program, &self.fragment_shader);

        self.gl.delete_shader(Some(&self.vertex_shader));
        self.gl.delete_shader(Some(&self.fragment_shader));

        self.gl.delete_program(Some(&self.program));
    }
}

pub struct GlUniformCollection<'a> {
    uniforms: HashMap<&'static str, &'a dyn AsGlUniform>,
}

impl<'a> GlUniformCollection<'a> {
    pub fn new() -> GlUniformCollection<'a> {
        GlUniformCollection {
            uniforms: HashMap::new(),
        }
    }

    pub fn add(&mut self, name: &'static str, uniform: &'a dyn AsGlUniform) -> &mut Self {
        self.uniforms.insert(name, uniform);

        self
    }
}

pub trait AsGlUniform {
    fn bind(&self, gl: &GL, program: &GlProgram, location: Option<&WebGlUniformLocation>);
}

impl AsGlUniform for f32 {
    fn bind(&self, gl: &GL, _program: &GlProgram, location: Option<&WebGlUniformLocation>) {
        gl.uniform1f(location, *self);
    }
}

impl AsGlUniform for [f32; 2] {
    fn bind(&self, gl: &GL, _program: &GlProgram, location: Option<&WebGlUniformLocation>) {
        gl.uniform2fv_with_f32_array(location, &self[..]);
    }
}

impl AsGlUniform for (f32, f32) {
    fn bind(&self, gl: &GL, program: &GlProgram, location: Option<&WebGlUniformLocation>) {
        [self.0, self.1].bind(gl, program, location);
    }
}

impl AsGlUniform for [f32; 3] {
    fn bind(&self, gl: &GL, _program: &GlProgram, location: Option<&WebGlUniformLocation>) {
        gl.uniform3fv_with_f32_array(location, &self[..]);
    }
}

impl AsGlUniform for [f32; 9] {
    fn bind(&self, gl: &GL, _program: &GlProgram, location: Option<&WebGlUniformLocation>) {
        gl.uniform_matrix3fv_with_f32_array(location, false, &self[..]);
    }
}

impl<'ctx> AsGlUniform for GlTexture<'ctx> {
    fn bind(&self, gl: &GL, program: &GlProgram, location: Option<&WebGlUniformLocation>) {
        let texture_unit = program.next_texture_unit();
        gl.active_texture(GL::TEXTURE0 + texture_unit);
        gl.bind_texture(GL::TEXTURE_2D, Some(&self.texture));

        gl.uniform1i(location, texture_unit as i32);
    }
}

pub struct GlModel<'ctx, V: AsGlVertex> {
    gl: &'ctx GlContext,
    id: u64,
    buffer: WebGlBuffer,
    instanced_buffer: WebGlBuffer,
    poly_type: u32,
    poly_count: i32,
    instanced_ext: AngleInstancedArrays,
    _marker: std::marker::PhantomData<V>,
}

impl<'ctx, V: AsGlVertex> GlModel<'ctx, V> {
    pub fn new(gl: &'ctx GlContext, vertexes: impl IntoIterator<Item = V>) -> GlModel<'ctx, V> {
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

        let instanced_buffer = gl.create_buffer().expect("Gl Instance Buffer");

        let instanced_ext = gl.load_extension().expect("Angle instaned arrays");

        GlModel {
            gl,
            id: rand::random(),
            buffer,
            poly_type,
            poly_count,
            instanced_buffer,
            instanced_ext,
            _marker: Default::default(),
        }
    }

    fn fill_vao(&self, program: &GlProgram) {
        self.gl.bind_buffer(GL::ARRAY_BUFFER, Some(&self.buffer));
        self.enable_attrs::<V>(program, None);
    }

    fn fill_vao_instanced<I: AsGlVertex>(&self, program: &GlProgram) {
        self.gl.bind_buffer(GL::ARRAY_BUFFER, Some(&self.buffer));
        self.enable_attrs::<V>(program, Some(0));
        self.gl
            .bind_buffer(GL::ARRAY_BUFFER, Some(&self.instanced_buffer));
        self.enable_attrs::<I>(program, Some(1));
    }

    fn enable_attrs<I: AsGlVertex>(&self, program: &GlProgram, divisor: Option<u32>) {
        let stride = I::ATTRIBUTES.iter().map(|a| a.1.size()).sum();
        let mut offset = 0;
        for (name, vtype) in I::ATTRIBUTES {
            let location = self.gl.get_attrib_location(&program.program, name);
            if location < 0 {
                continue;
            }
            let location = location as u32;
            if let Some(divisor) = divisor {
                for i in 0..vtype.elements() {
                    self.instanced_ext
                        .vertex_attrib_divisor_angle(location + i, divisor);
                }
            }
            vtype.layout(&self.gl, location, stride, offset);
            vtype.enable(&self.gl, location);
            offset += vtype.size();
        }
    }

    fn draw(&self) {
        self.gl.draw_arrays(self.poly_type, 0, self.poly_count);
    }

    fn draw_instanced<I: AsGlVertex>(
        &self,
        instance_vertexes: impl IntoIterator<Item = I, IntoIter = impl ExactSizeIterator<Item = I>>,
    ) {
        self.gl
            .bind_buffer(GL::ARRAY_BUFFER, Some(&self.instanced_buffer));

        let iter = instance_vertexes.into_iter();
        let count = iter.len();
        let mut data = Vec::with_capacity(count * I::SIZE);
        for v in iter {
            v.write(&mut data);
        }

        self.gl
            .buffer_data_with_u8_array(GL::ARRAY_BUFFER, data.as_slice(), GL::DYNAMIC_DRAW);

        self.instanced_ext
            .draw_arrays_instanced_angle(self.poly_type, 0, self.poly_count, count as i32)
            .expect("Instanced Draw");
    }
}

impl<'ctx, V: AsGlVertex> Drop for GlModel<'ctx, V> {
    fn drop(&mut self) {
        self.gl.delete_buffer(Some(&self.buffer));
        self.gl.delete_buffer(Some(&self.instanced_buffer));
    }
}

pub trait AsGlVertex: Any {
    const ATTRIBUTES: &'static [(&'static str, GlValueType)];
    const POLY_TYPE: u32;
    const SIZE: usize;

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

pub struct GlTexture<'ctx> {
    gl: &'ctx GlContext,
    texture: WebGlTexture,
}

impl<'ctx> GlTexture<'ctx> {
    pub fn new(gl: &'ctx GlContext, width: u32, height: u32) -> GlTexture<'ctx> {
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

impl<'ctx> Drop for GlTexture<'ctx> {
    fn drop(&mut self) {
        self.gl.delete_texture(Some(&self.texture));
    }
}

pub struct GlFramebuffer<'ctx> {
    gl: &'ctx GlContext,
    texture: GlTexture<'ctx>,
    frame_buffer: WebGlFramebuffer,
    width: u32,
    height: u32,
}

impl<'ctx> GlFramebuffer<'ctx> {
    pub fn new(gl: &'ctx GlContext, width: u32, height: u32) -> GlFramebuffer<'ctx> {
        let texture = GlTexture::new(gl, width, height);
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

impl<'ctx> Drop for GlFramebuffer<'ctx> {
    fn drop(&mut self) {
        self.gl.delete_framebuffer(Some(&self.frame_buffer));
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = ANGLEInstancedArrays)]
    #[derive(Clone)]
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

    #[wasm_bindgen(js_name = OESVertexArrayObject)]
    #[derive(Clone)]
    type OesVertexArrayObject;

    #[wasm_bindgen(js_name = WebGLVertexArrayObject)]
    type WebGlVertexArrayObject;

    #[wasm_bindgen(method, getter, js_name = VERTEX_ATTRIB_ARRAY_BINDING_OES)]
    fn vertex_attrib_array_binding_oes(this: &OesVertexArrayObject) -> i32;

    #[wasm_bindgen(method, catch, js_name = createVertexArrayOES)]
    fn create_vertex_array_oes(
        this: &OesVertexArrayObject,
    ) -> Result<WebGlVertexArrayObject, JsValue>;

    #[wasm_bindgen(method, catch, js_name = deleteVertexArrayOES)]
    fn delete_vertex_array_oes(
        this: &OesVertexArrayObject,
        vao: WebGlVertexArrayObject,
    ) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = bindVertexArrayOES)]
    fn bind_vertex_array_oes(
        this: &OesVertexArrayObject,
        vao: Option<&WebGlVertexArrayObject>,
    ) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch, js_name = isVertexArrayOES)]
    fn is_vertex_array_oes(
        this: &OesVertexArrayObject,
        vao: &WebGlVertexArrayObject,
    ) -> Result<bool, JsValue>;

}
