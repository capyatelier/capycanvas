override MATERIAL_OPERATION: u32;
override MATERIAL_NORMAL_BLEND: bool = false;

const OP_DEPOSIT: u32 = 0u;
const OP_COVERAGE: u32 = 1u;
const OP_LIQUIFY: u32 = 2u;
const OP_SMUDGE: u32 = 3u;
const OP_WET: u32 = 4u;
const OP_WATERCOLOR: u32 = 5u;
const OP_CLONE: u32 = 6u;

struct Target {
    origin_extent: vec4<f32>,
    document_extent: vec4<f32>,
}

@group(0) @binding(0) var<uniform> style: Style;
@group(1) @binding(0) var<uniform> render_target: Target;

@group(2) @binding(0) var source_00: texture_2d<f32>;
@group(2) @binding(1) var source_10: texture_2d<f32>;
@group(2) @binding(2) var source_20: texture_2d<f32>;
@group(2) @binding(3) var source_01: texture_2d<f32>;
@group(2) @binding(4) var source_11: texture_2d<f32>;
@group(2) @binding(5) var source_21: texture_2d<f32>;
@group(2) @binding(6) var source_02: texture_2d<f32>;
@group(2) @binding(7) var source_12: texture_2d<f32>;
@group(2) @binding(8) var source_22: texture_2d<f32>;
@group(2) @binding(9) var<storage, read> dabs: array<Dab>;
@group(2) @binding(10) var stroke_coverage_texture: texture_2d<f32>;
@group(2) @binding(11) var reservoir_texture: texture_2d<f32>;
struct MaterialSources {
    // 0: adjacent pages; 1: disjoint gather pages; 2: gathered sample field.
    // Adjacent pages also provide the ordered dry-contact range in zw. Gather
    // pages count y pages; z adds the field gathered by earlier chunks. A
    // watercolor job's w writes the whole wetness page.
    header: vec4<u32>,
    // For page kernels, pages[0] is the local dirty rectangle (min xy, max xy).
    // A clone's pages[1..5] map it to its source, whose 2x2 blocks of target
    // and reference pages it binds around source_11. The reservoir exchange's
    // pages[0] holds the bits of a new stroke's starting reservoir.
    pages: array<vec4<u32>, 9>,
}
@group(2) @binding(12) var<uniform> material_sources: MaterialSources;

struct MaterialOutput {
    color: vec4<f32>,
    coverage: vec4<f32>,
    wetness: vec4<f32>,
}

const MATERIAL_REPAINT: u32 = 1u;
const MATERIAL_HELD: u32 = 2u;
struct MaterialContact { pigment:vec3<f32>, coverage:f32, start:f32, flags:u32 }

fn contact_coverage(dab: Dab, world: vec2<f32>) -> f32 {
    return contact_coverage_field(dab, world, contact_field(world));
}

fn contact_coverage_field(dab: Dab, world: vec2<f32>, field: vec2<f32>) -> f32 {
    return brush_footprint(dab, world, field) * brush_selection_at(world);
}

fn brush_blend() -> u32 {
    return u32(style.render_mode.x);
}

fn deposit_encoded() -> bool {
    return (brush_blend() & BLEND_PERCEPTUAL) != 0u && style.operation.w == 0u;
}

fn deposit_color(color: vec3<f32>) -> vec3<f32> {
    if deposit_encoded() {
        return sdr_encode(color, WORKING_SPACE);
    }
    return color;
}

fn deposit_paint(paint: vec4<f32>) -> vec4<f32> {
    if deposit_encoded() {
        return working_encode(paint);
    }
    return paint;
}

fn stored_paint(paint: vec4<f32>) -> vec4<f32> {
    if deposit_encoded() {
        return working_decode(paint);
    }
    return paint;
}

fn source_over(destination: vec4<f32>, source_color: vec3<f32>, source_alpha: f32) -> vec4<f32> {
    let da = destination.a;
    // Normal blending has no backdrop-color term. Besides avoiding an
    // unassociation and two redundant products, keep this direct form: the
    // expanded general expression produces dark contact edges in specialized
    // shaders on the Wacom's Adreno Vulkan driver (including native saved paint).
    if MATERIAL_NORMAL_BLEND {
        if style.color.a > 0.5 {
            return vec4<f32>(mix(destination.rgb, source_color * da, source_alpha), da);
        }
        return vec4<f32>(
            destination.rgb * (1.0 - source_alpha) + source_color * source_alpha,
            source_alpha + da * (1.0 - source_alpha),
        );
    }
    let backdrop = working_unassociate(destination);
    let blended = blend(source_color, backdrop, brush_blend());
    if style.color.a > 0.5 {
        return vec4<f32>(mix(destination.rgb, blended * da, source_alpha), da);
    }
    let rgb = destination.rgb * (1.0 - source_alpha)
        + source_color * source_alpha * (1.0 - da)
        + blended * source_alpha * da;
    return vec4<f32>(rgb, source_alpha + da * (1.0 - source_alpha));
}

fn stroke_increment(previous: f32, next: f32) -> f32 {
    let opacity = style.canvas_opacity.z;
    return clamp(working_ratio(opacity * (next - previous), 1.0 - opacity * previous), 0.0, 1.0);
}

fn grouped_opacity() -> bool {
    return style.canvas_opacity.z < 1.0
        && (MATERIAL_OPERATION == OP_DEPOSIT || MATERIAL_OPERATION == OP_COVERAGE || MATERIAL_OPERATION == OP_CLONE);
}

fn deposit_unchanged(paint: vec4<f32>, background: vec4<f32>, pigment: vec3<f32>) -> bool {
    if grouped_opacity() {
        let source = select(vec4<f32>(pigment,1.0),vec4<f32>(0.0),style.operation.w!=0u);
        let limit_color = background*(1.0-style.canvas_opacity.z)+source*style.canvas_opacity.z;
        if paint.a!=limit_color.a { return false; }
        return all(paint==stored_paint(limit_color));
    }
    return (style.operation.w!=0u && all(paint==vec4<f32>(0.0)))
        || (style.operation.w==0u && paint.a==1.0 && all(paint.rgb==style.material_b.xyz));
}

fn grouped_deposit(result: vec4<f32>, background: vec4<f32>, pigment: vec3<f32>, alpha: f32) -> vec4<f32> {
    let opacity = style.canvas_opacity.z;
    if opacity<=0.0 { return result; }
    if style.operation.w != 0u {
        return result * (1.0-alpha) + background * ((1.0-opacity)*alpha);
    }
    if MATERIAL_NORMAL_BLEND {
        let source = select(vec4<f32>(pigment,1.0),vec4<f32>(pigment*background.a,background.a),style.color.a>0.5);
        return mix(result,background*(1.0-opacity)+source*opacity,alpha);
    }
    let full = (result-background*(1.0-opacity))/opacity;
    return mix(background,source_over(full,pigment,alpha),opacity);
}

fn material_alpha_locked(result:MaterialOutput, original:vec4<f32>)->MaterialOutput {
    var locked=result;
    if style.operation.w!=0u {locked.color=original;}
    else {locked.color=vec4<f32>(working_unassociate(result.color)*original.a,original.a);}
    return locked;
}

fn pointwise_aggregate()->bool {
    return (MATERIAL_OPERATION==OP_DEPOSIT || MATERIAL_OPERATION==OP_COVERAGE)
        && contact_feature(64u,style.contact_a.z>0.5) && !contact_uniform()
        && contact_feature(8192u,style.material_a.w>0.5) && style.material_a.w>0.5;
}
