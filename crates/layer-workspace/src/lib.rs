//! Workspace ownership and storage policy shared by native and browser hosts.
//! Hosts provide asynchronous transport, lifecycle events and native controls.
mod model;
mod protocol;
pub use model::*;
pub use protocol::*;
#[cfg(feature = "native")]
mod sqlite;
#[cfg(feature = "native")]
pub use sqlite::{Clock, SqliteStore};
#[cfg(feature = "native")]
mod worker;
#[cfg(feature = "native")]
pub use worker::{StoreReply, StoreWorker};
