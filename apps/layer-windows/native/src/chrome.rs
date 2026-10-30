use layer_host::scene::{Glass, GlassLayout, NavigatorSlot, Navigators};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChromePacket {
    revision: u64,
    glass: GlassLayout,
    overviews: Vec<NavigatorSlot>,
}

#[derive(Default)]
pub struct Chrome {
    pub glass: Glass,
    pub navigators: Navigators,
    pub presented_revision: Option<u64>,
    revision: u64,
}
impl Chrome {
    pub fn set(&mut self, packet: ChromePacket) -> Result<bool, String> {
        if packet.revision <= self.revision { return Ok(false); }
        let mut navigators = Navigators::default();
        navigators.set(packet.overviews)?;
        self.glass.set(packet.glass)?;
        self.navigators = navigators;
        self.revision = packet.revision;
        Ok(true)
    }
    pub fn presented(&mut self, complete: bool) {
        if complete { self.presented_revision = Some(self.revision); }
    }
    pub fn can_present(&self, staged: bool) -> bool {
        staged || self.presented_revision.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(revision: u64) -> serde_json::Value {
        serde_json::json!({"revision":revision,"glass":{"regions":[[1,2,30,40,3,3,3,3]],"connections":[]},
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
            assert_eq!(chrome.revision, 1);
            assert_eq!(chrome.glass.count(), 1);
            assert_eq!(chrome.navigators.count(), 1);
        }
        assert!(!chrome.set(serde_json::from_value(packet(1)).unwrap()).unwrap());
        assert!(!chrome.set(serde_json::from_value(packet(0)).unwrap()).unwrap());
        chrome.presented(false);assert_eq!(chrome.presented_revision, None);
        assert!(chrome.set(serde_json::from_value(serde_json::json!({"revision":2,"glass":{"regions":[],"connections":[]},"overviews":[]})).unwrap()).unwrap());
        chrome.presented(false);assert_eq!(chrome.presented_revision, None);
        chrome.presented(true);assert_eq!(chrome.presented_revision, Some(2));
        assert_eq!(chrome.glass.count(), 0);
        assert_eq!(chrome.navigators.count(), 0);
    }
    #[test]
    fn unstaged_chrome_keeps_the_last_acknowledged_revision() {
        let mut chrome = Chrome::default();
        chrome.set(serde_json::from_value(packet(1)).unwrap()).unwrap();
        assert!(chrome.can_present(false));
        chrome.presented(false);assert_eq!(chrome.presented_revision, None);
        chrome.presented(true);assert_eq!(chrome.presented_revision, Some(1));
        assert!(!chrome.can_present(false));
        assert!(chrome.can_present(true));
        chrome.set(serde_json::from_value(packet(2)).unwrap()).unwrap();
        assert!(chrome.can_present(true));
        chrome.presented(false);assert_eq!(chrome.presented_revision, Some(1));
        chrome.presented(true);assert_eq!(chrome.presented_revision, Some(2));
        chrome.presented_revision = None;
        assert!(chrome.can_present(false));
        chrome.presented(false);assert_eq!(chrome.presented_revision, None);
        chrome.presented(true);assert_eq!(chrome.presented_revision, Some(2));
    }
}
