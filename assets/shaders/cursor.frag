#version 450

#include "cursor_default.glsl"

layout(location = 0) in vec2 FragCoord;
layout(location = 0) out vec4 FragColor;

layout(push_constant) uniform CursorPush {
    vec4 rect;
    uvec4 state;
} pc;

float sdSegment(vec2 p, vec2 a, vec2 b) {
    vec2 pa = p - a;
    vec2 ba = b - a;
    float h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

bool pointInTriangle(vec2 p, vec2 p0, vec2 p1, vec2 p2) {
    float c0 = (p1.x - p0.x) * (p.y - p0.y)
        - (p1.y - p0.y) * (p.x - p0.x);
    float c1 = (p2.x - p1.x) * (p.y - p1.y)
        - (p2.y - p1.y) * (p.x - p1.x);
    float c2 = (p0.x - p2.x) * (p.y - p2.y)
        - (p0.y - p2.y) * (p.x - p2.x);
    return (c0 >= 0.0 && c1 >= 0.0 && c2 >= 0.0)
        || (c0 <= 0.0 && c1 <= 0.0 && c2 <= 0.0);
}

float lineMask(vec2 p, vec2 a, vec2 b, float width) {
    return 1.0 - smoothstep(width, width + 0.012, sdSegment(p, a, b));
}

float ringMask(vec2 p, vec2 center, float radius, float width) {
    return 1.0 - smoothstep(width, width + 0.012,
        abs(length(p - center) - radius));
}

void main() {
    // Version compacte et robuste : le pointeur normal GNOME/Adwaita.
    // Les autres types retombent volontairement sur cette forme en attendant
    // des modules de familles séparés, pour ne pas gonfler le pipeline Venus.
    uint row = min(uint(FragCoord.y * 24.0), 23u);
    uint column = min(uint(FragCoord.x * 24.0), 23u);
    uint bit = 1u << column;
    if ((GNOME_DEFAULT_MASK[row] & bit) == 0u) {
        discard;
    }
    FragColor = vec4(0.90, 0.08, 0.08, 1.0);
}
