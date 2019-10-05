precision highp float;

varying vec2 v_uv;
uniform float u_time;
uniform vec2 u_mouse;
uniform vec2 u_resolution;

vec2 random2(in vec2 st) {
  st = vec2(dot(st, vec2(127.1, 311.7)), dot(st, vec2(269.5, 183.3)));
  return fract(sin(st) * 43758.5453) * 10.0;
}

float noise(in vec2 st) {
  vec2 i = floor(st);
  vec2 f = fract(st);

  vec2 u = smoothstep(0.0, 1.0, f);

  return mix( mix( dot( random2(i + vec2(0.0, 0.0)), f - vec2(0.0, 0.0)),
                   dot( random2(i + vec2(1.0, 0.0)), f - vec2(1.0, 0.0)), u.x),
              mix( dot( random2(i + vec2(0.0, 1.0)), f - vec2(0.0, 1.0)),
                   dot( random2(i + vec2(1.0, 1.0)), f - vec2(1.0, 1.0)), u.x), u.y);
}

void main() {
  //vec2 st = v_uv * vec2(2.0, 1.25);
  vec2 st = gl_FragCoord.xy/u_resolution.xy;
  st.x *= u_resolution.x/u_resolution.y;
  st = v_uv * vec2(2.0, 1.25);
  st.y += u_time/5000.0;
  st += u_mouse * vec2(1.0, -1.0) / 3.0;

  float t = u_time * 0.0001;

  //t = abs(1.0-sin(u_time*.0001) - 0.5) * 2.0;
  t = 0.5 + abs(sin(t));

  st += noise(st * 2.0)*t;
  vec3 color = vec3(1.0) * smoothstep(0.38, 0.68, noise(st * 0.8));
  //color += smoothstep(0.15, 0.2, noise(st * 5.0));
  //color -= smoothstep(0.35, 0.4, noise(st * 5.0));
  //color = vec3(noise(st));

  gl_FragColor = vec4(color, 1.0);
}