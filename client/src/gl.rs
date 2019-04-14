use log::info;
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
        model.draw();
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

        let (poly_type, poly_count) = V::poly_info(count);

        GlModel {
            buffer,
            gl,
            poly_type,
            poly_count,
            _marker: Default::default(),
        }
    }

    fn draw(&self) {
        self.gl.bind_buffer(GL::ARRAY_BUFFER, Some(&self.buffer));
        V::bind_attrs(&self.gl);
        self.gl.draw_arrays(self.poly_type, 0, self.poly_count);
        V::unbind_attrs(&self.gl);
    }
}

impl<V: AsGlVertex> Drop for GlModel<V> {
    fn drop(&mut self) {
        self.gl.delete_buffer(Some(&self.buffer));
    }
}

pub trait AsGlVertex {
    fn write(&self, buf: impl std::io::Write);

    fn bind_attrs(gl: &GL);

    fn unbind_attrs(gl: &GL);

    fn poly_info(len: usize) -> (u32, i32);
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
