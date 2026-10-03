//! Delivered XMP is a filtered copy of the photo's packet. Rights, creator,
//! contact and camera properties stay; file paths, edit history, raw
//! settings, container layouts and regenerated image facts are left out, and
//! so is location when the recipe removes it.
use super::metadata::{ExportMetadata, MetadataKeep};
use quick_xml::{
    Writer,
    events::{BytesStart, Event, attributes::Attribute},
    name::ResolveResult,
    reader::NsReader,
};

const INVALID: &str = "Invalid XMP metadata";
const MAX_DEPTH: usize = 64;
pub(super) const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

const XML: &[u8] = b"http://www.w3.org/XML/1998/namespace";
const DC: &[u8] = b"http://purl.org/dc/elements/1.1/";
const XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/";
const XMP_RIGHTS: &[u8] = b"http://ns.adobe.com/xap/1.0/rights/";
const XMP_MM: &[u8] = b"http://ns.adobe.com/xap/1.0/mm/";
const XMP_NOTE: &[u8] = b"http://ns.adobe.com/xmp/note/";
const CRS: &[u8] = b"http://ns.adobe.com/camera-raw-settings/1.0/";
const PHOTOSHOP: &[u8] = b"http://ns.adobe.com/photoshop/1.0/";
const TIFF: &[u8] = b"http://ns.adobe.com/tiff/1.0/";
const EXIF: &[u8] = b"http://ns.adobe.com/exif/1.0/";
const IPTC_CORE: &[u8] = b"http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/";
const IPTC_EXT: &[u8] = b"http://iptc.org/std/Iptc4xmpExt/2008-02-29/";
const PLUS: &[u8] = b"http://ns.useplus.org/ldf/xmp/1.0/";
const HDR_GAIN_MAP: &[u8] = b"http://ns.adobe.com/hdr-gain-map/1.0/";
const APPLE_GAIN_MAP: &[u8] = b"http://ns.apple.com/HDRGainMap/1.0/";
const GOOGLE_CONTAINER: &[u8] = b"http://ns.google.com/photos/1.0/container/";
const GOOGLE_ITEM: &[u8] = b"http://ns.google.com/photos/1.0/container/item/";
const GOOGLE_CAMERA: &[u8] = b"http://ns.google.com/photos/1.0/camera/";
const GOOGLE_DEPTH: &[u8] = b"http://ns.google.com/photos/1.0/depthmap/";
const GOOGLE_IMAGE: &[u8] = b"http://ns.google.com/photos/1.0/image/";

fn rights(ns: &[u8], name: &[u8]) -> bool {
    ns == XMP_RIGHTS
        || matches!(
            (ns, name),
            (DC, b"rights" | b"creator")
                | (IPTC_CORE, b"CreatorContactInfo")
                | (PHOTOSHOP, b"Credit" | b"AuthorsPosition")
                | (TIFF, b"Artist" | b"Copyright")
                | (PLUS, b"CopyrightOwner" | b"ImageCreator" | b"Licensor")
        )
}
fn location(ns: &[u8], name: &[u8]) -> bool {
    (ns == EXIF && name.starts_with(b"GPS"))
        || matches!(
            (ns, name),
            (PHOTOSHOP, b"City" | b"State" | b"Country")
                | (IPTC_CORE, b"Location" | b"CountryCode")
                | (IPTC_EXT, b"LocationCreated" | b"LocationShown")
        )
}
/// Paths, history, raw settings, file containers and facts that export regenerates.
fn stale(ns: &[u8], name: &[u8]) -> bool {
    [XMP_MM, XMP_NOTE, CRS, HDR_GAIN_MAP, APPLE_GAIN_MAP, GOOGLE_CONTAINER, GOOGLE_ITEM, GOOGLE_CAMERA, GOOGLE_DEPTH, GOOGLE_IMAGE]
        .contains(&ns)
        || matches!(
            (ns, name),
            (XMP, b"Thumbnails")
                | (PHOTOSHOP, b"History" | b"DocumentAncestors" | b"ICCProfile" | b"ColorMode")
                | (
                    TIFF,
                    b"Orientation" | b"ImageWidth" | b"ImageLength" | b"XResolution" | b"YResolution"
                        | b"ResolutionUnit" | b"NativeDigest"
                )
                | (EXIF, b"PixelXDimension" | b"PixelYDimension" | b"NativeDigest")
        )
}
fn keep(ns: &[u8], name: &[u8], policy: ExportMetadata) -> bool {
    match policy.keep {
        MetadataKeep::None => false,
        MetadataKeep::CopyrightContact => rights(ns, name),
        MetadataKeep::All => !(stale(ns, name) || policy.remove_location && location(ns, name)),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Outside,
    Rdf,
    Description,
    Property(bool),
    Skipped,
}
/// Namespace declarations as raw `xmlns` keys and values.
type Declarations = Vec<(Vec<u8>, Vec<u8>)>;
struct Frame {
    role: Role,
    declarations: Declarations,
}

fn namespace(result: ResolveResult<'_>) -> Vec<u8> {
    match result {
        ResolveResult::Bound(ns) => ns.as_ref().to_vec(),
        _ => Vec::new(),
    }
}

fn declarations(start: &BytesStart<'_>) -> Result<Declarations, String> {
    let mut found = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| INVALID)?;
        if attribute.key.as_namespace_binding().is_some() {
            found.push((attribute.key.as_ref().to_vec(), attribute.value.to_vec()));
        }
    }
    Ok(found)
}

/// The kept `rdf:Description` elements, each declaring every namespace it
/// uses so that it can join another packet. None when nothing is kept.
pub(super) fn descriptions(packet: &[u8], policy: ExportMetadata) -> Result<Option<String>, String> {
    let mut reader = NsReader::from_reader(packet);
    let mut stack: Vec<Frame> = Vec::new();
    let mut output = Vec::new();
    let mut description: Option<(Writer<Vec<u8>>, usize)> = None;
    loop {
        let event = reader.read_event().map_err(|_| INVALID)?;
        let parent = stack.last().map_or(Role::Outside, |f| f.role);
        match &event {
            Event::Start(start) | Event::Empty(start) => {
                let (ns, local) = reader.resolver().resolve_element(start.name());
                let ns = namespace(ns);
                let own = declarations(start)?;
                let role = match parent {
                    Role::Outside if ns == RDF.as_bytes() && local.as_ref() == b"RDF" => Role::Rdf,
                    Role::Outside => Role::Outside,
                    Role::Rdf if ns == RDF.as_bytes() && local.as_ref() == b"Description" => {
                        let mut copy = start.to_owned();
                        copy.clear_attributes();
                        let mut hoisted: Vec<&(Vec<u8>, Vec<u8>)> = Vec::new();
                        for declaration in stack.iter().rev().flat_map(|f| &f.declarations) {
                            if !own.iter().chain(hoisted.iter().copied()).any(|(k, _)| *k == declaration.0) {
                                hoisted.push(declaration);
                            }
                        }
                        for (key, value) in hoisted {
                            copy.push_attribute(Attribute::from((key.as_slice(), value.as_slice())));
                        }
                        let mut kept = 0;
                        for attribute in start.attributes() {
                            let attribute = attribute.map_err(|_| INVALID)?;
                            let (ns, local) = reader.resolver().resolve_attribute(attribute.key);
                            let ns = namespace(ns);
                            let property = attribute.key.as_namespace_binding().is_none()
                                && ![RDF.as_bytes(), XML].contains(&ns.as_slice());
                            if property && !keep(&ns, local.as_ref(), policy) {
                                continue;
                            }
                            kept += usize::from(property);
                            let value = attribute
                                .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())
                                .map_err(|_| INVALID)?;
                            let key = std::str::from_utf8(attribute.key.as_ref()).map_err(|_| INVALID)?;
                            copy.push_attribute((key, value.as_ref()));
                        }
                        let mut writer = Writer::new(Vec::new());
                        writer
                            .write_event(if matches!(event, Event::Start(_)) {
                                Event::Start(copy)
                            } else {
                                Event::Empty(copy)
                            })
                            .map_err(|e| e.to_string())?;
                        description = Some((writer, kept));
                        Role::Description
                    }
                    Role::Rdf | Role::Skipped => Role::Skipped,
                    Role::Description => {
                        let kept = keep(&ns, local.as_ref(), policy);
                        if let Some((_, count)) = &mut description {
                            *count += usize::from(kept);
                        }
                        Role::Property(kept)
                    }
                    Role::Property(kept) => Role::Property(kept),
                };
                if let Role::Property(true) = role
                    && let Some((writer, _)) = &mut description
                {
                    writer.write_event(event.borrow()).map_err(|e| e.to_string())?;
                }
                if matches!(event, Event::Start(_)) {
                    stack.push(Frame { role, declarations: own });
                    if stack.len() > MAX_DEPTH {
                        return Err(INVALID.into());
                    }
                } else if role == Role::Description {
                    finish(&mut description, &mut output);
                }
            }
            Event::End(_) => {
                let frame = stack.pop().ok_or(INVALID)?;
                match frame.role {
                    Role::Description => {
                        if let Some((writer, _)) = &mut description {
                            writer.write_event(event.borrow()).map_err(|e| e.to_string())?;
                        }
                        finish(&mut description, &mut output);
                    }
                    Role::Property(true) => {
                        if let Some((writer, _)) = &mut description {
                            writer.write_event(event.borrow()).map_err(|e| e.to_string())?;
                        }
                    }
                    _ => (),
                }
            }
            Event::GeneralRef(reference) => {
                let name: &[u8] = reference.as_ref();
                let predefined = matches!(name, b"amp" | b"lt" | b"gt" | b"quot" | b"apos");
                if !predefined && !reference.resolve_char_ref().is_ok_and(|c| c.is_some()) {
                    return Err(INVALID.into());
                }
                write_content(parent, &mut description, &event)?;
            }
            Event::Text(_) | Event::CData(_) | Event::Comment(_) => {
                write_content(parent, &mut description, &event)?;
            }
            Event::DocType(_) => return Err(INVALID.into()),
            Event::Decl(_) | Event::PI(_) => (),
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        return Err(INVALID.into());
    }
    if output.is_empty() {
        return Ok(None);
    }
    String::from_utf8(output).map(Some).map_err(|_| INVALID.into())
}

fn write_content(
    parent: Role,
    description: &mut Option<(Writer<Vec<u8>>, usize)>,
    event: &Event<'_>,
) -> Result<(), String> {
    if parent == Role::Property(true)
        && let Some((writer, _)) = description
    {
        writer.write_event(event.borrow()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn finish(description: &mut Option<(Writer<Vec<u8>>, usize)>, output: &mut Vec<u8>) {
    if let Some((writer, kept)) = description.take()
        && kept > 0
    {
        output.extend(writer.into_inner());
    }
}

/// A complete packet around self-contained `rdf:Description` elements.
pub(super) fn packet(descriptions: &str) -> Vec<u8> {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF}\">{descriptions}</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    )
    .into_bytes()
}
