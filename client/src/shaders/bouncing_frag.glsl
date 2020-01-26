precision highp float;

varying float v_alpha;

void main() {
  vec3 color = vec3(v_alpha);
  gl_FragColor = vec4(color.xyz, 1.0);
}
