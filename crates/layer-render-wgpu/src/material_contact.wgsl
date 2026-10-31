fn material_contact(dab:Dab, world:vec2<f32>, field:vec2<f32>, prior:f32, selected:f32)->MaterialContact {
    if bristles_enabled() {
        let paint=bristle_paint(dab,world,field.y);
        if paint.held {return MaterialContact(paint.color,paint.coverage*selected,0.0,MATERIAL_HELD);}
        let coverage=paint.opacity*select(paint.coverage*selected,max(paint.coverage*selected,prior),paint.reached);
        return MaterialContact(paint.color,coverage,0.0,select(0u,MATERIAL_REPAINT,paint.reached || (paint.rim && prior>=1.0)));
    }
    let footprint=footprint_and_start(dab,world,field)*selected;
    return MaterialContact(dab.color.rgb,footprint.x,footprint.y,0u);
}
