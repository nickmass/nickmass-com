attribute mat3 a_model_matrix;
attribute vec2 a_position;
attribute float a_alpha;
attribute vec3 a_color;

varying float v_alpha;
varying vec3 v_color;

uniform mat3 u_view_matrix;

void main() {
  vec3 pos = vec3(a_position, 1.0) * a_model_matrix * u_view_matrix;
  v_alpha = a_alpha;
  v_color = a_color;
  gl_Position = vec4(pos.xy / pos.z, 1.0, 1.0);
}
