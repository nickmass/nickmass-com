precision mediump float;

varying float v_alpha;
varying vec3 v_color;

uniform float u_alpha;

void main() {
  gl_FragColor = vec4(v_color * v_alpha * u_alpha, v_alpha * u_alpha);
}
