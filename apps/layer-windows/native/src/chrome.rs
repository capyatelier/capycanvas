use layer_host::scene::{Glass, GlassLayout, NavigatorSlot, Navigators};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChromePacket {
    revision: u64,
    document: [u32;2],
    glass: GlassLayout,
    overviews: Vec<NavigatorSlot>,
}

#[derive(Default)]
pub struct Chrome {
    pub glass: Glass,
    pub navigators: Navigators,
    pub presented_revision: Option<u64>,
    pub presented_document: Option<[u32;2]>,
    pub document: [u32;2],
    revision: u64,
}
impl Chrome {
    pub fn set(&mut self, packet: ChromePacket) -> Result<bool, String> {
        if packet.revision <= self.revision { return Ok(false); }
        let mut navigators = Navigators::default();
        navigators.set(packet.overviews)?;
        self.glass.set(packet.glass)?;
        self.navigators = navigators;
        self.document = packet.document;
        self.revision = packet.revision;
        Ok(true)
    }
    pub fn revision(&self)->u64 {self.revision}
    pub fn completed(&mut self, revision: u64, document: [u32;2]) {
        if revision>0&&revision<=self.revision&&self.presented_revision.is_none_or(|shown|revision>shown){self.presented_revision=Some(revision);self.presented_document=Some(document);}
    }
    pub fn reset_presented(&mut self){self.presented_revision=None;self.presented_document=None;}
    pub fn can_present(&self, staged: bool) -> bool {
        staged || self.presented_revision.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(revision: u64) -> serde_json::Value {
        serde_json::json!({"revision":revision,"document":[192,192],"glass":{"regions":[[1,2,30,40,3,3,3,3]],"connections":[]},
            "overviews":[{"bounds":[1,2,30,40],"clip":[1,2,30,40],"order":1}]})
    }
    #[test]
    fn chrome_packet_rejects_partial_updates_and_old_revisions() {
        let mut chrome = Chrome::default();
        assert!(chrome.set(serde_json::from_value(packet(1)).unwrap()).unwrap());
        for field in ["glass", "overviews"] {
            let mut invalid = packet(2);
            if field == "glass" { invalid[field]["regions"][0][2] = (-1).into(); }
            else { invalid[field][0]["opacity"] = 2.into(); }
            assert!(chrome.set(serde_json::from_value(invalid).unwrap()).is_err());
            assert_eq!(chrome.revision(), 1);assert_eq!(chrome.document,[192;2]);
            assert_eq!(chrome.glass.count(), 1);
            assert_eq!(chrome.navigators.count(), 1);
        }
        assert!(!chrome.set(serde_json::from_value(packet(1)).unwrap()).unwrap());
        assert!(!chrome.set(serde_json::from_value(packet(0)).unwrap()).unwrap());
        chrome.completed(0,[192;2]);assert_eq!(chrome.presented_revision, None);
        assert!(chrome.set(serde_json::from_value(serde_json::json!({"revision":2,"document":[192,192],"glass":{"regions":[],"connections":[]},"overviews":[]})).unwrap()).unwrap());
        chrome.completed(0,[192;2]);assert_eq!(chrome.presented_revision, None);
        chrome.completed(2,[192;2]);assert_eq!(chrome.presented_revision, Some(2));
        assert_eq!(chrome.glass.count(), 0);
        assert_eq!(chrome.navigators.count(), 0);
    }
    #[test]
    fn unstaged_chrome_keeps_the_last_acknowledged_revision() {
        let mut chrome = Chrome::default();
        chrome.set(serde_json::from_value(packet(1)).unwrap()).unwrap();
        assert!(chrome.can_present(false));
        chrome.completed(0,[192;2]);assert_eq!(chrome.presented_revision, None);
        chrome.completed(1,[192;2]);assert_eq!(chrome.presented_revision, Some(1));
        assert_eq!(chrome.presented_document,Some([192;2]));
        assert!(!chrome.can_present(false));
        assert!(chrome.can_present(true));
        let mut next=packet(2);next["document"]=serde_json::json!([128,64]);
        chrome.set(serde_json::from_value(next).unwrap()).unwrap();
        assert!(chrome.can_present(true));
        chrome.completed(0,[192;2]);assert_eq!(chrome.presented_revision, Some(1));
        assert_eq!(chrome.presented_document,Some([192;2]));
        chrome.completed(2,[128,64]);assert_eq!(chrome.presented_revision, Some(2));
        chrome.completed(1,[192;2]);assert_eq!(chrome.presented_document,Some([128,64]));
        chrome.reset_presented();
        assert_eq!(chrome.presented_document,None);
        assert!(chrome.can_present(false));
        chrome.completed(0,[192;2]);assert_eq!(chrome.presented_revision, None);
        chrome.completed(2,[128,64]);assert_eq!(chrome.presented_revision, Some(2));
    }
}
