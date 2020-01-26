precision highp float;

attribute mat3 a_model_matrix;
attribute vec2 a_position;

uniform mat3 u_view_matrix;

void main() {
  vec3 pos = vec3(a_position, 1.0) * a_model_matrix * u_view_matrix;
  gl_Position = vec4(pos.xy / pos.z, 1.0, 1.0);
}
