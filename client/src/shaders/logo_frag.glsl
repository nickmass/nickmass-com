precision highp float;

varying vec2 v_uv;

uniform sampler2D u_frame_sampler;

void main() {
  vec4 frame_color = texture2D(u_frame_sampler, v_uv);
  vec4 logo_color = (1.0 - frame_color) * 1.8;
  gl_FragColor = vec4(logo_color.xyz, 1.0);
}