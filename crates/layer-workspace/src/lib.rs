//! Workspace ownership and storage policy shared by native and browser hosts.
//! Hosts provide asynchronous transport, lifecycle events and native controls.
mod manager;
mod presentation;
pub use presentation::*;
mod model;
mod package;
mod protocol;
pub use package::*;
mod retention;
pub use manager::*;
pub use model::*;
pub use protocol::*;
pub use retention::*;
#[cfg(feature = "native")]
mod sqlite;
#[cfg(feature = "native")]
pub use sqlite::{Clock, SqliteStore};
#[cfg(feature = "native")]
mod worker;
#[cfg(feature = "native")]
pub use worker::{StoreReply, StoreWorker};
