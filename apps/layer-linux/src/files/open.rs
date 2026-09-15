//! Signature-based project/photo loading on a file worker. A source photo never
//! grants authority to overwrite that path with a native master.
use layer_core::{Document, Project, ProjectLimits, color::RgbSpace};
use layer_ui::DocumentLocation;
use std::{
    io::{BufRead, BufReader},
    path::Path,
    sync::Arc,
};

pub(crate) fn read(
    path: &Path,
    location: DocumentLocation,
) -> Result<(Project, Option<DocumentLocation>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let mut reader = BufReader::new(file);
    if reader
        .fill_buf()
        .map_err(|e| e.to_string())?
        .starts_with(b"CAPY")
    {
        return Project::read(reader, ProjectLimits::default()).map(|p| (p, Some(location)));
    }
    let source = layer_color::photo::read_photo(reader, Default::default())?;
    let space = layer_color::suggested_working_space(&source.interpretation.profile)?
        .unwrap_or(RgbSpace::ProPhoto);
    let mut document = Document::new("untitled", source.extent[0], source.extent[1]);
    document.color = layer_core::color::DocumentColor {
        space,
        depth: source.interpretation.depth,
    };
    document.layers[0].name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect::<String>()
        .into();
    if document.layers[0].name.is_empty() {
        document.layers[0].name = "Photo".into();
    }
    document.layers[0].source = Some(Arc::new(source));
    // Retained photo alpha remains visible. A separate Paper layer is available
    // to the user, but does not silently flatten an opened transparent image.
    document.layers[1].visible = false;
    let project = Project {
        document,
        assets: Default::default(),
    };
    project.validate(ProjectLimits::default())?;
    Ok((project, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{ColorProfile, IntegerDepth, source::*};
    #[test]
    fn photo_open_preserves_source_depth_profile_and_master_separation() {
        let directory =
            std::env::temp_dir().join(format!("capy-open-photo-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        for space in RgbSpace::ALL {
            for depth in [IntegerDepth::U8, IntegerDepth::U16] {
                let profile = ColorProfile::Icc(
                    layer_color::profile_bytes(&ColorProfile::Builtin(space))
                        .unwrap()
                        .into(),
                );
                let mut builder = SourceBuilder::new(
                    [3, 1],
                    SourceInterpretation {
                        channels: SourceChannels::Rgba,
                        depth,
                        profile,
                        profile_assumed: false,
                    },
                    1024 * 1024,
                )
                .unwrap();
                let codes = [
                    65535u16, 0, 32767, 1, 12345, 54321, 1001, 50000, 7654, 65535, 12456, 0,
                ];
                let row: Vec<u8> = if depth == IntegerDepth::U8 {
                    codes.map(|v| (v / 257) as u8).to_vec()
                } else {
                    codes.into_iter().flat_map(u16::to_le_bytes).collect()
                };
                builder.push_row(&row).unwrap();
                let source = builder.finish().unwrap();
                // Misleading extension cannot change file interpretation.
                let path = directory.join(format!("Photo-{space:?}-{depth:?}.capy"));
                layer_color::photo::write_png(std::fs::File::create(&path).unwrap(), &source)
                    .unwrap();
                let bytes = std::fs::read(&path).unwrap();
                let (project, location) = read(
                    &path,
                    DocumentLocation {
                        uri: "source".into(),
                        name: "Photo.png".into(),
                    },
                )
                .unwrap();
                assert!(location.is_none());
                assert_eq!(project.document.color.space, space);
                assert_eq!(project.document.color.depth, depth);
                assert!(!project.document.layers[1].visible);
                assert_eq!(project.document.layers[0].source.as_deref(), Some(&source));
                let mut master = Vec::new();
                project.write(&mut master).unwrap();
                assert_eq!(
                    Project::read(std::io::Cursor::new(master), Default::default()).unwrap(),
                    project
                );
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
            }
        }
        let path = directory.join("Ordinary.jpg");
        let mut builder = SourceBuilder::new(
            [3, 1],
            SourceInterpretation {
                channels: SourceChannels::Rgb,
                depth: IntegerDepth::U8,
                profile: ColorProfile::default(),
                profile_assumed: true,
            },
            1024,
        )
        .unwrap();
        builder
            .push_row(&[230, 75, 100, 245, 180, 100, 25, 190, 110])
            .unwrap();
        let mut jpeg = Vec::new();
        layer_color::photo::write_jpeg(&mut jpeg, &builder.finish().unwrap(), 95).unwrap();
        // Strip APP2 ICC segments to exercise an ordinary untagged JPEG.
        let mut untagged = jpeg[..2].to_vec();
        let mut at = 2;
        while jpeg[at + 1] != 0xda {
            let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]])) + 2;
            if jpeg[at + 1] != 0xe2 {
                untagged.extend_from_slice(&jpeg[at..at + len]);
            }
            at += len;
        }
        untagged.extend_from_slice(&jpeg[at..]);
        std::fs::write(&path, untagged).unwrap();
        let (project, location) = read(
            &path,
            DocumentLocation {
                uri: "source".into(),
                name: "Ordinary.jpg".into(),
            },
        )
        .unwrap();
        assert!(location.is_none());
        assert_eq!(project.document.color, Default::default());
        assert!(
            project.document.layers[0]
                .source
                .as_ref()
                .unwrap()
                .interpretation
                .profile_assumed
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
