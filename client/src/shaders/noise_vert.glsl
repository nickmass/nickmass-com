precision highp float;

attribute vec2 a_position;
attribute vec2 a_uv;

uniform mat3 u_view_matrix;

varying vec2 v_uv;

void main() {
  vec3 pos = vec3(a_position, 1.0) * u_view_matrix;
  v_uv = (vec2(pos.x / pos.z, pos.y / pos.z * -1.0) + 1.0) / 2.0;
  gl_Position = vec4(pos.xy / pos.z, 1.0, 1.0);
}