mod identity;
mod resource;
mod store;
mod topology;
pub use identity::PortableId;
pub use resource::{EncodedBytes, Resource, ResourceEncoding};
pub use store::{Handle, Store};
pub use topology::{Content, GraphLimits, GraphShape, Shape, Support};

#[cfg(test)]
mod tests;

mod artwork;
pub use artwork::*;
mod extensions;
pub use extensions::{Extensions, OpaqueResource};
