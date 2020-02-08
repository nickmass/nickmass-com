precision mediump float;

varying float v_alpha;
varying vec2 v_normal;

void main() {
  vec3 color = vec3(v_alpha);
  gl_FragColor = vec4(color.xyz, 1.0 - smoothstep(0.4, 1.0, length(v_normal)));
}
