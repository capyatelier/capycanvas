//! Durable recovery ordering, independent of files, IndexedDB, locks or timers.
//! A host executes one returned work ticket and reports completion. Failed work
//! never acknowledges a checkpoint and never retires the only durable origin.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryDocument {
    pub epoch: u64,
    pub revision: u64,
    pub modified: bool,
    pub busy: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecoveryWorkKind {
    Capture,
    Retire,
    Restore { key: String },
    RetireOrigin { key: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryWork {
    pub token: u64,
    pub kind: RecoveryWorkKind,
    document: Option<RecoveryDocument>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
enum Origin {
    Offered(String),
    Restoring(String),
    Restored(String),
    Durable(String),
}
#[derive(Default, Serialize, Deserialize)]
pub struct RecoveryState {
    checkpoint: Option<RecoveryDocument>,
    document: Option<RecoveryDocument>,
    owned: bool,
    closed: bool,
    next_token: u64,
    pending: Option<RecoveryWork>,
    retire: bool,
    discard_origin: bool,
    origin: Option<Origin>,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecoveryEvent {
    Observe {
        document: RecoveryDocument,
        owned: bool,
    },
    Ownership {
        owned: bool,
    },
    Offer {
        key: String,
        owned: bool,
    },
    Restore,
    /// Used by a host which opens the recovered artwork in a new window.
    Adopted {
        key: String,
    },
    Dismiss {
        discard: bool,
    },
    Retire {
        #[serde(default)]
        discard_origin: bool,
    },
    Complete {
        token: u64,
        success: bool,
    },
    Close,
    /// A native window can veto an already prepared close. Resume only after
    /// its accepted storage work finishes, then checkpoint the live drawing.
    Resume,
}
#[derive(Default, Serialize)]
pub struct RecoveryUpdate {
    pub work: Option<RecoveryWork>,
    pub offer: Option<String>,
    pub busy: bool,
    /// The latest observation (or accepted close) has completed all storage
    /// work, including retirement of an adopted recovery origin.
    pub current: bool,
    pub release: Vec<String>,
}
impl RecoveryState {
    fn schedule(&mut self, kind: RecoveryWorkKind) -> RecoveryWork {
        self.next_token += 1;
        let work = RecoveryWork {
            token: self.next_token,
            kind,
            document: self.document,
        };
        self.pending = Some(work.clone());
        work
    }
    fn next(&mut self) -> Option<RecoveryWork> {
        if self.pending.is_some() || !self.owned {
            return None;
        }
        if self.retire {
            return Some(self.schedule(RecoveryWorkKind::Retire));
        }
        if let Some(Origin::Durable(key)) = &self.origin {
            return Some(self.schedule(RecoveryWorkKind::RetireOrigin { key: key.clone() }));
        }
        if self.closed || matches!(self.origin, Some(Origin::Offered(_) | Origin::Restoring(_))) {
            return None;
        }
        let document = self.document?;
        if document.busy {
            return None;
        }
        if self.checkpoint == Some(document) && !matches!(self.origin, Some(Origin::Restored(_))) {
            return None;
        }
        Some(self.schedule(
            if document.modified || matches!(self.origin, Some(Origin::Restored(_))) {
                RecoveryWorkKind::Capture
            } else {
                RecoveryWorkKind::Retire
            },
        ))
    }
    pub fn event(&mut self, event: RecoveryEvent) -> Result<RecoveryUpdate, String> {
        let mut update = RecoveryUpdate::default();
        let mut advance = true;
        match event {
            RecoveryEvent::Ownership { owned } => {
                self.owned = owned;
            }
            RecoveryEvent::Observe { document, owned } => {
                self.document = Some(document);
                self.owned = owned;
            }
            RecoveryEvent::Offer { key, owned } => {
                if owned && !self.closed && self.origin.is_none() && self.pending.is_none() {
                    self.origin = Some(Origin::Offered(key));
                }
            }
            RecoveryEvent::Restore => {
                if self.pending.is_some() || self.closed {
                    return Err("Recovery work is still pending".into());
                }
                match self.origin.clone() {
                    Some(Origin::Offered(key)) => {
                        self.origin = Some(Origin::Restoring(key.clone()));
                        update.work = Some(self.schedule(RecoveryWorkKind::Restore { key }));
                    }
                    Some(Origin::Restored(_)) => update.work = self.next(),
                    _ => return Err("No recovery copy is available".into()),
                }
            }
            RecoveryEvent::Adopted { key } => {
                if self.pending.is_some() {
                    return Err("Recovery work is still pending".into());
                }
                self.origin = Some(Origin::Restored(key));
                self.checkpoint = None;
            }
            RecoveryEvent::Dismiss { discard } => {
                if self.pending.is_some() {
                    return Err("Recovery work is still pending".into());
                }
                if let Some(Origin::Offered(key) | Origin::Restored(key)) = self.origin.take() {
                    if discard {
                        self.origin = Some(Origin::Durable(key));
                    } else {
                        update.release.push(key);
                    }
                }
            }
            RecoveryEvent::Retire { discard_origin } => {
                self.retire = true;
                self.discard_origin |= discard_origin;
                self.checkpoint = None;
            }
            RecoveryEvent::Complete { token, success } => {
                if self.pending.as_ref().is_none_or(|p| p.token != token) {
                    return Err("Stale recovery completion".into());
                }
                let work = self.pending.take().unwrap();
                advance = success
                    || (self.retire
                        && matches!(
                            work.kind,
                            RecoveryWorkKind::Capture | RecoveryWorkKind::Restore { .. }
                        ));
                match work.kind {
                    RecoveryWorkKind::Capture if success => {
                        if !self.retire {
                            self.checkpoint = work.document.filter(|document| document.modified);
                        }
                        if let Some(Origin::Restored(key)) = self.origin.take() {
                            self.origin = Some(Origin::Durable(key));
                        }
                    }
                    RecoveryWorkKind::Retire if success => {
                        self.retire = false;
                        self.checkpoint = work.document;
                        if self.discard_origin {
                            if let Some(origin) = self.origin.take() {
                                let key = match origin {
                                    Origin::Offered(k)
                                    | Origin::Restoring(k)
                                    | Origin::Restored(k)
                                    | Origin::Durable(k) => k,
                                };
                                self.origin = Some(Origin::Durable(key));
                            }
                            self.discard_origin = false;
                        }
                    }
                    RecoveryWorkKind::Restore { key } => {
                        self.origin = Some(if success {
                            self.checkpoint = None;
                            Origin::Restored(key)
                        } else {
                            Origin::Offered(key)
                        });
                    }
                    RecoveryWorkKind::RetireOrigin { key } if success => {
                        self.origin = None;
                        update.release.push(key);
                    }
                    _ => (),
                }
            }
            RecoveryEvent::Close => {
                self.closed = true;
            }
            RecoveryEvent::Resume => {
                if self.pending.is_some() {
                    return Err("Recovery work is still pending".into());
                }
                self.closed = false;
                self.retire = false;
                self.discard_origin = false;
                self.checkpoint = None;
            }
        }
        if update.work.is_none() && advance {
            update.work = self.next();
        }
        update.offer = match &self.origin {
            Some(Origin::Offered(k) | Origin::Restoring(k) | Origin::Restored(k)) => {
                Some(k.clone())
            }
            _ => None,
        };
        update.busy = self.pending.is_some() || matches!(self.origin, Some(Origin::Restoring(_)));
        update.current = !update.busy
            && !self.retire
            && self.origin.is_none()
            && (self.closed || self.checkpoint == self.document);
        Ok(update)
    }
}
impl<R: layer_render::CanvasRenderer> crate::UiSession<R> {
    pub fn recovery_document(&self) -> RecoveryDocument {
        RecoveryDocument {
            epoch: self.state().document_file.epoch,
            revision: self.engine().document().revision,
            modified: self.state().document_file.modified,
            busy: self.state().document_file.busy || self.require_raster_snapshot().is_err(),
        }
    }
}
/// Opaque state transport used by JNI and Wasm. All decisions execute above.
pub fn recovery_update(state: &str, event: RecoveryEvent) -> Result<serde_json::Value, String> {
    let mut state: RecoveryState = if state.is_empty() {
        Default::default()
    } else {
        serde_json::from_str(state).map_err(|e| e.to_string())?
    };
    let update = state.event(event)?;
    Ok(
        serde_json::json!({"state":serde_json::to_string(&state).map_err(|e| e.to_string())?,"update":update}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observe(state: &mut RecoveryState, revision: u64, modified: bool) -> Option<RecoveryWork> {
        state
            .event(RecoveryEvent::Observe {
                document: RecoveryDocument {
                    epoch: 1,
                    revision,
                    modified,
                    busy: false,
                },
                owned: true,
            })
            .unwrap()
            .work
    }
    fn finish(state: &mut RecoveryState, work: &RecoveryWork, success: bool) -> RecoveryUpdate {
        state
            .event(RecoveryEvent::Complete {
                token: work.token,
                success,
            })
            .unwrap()
    }
    #[test]
    fn capture_retries_and_discard_waits_for_accepted_write() {
        let mut state = RecoveryState::default();
        let work = observe(&mut state, 1, true).unwrap();
        assert!(observe(&mut state, 2, true).is_none());
        assert!(finish(&mut state, &work, false).work.is_none());
        let work = observe(&mut state, 2, true).unwrap();
        assert!(
            state
                .event(RecoveryEvent::Retire {
                    discard_origin: false
                })
                .unwrap()
                .work
                .is_none()
        );
        let removal = finish(&mut state, &work, true).work.unwrap();
        assert_eq!(removal.kind, RecoveryWorkKind::Retire);
        state.event(RecoveryEvent::Close).unwrap();
        assert!(finish(&mut state, &removal, true).work.is_none());
        assert!(
            state
                .event(RecoveryEvent::Complete {
                    token: work.token,
                    success: true
                })
                .is_err()
        );
    }
    #[test]
    fn closing_after_failed_capture_still_retires_the_previous_copy() {
        let mut state = RecoveryState::default();
        let capture = observe(&mut state, 1, true).unwrap();
        state
            .event(RecoveryEvent::Retire {
                discard_origin: true,
            })
            .unwrap();
        state.event(RecoveryEvent::Close).unwrap();
        let retire = finish(&mut state, &capture, false).work.unwrap();
        assert_eq!(retire.kind, RecoveryWorkKind::Retire);
        assert!(finish(&mut state, &retire, true).work.is_none());
    }
    #[test]
    fn current_status_waits_for_latest_idle_observation_and_successful_storage() {
        let mut state = RecoveryState::default();
        let first = observe(&mut state, 1, true).unwrap();
        let busy = RecoveryDocument {
            epoch: 1,
            revision: 2,
            modified: true,
            busy: true,
        };
        assert!(
            !state
                .event(RecoveryEvent::Observe {
                    document: busy,
                    owned: true
                })
                .unwrap()
                .current
        );
        let completed = finish(&mut state, &first, true);
        assert!(!completed.current && completed.work.is_none());
        let latest = observe(&mut state, 2, true).unwrap();
        assert!(!finish(&mut state, &latest, false).current);
        let retry = observe(&mut state, 2, true).unwrap();
        assert!(finish(&mut state, &retry, true).current);
    }
    #[test]
    fn cancelled_native_close_recaptures_the_live_drawing_after_retirement() {
        let mut state = RecoveryState::default();
        let capture = observe(&mut state, 1, true).unwrap();
        state
            .event(RecoveryEvent::Retire {
                discard_origin: true,
            })
            .unwrap();
        state.event(RecoveryEvent::Close).unwrap();
        assert!(
            state.event(RecoveryEvent::Resume).is_err(),
            "accepted storage finishes before a close veto"
        );
        let retire = finish(&mut state, &capture, true).work.unwrap();
        assert_eq!(retire.kind, RecoveryWorkKind::Retire);
        assert!(finish(&mut state, &retire, true).current);
        let resumed = state.event(RecoveryEvent::Resume).unwrap();
        assert!(!resumed.current);
        assert_eq!(
            resumed.work.as_ref().unwrap().kind,
            RecoveryWorkKind::Capture
        );
        assert!(finish(&mut state, &resumed.work.unwrap(), true).current);
    }
    #[test]
    fn adopted_origin_is_not_current_until_its_replacement_and_retirement_finish() {
        let mut state = RecoveryState::default();
        state
            .event(RecoveryEvent::Observe {
                document: RecoveryDocument {
                    epoch: 2,
                    revision: 1,
                    modified: true,
                    busy: false,
                },
                owned: false,
            })
            .unwrap();
        let adopted = state
            .event(RecoveryEvent::Adopted {
                key: "previous owner".into(),
            })
            .unwrap();
        assert!(!adopted.current && adopted.work.is_none());
        let capture = state
            .event(RecoveryEvent::Ownership { owned: true })
            .unwrap()
            .work
            .unwrap();
        let published = finish(&mut state, &capture, true);
        assert!(!published.current && published.release.is_empty());
        let retire = published.work.unwrap();
        assert!(matches!(retire.kind, RecoveryWorkKind::RetireOrigin { .. }));
        let completed = finish(&mut state, &retire, true);
        assert!(completed.current);
        assert_eq!(completed.release, ["previous owner"]);
    }
    #[test]
    fn a_clean_adopted_drawing_retires_its_replacement_after_preserving_the_origin() {
        let mut state = RecoveryState::default();
        state
            .event(RecoveryEvent::Observe {
                document: RecoveryDocument {
                    epoch: 2,
                    revision: 1,
                    modified: false,
                    busy: false,
                },
                owned: false,
            })
            .unwrap();
        state
            .event(RecoveryEvent::Adopted {
                key: "saved origin".into(),
            })
            .unwrap();
        let capture = state
            .event(RecoveryEvent::Ownership { owned: true })
            .unwrap()
            .work
            .unwrap();
        assert_eq!(capture.kind, RecoveryWorkKind::Capture);
        let origin = finish(&mut state, &capture, true).work.unwrap();
        assert!(matches!(origin.kind, RecoveryWorkKind::RetireOrigin { .. }));
        let removed = finish(&mut state, &origin, true);
        assert_eq!(removed.release, ["saved origin"]);
        let own_copy = removed
            .work
            .expect("clean drawing must not leave an unnecessary recovery copy");
        assert_eq!(own_copy.kind, RecoveryWorkKind::Retire);
        assert!(finish(&mut state, &own_copy, true).current);
    }
    #[test]
    fn recovery_origin_survives_failed_replacement_until_a_durable_copy_exists() {
        let mut state = RecoveryState::default();
        state.owned = true;
        state
            .event(RecoveryEvent::Offer {
                key: "origin".into(),
                owned: false,
            })
            .unwrap();
        assert!(state.event(RecoveryEvent::Restore).is_err());
        state
            .event(RecoveryEvent::Offer {
                key: "origin".into(),
                owned: true,
            })
            .unwrap();
        let restore = state.event(RecoveryEvent::Restore).unwrap().work.unwrap();
        assert!(observe(&mut state, 3, true).is_none());
        let capture = finish(&mut state, &restore, true).work.unwrap();
        assert_eq!(capture.kind, RecoveryWorkKind::Capture);
        let failed = finish(&mut state, &capture, false);
        assert!(failed.release.is_empty());
        assert_eq!(failed.offer.as_deref(), Some("origin"));
        let retry = observe(&mut state, 3, true).unwrap();
        let remove = finish(&mut state, &retry, true).work.unwrap();
        assert_eq!(
            remove.kind,
            RecoveryWorkKind::RetireOrigin {
                key: "origin".into()
            }
        );
        assert!(finish(&mut state, &remove, false).release.is_empty());
        let retry = observe(&mut state, 3, true).unwrap();
        assert_eq!(finish(&mut state, &retry, true).release, ["origin"]);
        assert!(observe(&mut state, 3, true).is_none());
        let clean = observe(&mut state, 4, false).unwrap();
        finish(&mut state, &clean, true);
        assert!(observe(&mut state, 4, false).is_none());
    }
}
