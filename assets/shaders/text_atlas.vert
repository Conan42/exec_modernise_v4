#version 450

layout(push_constant) uniform TextPush {
    vec4 rect;
    vec4 atlas_rect;
    vec4 sizes; // screen width/height, then atlas width/height
    vec4 color;
} pc;

layout(location = 0) out vec2 texcoord;
layout(location = 1) out vec4 text_color;

const vec2 vertices[6] = vec2[](
    vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0)
);

void main() {
    vec2 local = vertices[gl_VertexIndex];
    vec2 pixel = pc.rect.xy + local * pc.rect.zw;
    // Le viewport Vulkan du framebuffer Venus utilise déjà l'origine en haut
    // à gauche. Les rectangles de la scène et le texte partagent donc la
    // même conversion écran -> NDC ; inverser Y ici déplaçait les glyphes
    // vers le bas du scanout.
    gl_Position = vec4(pixel.x / pc.sizes.x * 2.0 - 1.0,
                       pixel.y / pc.sizes.y * 2.0 - 1.0,
                       0.0, 1.0);
    // Les coordonnées sont les limites du rectangle de texels. Lorsque le
    // quad et le canevas ont la même taille en pixels, le centre de chaque
    // fragment tombe ainsi au centre du texel correspondant. Une contraction
    // d'un demi-texel décalait les lettres et ajoutait du flou bilinéaire.
    texcoord = (pc.atlas_rect.xy + local * pc.atlas_rect.zw) / pc.sizes.zw;
    text_color = pc.color;
}
