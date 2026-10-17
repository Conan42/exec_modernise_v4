#version 450

layout(push_constant) uniform RoundedPush {
    vec4 rect;
    vec4 shape; // radius, border width, relief mode, unused
    vec4 color;
    vec4 target; // screen width/height, unused, unused
} pc;

layout(location = 0) in vec2 local_position;
layout(location = 0) out vec4 out_color;

const vec3 RELIEF_LIGHT_COLOR = vec3(0.5764706);
const vec3 RELIEF_DARK_COLOR = vec3(0.0);

float rounded_box_sdf(vec2 point, vec2 half_size, float radius) {
    vec2 q = abs(point) - half_size + vec2(radius);
    return length(max(q, vec2(0.0)))
        + min(max(q.x, q.y), 0.0)
        - radius;
}

void main() {
    vec2 half_size = pc.rect.zw * 0.5;
    // Keep the same center for the outer and inner shapes. Using the inner
    // half-size as a new center removes the top and left half of the border.
    vec2 centered_position = local_position - half_size;
    float radius = min(pc.shape.x, min(half_size.x, half_size.y));
    // Mode 3 is reserved for LEDs. Their silhouette is a true disk based on
    // the actual rendered rectangle, instead of a rounded rectangle whose
    // corner radius can make a small LED look square when enlarged.
    float outer_distance = pc.shape.z > 2.5
        ? length(centered_position) - min(half_size.x, half_size.y)
        : rounded_box_sdf(centered_position, half_size, radius);
    float aa = max(fwidth(outer_distance), 0.001);
    float outer = 1.0 - smoothstep(0.0, aa, outer_distance);

    float border = max(pc.shape.y, 0.0);
    float coverage = outer;
    if (border > 0.0) {
        vec2 inner_half_size = max(half_size - vec2(border), vec2(0.0));
        float inner_radius = max(radius - border, 0.0);
        float inner_distance = rounded_box_sdf(
            centered_position, inner_half_size, inner_radius);
        float inner_aa = max(fwidth(inner_distance), 0.001);
        float inner = 1.0 - smoothstep(0.0, inner_aa, inner_distance);
        coverage = clamp(outer - inner, 0.0, 1.0);
    }
    if (coverage <= 0.0) {
        discard;
    }
    vec3 border_color = pc.color.rgb;
    if (border > 0.0 && pc.shape.z > 0.5) {
        // Relief mode 1 is the normal button state: light top/left and
        // dark bottom/right. Mode 2 reverses those sides while pressed.
        float distance_left = local_position.x;
        float distance_right = pc.rect.z - local_position.x;
        float distance_top = local_position.y;
        float distance_bottom = pc.rect.w - local_position.y;
        // Blend the two adjacent sides in a rounded corner. Choosing only
        // the nearest side leaves a bright isolated pixel at the joins.
        float left_weight = 1.0 / max(distance_left, 0.25);
        float right_weight = 1.0 / max(distance_right, 0.25);
        float top_weight = 1.0 / max(distance_top, 0.25);
        float bottom_weight = 1.0 / max(distance_bottom, 0.25);
        float weight_sum = left_weight + right_weight
            + top_weight + bottom_weight;
        float dark_amount = (right_weight + bottom_weight) / weight_sum;
        bool pressed = pc.shape.z > 1.5;
        bool top_left_corner = local_position.x < pc.shape.x
            && local_position.y < pc.shape.x;
        bool bottom_right_corner = local_position.x > pc.rect.z - pc.shape.x
            && local_position.y > pc.rect.w - pc.shape.x;
        dark_amount = pressed ? 1.0 - dark_amount : dark_amount;
        // These are the two corners where both adjacent sides are dark in
        // the corresponding relief state. Keep them dark instead of letting
        // the distant light sides create a bright isolated pixel.
        if ((pressed && top_left_corner)
            || (!pressed && bottom_right_corner)) {
            dark_amount = 1.0;
        }
        border_color = mix(RELIEF_LIGHT_COLOR, RELIEF_DARK_COLOR, dark_amount);
    }
    out_color = vec4(border_color, pc.color.a * coverage);
}
