//! What a retouching stroke samples. The stroke fixes its source at pen-down so
//! replays and late corrections sample the same layers.
use crate::{Document, OccurrenceHandle, SourceTarget, Point};
use std::{collections::BTreeSet, sync::Arc};

/// The pixels a retouching tool copies from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RetouchSource {
    /// The reference layers below the editing layer, with the editing layer
    /// over them, as the stroke found them.
    #[default]
    References,
    /// The editing layer alone, as the stroke found it.
    Editing,
}

/// A retouching stroke's source, fixed at pen-down.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Retouch {
    pub source: RetouchSource,
    /// Layers composing the reference pixels below the editing layer. Empty
    /// when the source is the editing layer alone.
    pub references: Arc<BTreeSet<OccurrenceHandle>>,
    /// A clone stroke copies editing-layer pixel `q` from `flip · q + offset`,
    /// in that layer's pixels, where a flipped axis scales by -1.
    pub offset: [f32; 2],
    pub flip: [bool; 2],
}

impl Retouch {
    /// The source `source` samples for a stroke on `target`. With no reference
    /// below the target, the target alone is the source.
    pub fn for_target(document: &Document, target: SourceTarget, source: RetouchSource) -> Self {
        let references = match source {
            RetouchSource::References => document.scene().source_owner(target).map(|h|document.references_below(h)).unwrap_or_default(),
            RetouchSource::Editing => BTreeSet::new(),
        };
        Self { source, references: Arc::new(references), ..Self::default() }
    }

    /// Copy through a document mapping `p ↦ flip · p + offset`, for a layer
    /// whose pixel `q` sits at document point `q + origin`.
    pub fn cloning(self, offset: [f32; 2], flip: [bool; 2], origin: Point) -> Self {
        let scale = flip.map(|f| if f { -1. } else { 1. });
        let origin = [origin.x, origin.y];
        let offset = std::array::from_fn(|axis| offset[axis] + (scale[axis] - 1.) * origin[axis]);
        Self { offset, flip, ..self }
    }
}

/// Where the Clone tool copies from, kept with the document. The next stroke
/// starts copying at `point`. An aligned source keeps the offset its first
/// stroke found, and follows each stroke, until the source is set again or
/// its offset is reset; otherwise every stroke starts again at `point`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloneSource {
    pub point: Option<Point>,
    /// Aligned strokes copy document point `p` from `scale · p + offset`.
    pub offset: Option<[f32; 2]>,
    pub aligned: bool,
    pub flip: [bool; 2],
}

impl Default for CloneSource {
    fn default() -> Self {
        Self { point: None, offset: None, aligned: true, flip: [false; 2] }
    }
}

impl CloneSource {
    fn scale(&self) -> [f32; 2] {
        self.flip.map(|f| if f { -1. } else { 1. })
    }

    fn map(&self, offset: [f32; 2], p: Point) -> Point {
        let [sx, sy] = self.scale();
        Point { x: sx * p.x + offset[0], y: sy * p.y + offset[1] }
    }

    /// Start copying from `point`, anchored at the next stroke.
    pub fn set(&mut self, point: Point) {
        self.point = Some(point);
        self.offset = None;
    }

    /// Move the source to `point`, shifting an established offset with it.
    pub fn move_to(&mut self, point: Point) {
        if let (Some(from), Some(offset)) = (self.point, self.offset.as_mut()) {
            offset[0] += point.x - from.x;
            offset[1] += point.y - from.y;
        }
        self.point = Some(point);
    }

    /// The next stroke starts copying at the source point again.
    pub fn reset_offset(&mut self) {
        self.offset = None;
    }

    pub fn set_aligned(&mut self, aligned: bool) {
        self.aligned = aligned;
        self.offset = None;
    }

    /// Mirror the copy along `axis` (0 horizontal, 1 vertical), keeping the
    /// source point where it is.
    pub fn toggle_flip(&mut self, axis: usize) {
        let anchor = self.point.zip(self.offset).map(|(point, offset)| {
            let [sx, sy] = self.scale();
            Point { x: sx * (point.x - offset[0]), y: sy * (point.y - offset[1]) }
        });
        self.flip[axis] = !self.flip[axis];
        if let (Some(point), Some(anchor)) = (self.point, anchor) {
            let [sx, sy] = self.scale();
            self.offset = Some([point.x - sx * anchor.x, point.y - sy * anchor.y]);
        }
    }

    /// The offset a stroke starting at document point `first` copies with,
    /// kept by an aligned source. None until a source is set.
    pub fn begin_stroke(&mut self, first: Point) -> Option<[f32; 2]> {
        let point = self.point?;
        let [sx, sy] = self.scale();
        let offset = self.offset.unwrap_or([point.x - sx * first.x, point.y - sy * first.y]);
        if self.aligned {
            self.offset = Some(offset);
        }
        Some(offset)
    }

    /// An aligned source continues from the stroke's last point, `last`.
    pub fn end_stroke(&mut self, last: Point) {
        if let Some(offset) = self.offset.filter(|_| self.aligned) {
            self.point = Some(self.map(offset, last));
        }
    }

    /// Where a stroke under the current offset would copy `p` from.
    pub fn source_of(&self, p: Point) -> Option<Point> {
        self.offset.map(|offset| self.map(offset, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation_test_support as fixture;
    use fixture as f;

    fn document() -> Document {
        let mut doc=f::document([64,64],&["Group","Above","Target","Below","Photo"]);
        f::nest(&mut doc,"Group",&["Above","Target","Below"]);
        doc
    }
    fn references(doc:&mut Document,names:&[&str]) {
        let handles=doc.scene().order().to_vec();
        for h in handles {let o=doc.artwork.occurrences.get_mut(h).unwrap();o.reference=names.contains(&o.name.as_ref());}
    }
    fn names(doc:&Document,members:&BTreeSet<OccurrenceHandle>)->BTreeSet<String> {
        members.iter().map(|h|doc.artwork.occurrences.get(*h).unwrap().name.to_string()).collect()
    }

    #[test]
    fn references_below_skip_the_target_and_everything_above_it() {
        let mut doc=document();
        let members=|doc:&Document| Retouch::for_target(doc,f::target(doc,"Target"),RetouchSource::References).references;
        assert!(members(&doc).is_empty(),"nothing marked samples the target alone");
        references(&mut doc,&["Above","Target"]);
        assert!(members(&doc).is_empty(),"references above the target and the target itself are ignored");
        references(&mut doc,&["Below","Photo"]);
        assert_eq!(names(&doc,&members(&doc)),["Group","Below","Photo"].map(String::from).into(),"a member's ancestor group composes it");
        references(&mut doc,&["Group"]);
        assert_eq!(names(&doc,&members(&doc)),["Group","Below"].map(String::from).into(),"a marked group contributes its children below the target");
        assert!(Retouch::for_target(&doc,f::target(&doc,"Target"),RetouchSource::Editing).references.is_empty());
    }

    #[test]
    fn a_clipping_stack_below_keeps_only_clips_below_the_target() {
        let mut doc=f::document([64,64],&["Top clip","Target","Low clip","Base"]);
        for name in ["Top clip","Target","Low clip"] {f::occurrence_mut(&mut doc,name).attachment = crate::Attachment::Clip;}f::refresh(&mut doc);
        references(&mut doc,&["Base"]);
        assert_eq!(names(&doc,&doc.references_below(f::id(&doc,"Target"))),["Low clip","Base"].map(String::from).into());
    }

    #[test]
    fn an_adjustment_below_brings_its_inputs() {
        let mut doc=f::document([64,64],&["Target","Curves","Input"]);
        f::effect(&mut doc,"Curves","curves");
        references(&mut doc,&["Curves"]);
        let members=doc.references_below(f::id(&doc,"Target"));
        assert!(members.contains(&f::id(&doc,"Curves"))&&members.contains(&f::id(&doc,"Input")),"{members:?}");
    }

    fn at(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    #[test]
    fn an_aligned_source_keeps_its_first_offset_and_follows_each_stroke() {
        let mut source = CloneSource::default();
        assert_eq!(source.begin_stroke(at(1., 1.)), None, "nothing to copy before a source is set");
        source.set(at(100., 50.));
        assert_eq!(source.begin_stroke(at(10., 20.)), Some([90., 30.]));
        source.end_stroke(at(40., 20.));
        assert_eq!(source.point, Some(at(130., 50.)), "the source follows the stroke");
        assert_eq!(source.begin_stroke(at(300., 300.)), Some([90., 30.]), "later strokes keep the offset");
        source.reset_offset();
        assert_eq!(source.begin_stroke(at(0., 0.)), Some([130., 50.]), "a reset starts at the source again");
        source.move_to(at(140., 60.));
        assert_eq!(source.offset, Some([140., 60.]), "moving the source moves its offset");
        source.set(at(5., 5.));
        assert_eq!(source.offset, None);
    }

    #[test]
    fn a_non_aligned_source_starts_every_stroke_at_its_point() {
        let mut source = CloneSource { aligned: false, ..CloneSource::default() };
        source.set(at(100., 50.));
        assert_eq!(source.begin_stroke(at(10., 20.)), Some([90., 30.]));
        source.end_stroke(at(40., 20.));
        assert_eq!(source.point, Some(at(100., 50.)));
        assert_eq!(source.begin_stroke(at(0., 0.)), Some([100., 50.]));
        assert_eq!(source.offset, None);
    }

    #[test]
    fn flipping_mirrors_about_the_source_point() {
        let mut source = CloneSource::default();
        source.set(at(100., 50.));
        source.toggle_flip(0);
        let offset = source.begin_stroke(at(10., 20.)).unwrap();
        assert_eq!(offset, [110., 30.]);
        assert_eq!(source.source_of(at(10., 20.)), Some(at(100., 50.)));
        assert_eq!(source.source_of(at(15., 20.)), Some(at(95., 50.)), "moving right copies leftwards");
        source.toggle_flip(1);
        assert_eq!(source.source_of(at(10., 20.)), Some(at(100., 50.)), "flipping keeps the anchored source");
        assert_eq!(source.source_of(at(10., 25.)), Some(at(100., 45.)));
        let retouch = Retouch::default().cloning(offset, [true, false], at(4., 6.));
        assert_eq!(retouch.offset, [102., 30.], "layer pixel q sits at q + origin");
        assert_eq!(retouch.flip, [true, false]);
    }
}
