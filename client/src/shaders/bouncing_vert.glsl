attribute vec2 a_position;
attribute vec2 a_normal;
attribute float a_line_width;
attribute float a_alpha;

varying float v_alpha;
varying vec2 v_normal;

uniform mat3 u_view_matrix;

void main() {
  vec2 position;
  if (a_line_width > 0.0) {
    position = a_position + (a_normal * a_line_width) / 2.0;
  } else {
    position = a_position;
  }
  vec3 pos = vec3(position, 1.0) * u_view_matrix;

  v_alpha = a_alpha;
  v_normal = a_normal;

  gl_Position = vec4(pos.xy / pos.z, 1.0, 1.0);
}
