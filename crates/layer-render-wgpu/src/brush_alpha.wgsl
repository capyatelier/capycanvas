fn brush_alpha(dab: Dab, coverage: f32) -> f32 {
    let dose = coverage*dab.flow*dab.color.a;
    if contact_feature(1u,style.contact_a.x>0.5) && !contact_uniform() {
        return 1.0-exp(-6.0*dose);
    }
    return clamp(dose,0.0,1.0);
}

fn relief_dose(dab:Dab,exposure:vec2<f32>,dose:f32)->f32 {
    let feed=6.0*dab.flow*dab.color.a;
    let full=feed*exposure.y;
    let delta=feed*exposure.x;
    let room=max(0.0,full-dose);
    let phi=mix(0.12,1.0,dab.contact.x*dab.contact.x*dab.contact.x);
    return dose+mix(min(delta,room),delta,phi);
}
