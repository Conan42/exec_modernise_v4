#version 450

layout(set = 0, binding = 0) uniform sampler2D alpha_atlas;
layout(location = 0) in vec2 texcoord;
layout(location = 1) in vec4 text_color;
layout(location = 0) out vec4 out_color;

void main() {
    float coverage = texture(alpha_atlas, texcoord).r;
    out_color = vec4(text_color.rgb, text_color.a * coverage);
}
