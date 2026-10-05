fn effect_reduced(id:vec2<u32>)->vec4<f32> {
    let side=u32(settings.operation_linear.w);
    let start=vec2<u32>(settings.operation_offset.xy)+id.xy*side;
    let size=min(vec2(side),vec2<u32>(settings.operation_linear.xy)-start);
    if retained_cells.enabled!=0u {
        var changed=false;
        for(var y=0u;y<size.y;y+=retained_cells.side) {
            for(var x=0u;x<size.x;x+=retained_cells.side) {
                let cell=(start+vec2(x,y))/retained_cells.side;
                changed=changed || retained_cells.cells[cell.y*(256u/retained_cells.side)+cell.x]!=0u;
            }
        }
        if !changed {discard;}
    }
    let cache=EFFECT_POSITION_INDEPENDENT && settings.options.w<=.5 && settings.extent.z==0.;
    var previous_front=vec4(0.);
    var previous_back=vec4(0.);
    var value=vec4(0.);
    var valid=false;
    var sum=vec4(0.);
    for (var y=0u;y<size.y;y+=1u) {
        for (var x=0u;x<size.x;x+=1u) {
            let p=vec2<f32>(start+vec2(x,y))+vec2(.5);
            let front_value=effect_front(p);
            let back_value=textureLoad(back,vec2<i32>(p),0);
            if !cache || !valid || any(bitcast<vec4<u32>>(front_value)!=bitcast<vec4<u32>>(previous_front))
                || any(bitcast<vec4<u32>>(back_value)!=bitcast<vec4<u32>>(previous_back)) {
                value=effect_value(Vertex(vec4(p,0.,1.),p/vec2(256.)),front_value,back_value);
                previous_front=front_value;
                previous_back=back_value;
                valid=true;
            }
            sum+=value;
        }
    }
    return sum/f32(size.x*size.y);
}
