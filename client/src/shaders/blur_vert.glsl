precision highp float;

attribute vec2 a_position;

uniform mat3 u_view_matrix;
uniform mat3 u_model_matrix;

void main() {
  vec3 pos = vec3(a_position, 1.0) * u_model_matrix * u_view_matrix;
  gl_Position = vec4(pos.xy / pos.z, 1.0, 1.0);
}
