precision mediump float;

varying float v_offset;

void main() {
    vec3 color = vec3(0.0, 0.0, 0.0);
    gl_FragColor = vec4(color.xyz, 1.0 - smoothstep(0.8, 1.0, v_offset));
}
