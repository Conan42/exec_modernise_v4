#version 450

layout(push_constant) uniform RoundedPush {
    vec4 rect;
    vec4 shape; // radius, border width, unused, unused
    vec4 color;
    vec4 target; // screen width/height, unused, unused
} pc;

const vec2 vertices[6] = vec2[](
    vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0)
);

layout(location = 0) out vec2 local_position;

void main() {
    vec2 local = vertices[gl_VertexIndex];
    vec2 pixel = pc.rect.xy + local * pc.rect.zw;
    gl_Position = vec4(pixel.x / pc.target.x * 2.0 - 1.0,
                       pixel.y / pc.target.y * 2.0 - 1.0,
                       0.0, 1.0);
    local_position = local * pc.rect.zw;
}
