use crate::previews::CapyPreview;
use layer_ui::{HistogramView, UiState};
use serde_json::json;
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct Scopes {
    views: [Option<HistogramView>; 3],
    colors: Option<[[u8; 3]; 4]>,
    revision: u64,
}

fn views(state: &UiState) -> [&HistogramView; 3] {
    [&state.histogram, &state.waveform, &state.tonal_histogram]
}

impl Scopes {
    pub fn revision(&mut self, state: &UiState) -> u64 {
        let colors = state.palette.histogram_colors().map(|color| color.0);
        let mut changed = self.colors.replace(colors) != Some(colors);
        for (cached, view) in self.views.iter_mut().zip(views(state)) {
            if !cached.as_ref().is_some_and(|old| old.channel == view.channel && old.logarithmic == view.logarithmic
                && old.data.as_ref().map(Arc::as_ptr) == view.data.as_ref().map(Arc::as_ptr)) {
                *cached = Some(view.clone());
                changed = true;
            }
        }
        if changed { self.revision += 1; }
        self.revision
    }
}

pub(crate) fn query(state: &UiState, size: Option<[u32; 2]>) -> Result<CapyPreview, String> {
    let colors = state.palette.histogram_colors().map(|color| color.0);
    let (extent, bgra) = state.waveform.waveform_premultiplied_rgba(colors).map_or((None, Vec::new()), |([width, height], pixels)| {
        let [columns, rows] = size.map_or([width, height], |[columns, rows]| [columns.clamp(1, 4096), rows.clamp(1, 2048)]);
        let mut bgra = Vec::with_capacity(columns as usize * rows as usize * 4);
        for row in 0..rows {
            let source = (row * height / rows * width) as usize;
            for column in 0..columns {
                let rgba = &pixels[(source + (column * width / columns) as usize) * 4..][..4];
                bgra.extend_from_slice(&[rgba[2], rgba[1], rgba[0], rgba[3]]);
            }
        }
        (Some([columns, rows]), bgra)
    });
    CapyPreview::packet(json!({"result":{
        "colors": colors,
        "histogram": state.histogram.histogram_plot(),
        "tonal_histogram": state.tonal_histogram.histogram_plot(),
        "waveform": extent,
    },"error":null}), bgra)
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::histogram::{Histogram, Waveform};
    fn read(packet: CapyPreview) -> (serde_json::Value, Vec<u8>) {
        let owned = Box::into_raw(Box::new(packet));
        unsafe {
            let metadata = std::ffi::CStr::from_ptr(crate::previews::capy_preview_metadata(owned));
            let metadata = serde_json::from_slice(metadata.to_bytes()).unwrap();
            let mut length = 0;
            let bytes = std::slice::from_raw_parts(crate::previews::capy_preview_bytes(owned, &mut length), length).to_vec();
            crate::previews::capy_preview_free(owned);
            (metadata, bytes)
        }
    }
    #[test]
    fn scope_revision_follows_plot_inputs_and_waveform_is_premultiplied_bgra() {
        let host = layer_host::NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let mut state = host.session.state().clone();
        let mut scopes = Scopes::default();
        let empty = scopes.revision(&state);
        assert_eq!(scopes.revision(&state), empty);
        let (metadata, bytes) = read(query(&state, Some([300, 100])).unwrap());
        assert!(metadata["error"].is_null() && metadata["result"]["waveform"].is_null() && bytes.is_empty());
        assert_eq!(metadata["result"]["histogram"], json!([]));
        let mut data = Histogram::new(host.session.engine().document().composition().color);
        data.channels[0].bins[64] = 8;
        data.channels[2].bins[192] = 2;
        data.pixels = 10;
        let mut counts = vec![0; Waveform::WORDS];
        counts[64 * Waveform::SIDE + 5] = 8;
        data.waveform = Some(Waveform { counts });
        let data = Arc::new(data);
        state.histogram.data = Some(data.clone());
        state.waveform.data = Some(data.clone());
        let published = scopes.revision(&state);
        assert!(published > empty);
        assert_eq!(scopes.revision(&state), published);
        state.histogram.status = "Exact".into();
        assert_eq!(scopes.revision(&state), published);
        state.waveform.channel = 1;
        let channel = scopes.revision(&state);
        assert!(channel > published);
        state.histogram.data = Some(Arc::new((*data).clone()));
        assert!(scopes.revision(&state) > channel);
        let (metadata, bytes) = read(query(&state, None).unwrap());
        let plotted = metadata["result"]["histogram"].as_array().unwrap();
        assert_eq!(plotted.iter().map(|plot| plot[0].as_u64().unwrap()).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(plotted[0][1][64], 1.);
        assert_eq!(plotted[2][1][192], 0.25);
        let [width, height] = [0, 1].map(|i| metadata["result"]["waveform"][i].as_u64().unwrap() as usize);
        assert_eq!((width, bytes.len()), (256, width * height * 4));
        let colors = state.palette.histogram_colors().map(|color| color.0);
        let red = [colors[0][2], colors[0][1], colors[0][0], 255];
        assert_eq!(bytes[((height - 1 - 64) * width + 5) * 4..][..4], red);
        assert!(bytes.as_chunks::<4>().0.iter().all(|[b, g, r, a]| b <= a && g <= a && r <= a));
        let mut host = host;
        let reply = crate::workspace::query(&mut host, r#"{"type":"scopes","size":[512,512]}"#).unwrap();
        assert!(read(reply).0["result"]["waveform"].is_null());
        let (metadata, scaled) = read(query(&state, Some([512, height as u32 * 2])).unwrap());
        assert_eq!(metadata["result"]["waveform"], json!([512, height * 2]));
        let row = (height - 1 - 64) * 2;
        for (x, y) in [(10, row), (11, row + 1)] { assert_eq!(scaled[(y * 512 + x) * 4..][..4], red); }
        assert_eq!(scaled[(row * 512 + 12) * 4 + 3], 0);
    }
}
