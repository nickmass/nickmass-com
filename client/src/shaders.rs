use crate::gl::*;

pub trait ShaderExt<'ctx> {
    fn with_shader<S: Shader>(gl: &'ctx GlContext) -> Self;
}

impl<'ctx> ShaderExt<'ctx> for GlProgram<'ctx> {
    fn with_shader<S: Shader>(gl: &'ctx GlContext) -> Self {
        GlProgram::new(gl, S::VERTEX, S::FRAGMENT)
    }
}

pub trait Shader {
    const FRAGMENT: &'static str;
    const VERTEX: &'static str;
}

macro_rules! shader(($name:ident, $path:expr) => {
pub struct $name;

impl Shader for $name {
    const FRAGMENT: &'static str = include_str!(concat!("shaders/", $path, "_frag.glsl"));
    const VERTEX: &'static str = include_str!(concat!("shaders/", $path, "_vert.glsl"));
}
});

shader!(QuadShader, "quad");
shader!(NoiseShader, "noise");
shader!(LogoShader, "logo");
shader!(CircleShader, "circle");
shader!(BouncingShader, "bouncing");
shader!(BlurShader, "blur");
shader!(BallShader, "ball");
