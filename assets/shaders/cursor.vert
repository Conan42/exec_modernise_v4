#version 450

layout(push_constant) uniform CursorPush {
    vec4 rect;
    uvec4 state;
} pc;

layout(location = 0) out vec2 FragCoord;

void main() {
    const vec2 corners[6] = vec2[6](
        vec2(0.0, 0.0), vec2(24.0, 0.0), vec2(24.0, 24.0),
        vec2(0.0, 0.0), vec2(24.0, 24.0), vec2(0.0, 24.0));
    const vec2 uv[6] = vec2[6](
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
    // Réduction visuelle autour de la pointe : le hotspot ne bouge pas.
    vec2 local = corners[gl_VertexIndex] * 0.65;
    // Hotspot du pointeur Adwaita/GNOME par défaut (3, 1).
    vec2 pixel = pc.rect.xy - vec2(3.0, 1.0) + local;
    // rect.zw est la taille du scanout. La convention d'origine basse est
    // celle déjà employée par le pass de composition V3/V1.
    gl_Position = vec4(pixel.x * 2.0 / pc.rect.z - 1.0,
                       pixel.y * 2.0 / pc.rect.w - 1.0, 0.0, 1.0);
    FragCoord = uv[gl_VertexIndex];
}
