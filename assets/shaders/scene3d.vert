#version 450

layout(push_constant) uniform ScenePush {
    vec4 rect;
    vec4 rotation;
    vec4 target;
    vec4 reserved;
} pc;

const vec3 vertices[36] = vec3[](
    vec3(-1.0, -1.0,  1.0), vec3( 1.0, -1.0,  1.0), vec3( 1.0,  1.0,  1.0),
    vec3(-1.0, -1.0,  1.0), vec3( 1.0,  1.0,  1.0), vec3(-1.0,  1.0,  1.0),
    vec3( 1.0, -1.0, -1.0), vec3(-1.0, -1.0, -1.0), vec3(-1.0,  1.0, -1.0),
    vec3( 1.0, -1.0, -1.0), vec3(-1.0,  1.0, -1.0), vec3( 1.0,  1.0, -1.0),
    vec3(-1.0,  1.0,  1.0), vec3( 1.0,  1.0,  1.0), vec3( 1.0,  1.0, -1.0),
    vec3(-1.0,  1.0,  1.0), vec3( 1.0,  1.0, -1.0), vec3(-1.0,  1.0, -1.0),
    vec3(-1.0, -1.0, -1.0), vec3( 1.0, -1.0, -1.0), vec3( 1.0, -1.0,  1.0),
    vec3(-1.0, -1.0, -1.0), vec3( 1.0, -1.0,  1.0), vec3(-1.0, -1.0,  1.0),
    vec3( 1.0, -1.0,  1.0), vec3( 1.0, -1.0, -1.0), vec3( 1.0,  1.0, -1.0),
    vec3( 1.0, -1.0,  1.0), vec3( 1.0,  1.0, -1.0), vec3( 1.0,  1.0,  1.0),
    vec3(-1.0, -1.0, -1.0), vec3(-1.0,  1.0, -1.0), vec3(-1.0,  1.0,  1.0),
    vec3(-1.0, -1.0, -1.0), vec3(-1.0,  1.0,  1.0), vec3(-1.0, -1.0,  1.0)
);

layout(location = 0) flat out uint face_index;

void main() {
    vec3 p = vertices[gl_VertexIndex];
    float yaw = pc.rotation.x;
    float pitch = pc.rotation.y;
    float cy = cos(yaw);
    float sy = sin(yaw);
    float cp = cos(pitch);
    float sp = sin(pitch);
    p = vec3(cy * p.x + sy * p.z, p.y, -sy * p.x + cy * p.z);
    p = vec3(p.x, cp * p.y - sp * p.z, sp * p.y + cp * p.z);

    float depth = p.z + 4.0;
    float scale = min(pc.rect.z, pc.rect.w) * 0.22;
    vec2 pixel = pc.rect.xy + pc.rect.zw * 0.5 + p.xy * scale / depth;
    gl_Position = vec4(pixel.x / pc.target.x * 2.0 - 1.0,
                       pixel.y / pc.target.y * 2.0 - 1.0,
                       p.z / 8.0,
                       1.0);
    face_index = uint(gl_VertexIndex / 6);
}
