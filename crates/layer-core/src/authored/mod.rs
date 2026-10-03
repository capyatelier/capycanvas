mod identity;
mod store;
mod topology;
pub use identity::PortableId;
pub use store::{Handle, Store};
pub use topology::{Content, GraphLimits, GraphShape, Shape, Support};

#[cfg(test)]
mod tests;
