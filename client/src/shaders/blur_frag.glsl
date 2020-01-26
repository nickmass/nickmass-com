precision highp float;

uniform vec2 u_dimensions;
const int length = 49;
uniform float u_gaussian[length];
uniform float u_blur_vert;
uniform sampler2D u_source;

vec3 convol_point(in vec2 offset, in float factor) {
  return texture2D(u_source, (gl_FragCoord.xy + offset) / u_dimensions).xyz * factor;
}

void main() {
  vec3 color = vec3(0.0, 0.0, 0.0);

  float blur_horz = 1.0 - u_blur_vert;

  for (int x = 0; x < length; x++) {
    float offset = float(x - ((length / 2) + 1));
    color += convol_point(vec2(blur_horz * offset, u_blur_vert * offset), u_gaussian[x]);
  }

  gl_FragColor = vec4(color.xyz, 1.0);
}

