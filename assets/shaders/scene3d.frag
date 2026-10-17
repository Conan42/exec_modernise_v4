#version 450

layout(location = 0) flat in uint face_index;
layout(location = 0) out vec4 out_color;

const vec3 colors[6] = vec3[](
    vec3(0.15, 0.55, 0.95),
    vec3(0.10, 0.30, 0.65),
    vec3(0.20, 0.80, 0.55),
    vec3(0.08, 0.40, 0.28),
    vec3(0.95, 0.55, 0.18),
    vec3(0.65, 0.25, 0.08)
);

void main() {
    out_color = vec4(colors[face_index % 6u], 1.0);
}
