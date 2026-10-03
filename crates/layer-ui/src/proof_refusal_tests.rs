use super::*;
use crate::{ColorFeatureError, DocumentHostErrorCopy, Localizer, Platform, UiLanguage, session::test_support::{Recorder, session, event}};
use layer_core::color::{RenderingIntent, SampleDepth, hdr::SdrRendition};
use layer_engine::PenPhase;
use serde_json::{Value, json};

#[track_caller]
fn refusal<T, E: Serialize>(session: &mut UiSession<Recorder>, result: Result<T, E>, expected: Value) {
    let encoded = serde_json::to_value(result.err().expect("proof operation must refuse")).unwrap();
    assert_eq!(encoded, expected);
    let reason: ColorFeatureError = serde_json::from_value(encoded.clone()).unwrap();
    let document = session.engine().document().clone();
    let checkpoint = session.engine().checkpoint();
    session.set_host_error_copy(Some(DocumentHostErrorCopy::Color(reason.clone())));
    let english = reason.message(&Localizer::shared(UiLanguage::English));
    for language in UiLanguage::ALL {
        let localizer = Localizer::shared(language);
        session.set_localization(localizer.clone());
        assert_eq!(session.state().host_error, Some(reason.message(&localizer)));
        assert_eq!(session.engine().document(), &document);
        assert_eq!(session.engine().checkpoint(), checkpoint);
        assert_eq!(serde_json::to_value(&reason).unwrap(), encoded);
        if language == UiLanguage::Japanese { assert_ne!(session.state().host_error.as_deref(), Some(english.as_str())); }
    }
}
fn recipe() -> ProofRecipe { ProofRecipe::new("sRGB".into(), ColorProfile::Builtin(RgbSpace::Srgb)) }

#[test]
fn proof_refusals_retain_real_producer_meanings_across_languages() {
    let mut s = session(Platform::Web);
    let result = ProofPreparation::begin(&s, None, None);
    refusal(&mut s, result, json!("ProofChooseProfile"));
    for (invalid, expected) in [
        (ProofRecipe {name:"x".repeat(1025), ..recipe()}, "name_limit"),
        (ProofRecipe {simulate_paper:true, simulate_black_ink:false, ..recipe()}, "paper_requires_black_ink"),
        (ProofRecipe {conversion:layer_core::color::ConversionOptions {intent:RenderingIntent::AbsoluteColorimetric, black_point_compensation:true}, ..recipe()}, "absolute_black_point"),
    ] {
        let result = ProofPreparation::begin(&s, None, Some(invalid));
        refusal(&mut s, result, json!({"ProofRecipe":expected}));
    }
    let result = crate::proof_panel::PrintProofSettings::default().recipe();
    refusal(&mut s, result, json!("ProofChoosePrintProfile"));
    let result = s.set_proof_mode(crate::ProofMode::Sdr);
    refusal(&mut s, result, json!("ProofAlreadySdr"));
    let result = s.set_proof_mode(crate::ProofMode::Print);
    refusal(&mut s, result, json!("ProofChoosePrintProfile"));
    let result = s.set_sdr_rendition(SdrRendition::default());
    refusal(&mut s, result, json!("ProofHdrArtwork"));
    let result = crate::proof_panel::apply(&mut s, crate::proof_panel::ProofAction::Number {key:"exposure".into(), value:f64::INFINITY, phase:None});
    refusal(&mut s, result, json!("ProofInvalidValue"));
    s.select_proof_mode(crate::ProofMode::Print).unwrap();
    let job = ProofPreparation::panel(&s, recipe()).unwrap();
    s.select_proof_mode(crate::ProofMode::Off).unwrap();
    let result = job.validate(&s);
    refusal(&mut s, result, json!("ProofInactive"));
    s.select_proof_mode(crate::ProofMode::Print).unwrap();
    let job = ProofPreparation::panel(&s, recipe()).unwrap();
    s.dispatch(UiAction::Invoke {command:crate::CommandId::SaveDocumentAs}).unwrap();
    let result = job.validate(&s);
    refusal(&mut s, result, json!("ProofDocumentBusy"));
    let id = s.state().requests.last().unwrap().id;
    s.complete_document_request(id, Ok(false)).unwrap();
    s.dispatch(UiAction::Invoke {command:crate::CommandId::SoftProofSetup}).unwrap();
    let id = s.state().requests.last().unwrap().id;
    let job = ProofPreparation::begin(&s, Some(id), Some(recipe())).unwrap();
    s.dispatch(UiAction::CompleteRequest {id, error:None}).unwrap();
    let result = job.validate(&s);
    refusal(&mut s, result, json!("ProofSetupInactive"));
    s.set_proof_recipe(Some(ProofRecipe::new("literal old profile".into(), ColorProfile::Icc(vec![1].into())))).unwrap();
    s.select_proof_mode(crate::ProofMode::Print).unwrap();
    let job = ProofPreparation::panel(&s, recipe()).unwrap();
    let result = job.apply(&mut s, false);
    refusal(&mut s, result, json!("ProofPreserveOriginal"));
    let result = job.build(|| true);
    refusal(&mut s, result, json!("ProofCancelled"));
    let calls=std::cell::Cell::new(0);
    let result=job.build(|| {let value=calls.get();calls.set(value+1);value>0});
    refusal(&mut s,result,json!("ProofCancelled"));
    assert!(calls.get()>1);
    s.set_proof_recipe(Some(recipe())).unwrap();
    let result = job.validate(&s);
    refusal(&mut s, result, json!("ProofDrawingChanged"));
    s.pen(event(&s, 1, PenPhase::Down, 0.5)).unwrap();
    let result = s.set_proof_mode(crate::ProofMode::Off);
    refusal(&mut s, result, json!({"ProofAdmission":"canvas_interaction"}));
    s.pen(event(&s, 2, PenPhase::Cancel, 0.)).unwrap();

    let mut document = layer_core::Document::new("HDR", 32, 32, layer_core::DocumentNames {paint:"Current ink".into(), paper:"Paper".into()});
    document.color.depth = SampleDepth::F32;
    let mut hdr = UiSession::new(Recorder {color:document.color, ..Default::default()}, document, [32,32], Platform::Web).unwrap();
    let result = hdr.set_sdr_rendition(SdrRendition {exposure:13., ..Default::default()});
    refusal(&mut hdr, result, json!("ProofInvalidRendition"));
    let checkpoint = hdr.engine().checkpoint();
    let result = hdr.edit_sdr_rendition(crate::ContactPhase::Down, SdrRendition {contrast:0., ..Default::default()});
    refusal(&mut hdr, result, json!("ProofInvalidRendition"));
    assert_eq!(hdr.engine().checkpoint(), checkpoint);
}
