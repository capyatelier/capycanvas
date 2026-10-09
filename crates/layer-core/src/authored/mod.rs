mod identity;
mod resource;
mod store;
mod topology;
pub use identity::PortableId;
pub use resource::{EncodedBytes, Resource, ResourceEncoding};
pub(crate) use resource::{EncodedIntegrity, IntegrityCache};
pub use store::{Handle, RecordChange, Store};
pub use topology::{Content, GraphError, GraphLimits, GraphShape, Shape, Support};

#[cfg(test)]
mod tests;

mod artwork;
pub use artwork::*;
mod extensions;
pub use extensions::{Extensions, OpaqueResource};
mod scene;
pub use scene::{EffectBaseline, SceneIndex, SceneScope, SceneSnapshot, SceneView};

mod occurrence_edits;
pub use occurrence_edits::{OccurrenceDropPlan, OccurrenceDropPosition};

mod objects;
mod object_edits;
pub use object_edits::{placed_bounds};
pub use objects::{Image, Affine64, Affine64Error, ImageInterpolation, ImageObject, PaintBase, PaintBasePolicy, MAX_NAME_BYTES, MAX_NAME_CHARS, bounded_name};
