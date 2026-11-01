fn pointwise_paint_fragment(fragment_position:vec4<f32>)->MaterialOutput {
    let world=render_target.origin_extent.xy+fragment_position.xy;

    var result = dry_original(vec2<i32>(floor(fragment_position.xy)));
    let state_coordinate = clamp(
        vec2<i32>(floor(fragment_position.xy)),
        vec2<i32>(0),
        vec2<i32>(255),
    );
    var stroke_coverage = dry_coverage(state_coordinate);
    if style.canvas_opacity.w > 0.5 {
        stroke_coverage = 0.0;
    }
    let range = material_sources.header.zw;
    let grouped = grouped_opacity();
    let aggregate=pointwise_aggregate();
    var dose=0.0;
    var background = vec4<f32>(0.0);
    if grouped { background = deposit_paint(dry_stroke_start(state_coordinate)); }
    let grip=contact_feature(64u,style.contact_a.z>0.5) && !contact_uniform() && style.operation.w==0u;
    if grip {
        if style.canvas_opacity.z<=0.0 {
            return MaterialOutput(result,vec4<f32>(stroke_coverage,0.0,0.0,1.0),vec4<f32>(0.0));
        }
        dose=-log(max(pointwise_transmittance(result,background),0.000001));
    }
    let initial_dose=dose;
    if aggregate && MATERIAL_OPERATION==OP_DEPOSIT && contact_feature(4096u,style.material_a.x>0.0)
        && !grouped && deposit_unchanged(result,background,dabs[range.x].color.rgb) {
        return MaterialOutput(result,vec4<f32>(0.0),vec4<f32>(0.0));
    }
    if !grouped && contact_uniform() && MATERIAL_NORMAL_BLEND && !bristles_enabled() && stroke_coverage >= 1.0 {
        return MaterialOutput(result, vec4<f32>(stroke_coverage, 0.0, 0.0, 1.0), vec4<f32>(0.0));
    }
    if MATERIAL_OPERATION == OP_DEPOSIT && grouped && style.material_a.w>0.5
        && deposit_unchanged(result,background,dabs[range.x].color.rgb) {
        return MaterialOutput(result,vec4<f32>(0.0),vec4<f32>(0.0));
    }
    let telescoped = !grouped && MATERIAL_OPERATION == OP_CLONE && contact_uniform() && MATERIAL_NORMAL_BLEND;
    var clone_source = vec4<f32>(0.0);
    if MATERIAL_OPERATION == OP_CLONE && !telescoped {
        let source = clone_source_at(fragment_position.xy);
        clone_source = vec4<f32>(deposit_color(working_unassociate(source)), source.a);
    }
    var deposited = false;
    let first_coverage = stroke_coverage;
    let tooth = contact_paper(world);
    let field = contact_field_with_paper(world, tooth);
    let selected = brush_selection_at(world);
    for (var offset = 0u; offset < range.y; offset += 1u) {
        let dab = dabs[range.x + offset];
        let contact=material_contact(dab,world,field,stroke_coverage,selected,dose);
        if (contact.flags & MATERIAL_HELD) != 0u {
            stroke_coverage=max(stroke_coverage,contact.coverage);
            continue;
        }
        let pigment=contact.pigment;
        let coverage=contact.coverage;
        let repaint=(contact.flags & MATERIAL_REPAINT) != 0u;
        if aggregate {
            if grip {dose=relief_dose(dab,vec2<f32>(coverage,contact.reference),dose);}
            else {dose+=coverage*dab.flow*dab.color.a;}
            continue;
        }
        if coverage <= 0.0 { continue; }
        var requested_alpha = brush_alpha(dab,coverage/max(selected,0.000001))*selected;
        if grip {
            let next=relief_dose(dab,vec2<f32>(coverage,contact.reference)/max(selected,0.000001),dose);
            requested_alpha=(1.0-exp(dose-next))*selected;
            if style.color.a<=0.5 {
                if selected==1.0 && MATERIAL_OPERATION!=OP_CLONE {dose=next;}
                else {
                    let alpha=requested_alpha*select(1.0,clone_source.a,MATERIAL_OPERATION==OP_CLONE);
                    dose-=log(max(1.0-alpha,0.000001));
                }
            }
        }
        if contact_uniform() && MATERIAL_OPERATION != OP_CLONE && !bristles_enabled() && requested_alpha < 1.0 {
            let laid = brush_alpha(dab,contact.reference/max(selected,0.000001))*selected;
            requested_alpha = clamp(working_ratio(requested_alpha-laid,1.0-laid),0.0,1.0);
        }
        if telescoped {
            stroke_coverage = max(stroke_coverage,requested_alpha);
            continue;
        }
        var source_alpha = requested_alpha;
        if grouped {
            if MATERIAL_OPERATION == OP_CLONE && contact_uniform() {
                let next = max(stroke_coverage,requested_alpha);
                source_alpha = clamp(working_ratio(clone_source.a*(next-stroke_coverage),
                    1.0-clone_source.a*stroke_coverage),0.0,1.0);
                stroke_coverage = next;
            } else if bristles_enabled() && contact_uniform() {
                if repaint { stroke_coverage = 0.0; }
                let next = max(stroke_coverage,source_alpha);
                source_alpha = clamp(working_ratio(next-stroke_coverage,1.0-stroke_coverage),0.0,1.0);
                stroke_coverage = next;
            } else {
                if MATERIAL_OPERATION == OP_CLONE { source_alpha *= clone_source.a; }
                stroke_coverage += (1.0-stroke_coverage)*source_alpha;
            }
        } else if contact_uniform() {
            if repaint && style.canvas_opacity.z == 1.0 { stroke_coverage = 0.0; }
            let next_coverage = select(stroke_coverage + (1.0 - stroke_coverage) * requested_alpha,
                max(stroke_coverage, requested_alpha), MATERIAL_OPERATION == OP_CLONE
                    || (bristles_enabled() && style.canvas_opacity.z == 1.0));
            if MATERIAL_OPERATION == OP_CLONE {
                source_alpha = clamp(
                    stroke_increment(clone_source.a * stroke_coverage, clone_source.a * next_coverage),
                    0.0,
                    1.0,
                );
            } else {
                source_alpha = clamp(
                    stroke_increment(stroke_coverage, next_coverage),
                    0.0,
                    1.0,
                );
            }
            stroke_coverage = next_coverage;
        } else {
            if MATERIAL_OPERATION == OP_CLONE { source_alpha *= clone_source.a; }
            if MATERIAL_OPERATION == OP_COVERAGE || style.canvas_opacity.z < 1.0 {
                let next_coverage = stroke_coverage + (1.0 - stroke_coverage) * source_alpha;
                source_alpha = stroke_increment(stroke_coverage, next_coverage);
                stroke_coverage = next_coverage;
            }
        }
        let erasing = !grouped && style.operation.w != 0u;
        let erased = result * (1.0 - source_alpha);
        if source_alpha > 0.0 {
            if !deposited { deposited = true; result = deposit_paint(result); }
            let color = select(pigment, clone_source.rgb, MATERIAL_OPERATION == OP_CLONE);
            if grouped { result = grouped_deposit(result, background, color, source_alpha); }
            else { result = source_over(result, color, source_alpha); }
        }
        result = select(result, erased, erasing);
    }
    if aggregate {
        var remaining = exp(-6.0*dose);
        if grip {remaining=exp(initial_dose-dose);}
        if grouped && remaining<1.0 {
            result = stored_paint(grouped_deposit(deposit_paint(result),background,dabs[range.x].color.rgb,1.0-remaining));
        } else if style.operation.w != 0u { result *= remaining; }
        else if remaining<1.0 {
            let alpha=1.0-remaining;
            let pigment=style.material_b.xyz;
            if result.a==0.0 || all(result.rgb==pigment*result.a) {
                let coverage=alpha+result.a*(1.0-alpha);
                result=vec4<f32>(pigment*coverage,coverage);
            } else {
                let destination=deposit_paint(result);
                result=stored_paint(vec4<f32>(destination.rgb*(1.0-alpha)+dabs[range.x].color.rgb*alpha,
                    alpha+destination.a*(1.0-alpha)));
            }
        }
        var coverage = 0.0;
        if MATERIAL_OPERATION == OP_COVERAGE {
            let prior = select(dry_coverage(vec2<i32>(floor(fragment_position.xy))),0.0,style.canvas_opacity.w>0.5);
            coverage = 1.0-(1.0-prior)*remaining;
        }
        return MaterialOutput(result,vec4<f32>(coverage,0.0,0.0,1.0),vec4<f32>(0.0));
    }
    if deposited {
        result = stored_paint(result);
    }
    if telescoped && stroke_coverage > first_coverage {
        let source = clone_source_at(fragment_position.xy);
        let alpha = clamp(
            stroke_increment(source.a * first_coverage, source.a * stroke_coverage),
            0.0,
            1.0,
        );
        if style.operation.w != 0u { result *= 1.0 - alpha; }
        else { result = stored_paint(source_over(deposit_paint(result), deposit_color(working_unassociate(source)), alpha)); }
    }
    return MaterialOutput(
        result,
        vec4<f32>(stroke_coverage, 0.0, 0.0, 1.0),
        vec4<f32>(0.0),
    );
}

fn pointwise_transmittance(result:vec4<f32>,background:vec4<f32>)->f32 {
    if grouped_opacity() {
        return 1.0-(result.a-(1.0-style.canvas_opacity.z)*background.a)/style.canvas_opacity.z;
    }
    return 1.0-result.a;
}

fn pointwise_material_result(fragment_position:vec4<f32>)->MaterialOutput {
        let p = vec2<u32>(fragment_position.xy);
        let bounds = material_sources.pages[0];
        if any(p < bounds.xy) || any(p >= bounds.zw) {
            return MaterialOutput(
                dry_original(vec2<i32>(p)),
                vec4<f32>(dry_coverage(vec2<i32>(p)), 0.0, 0.0, 1.0),
                vec4<f32>(0.0),
            );
        }
    var result=pointwise_paint_fragment(fragment_position);
    if style.color.a>0.5 && !pointwise_aggregate() {
        result=material_alpha_locked(result,dry_original(vec2<i32>(floor(fragment_position.xy))));
    }
    result.color=layer_color(result.color,WORKING_LUMA,style.color_mode.x,style.color_mode.y);
    return result;
}
