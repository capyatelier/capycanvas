//! Workspace ownership and storage policy shared by native and browser hosts.
//! Hosts provide asynchronous transport, lifecycle events and native controls.
mod manager;
mod presentation;
pub use presentation::*;
mod prompts;
pub use prompts::*;
mod history_presentation;
pub use history_presentation::*;
mod model;
mod package;
mod protocol;
pub use package::*;
mod retention;
mod browser;
pub use browser::BrowserDatabase;
mod controller;
pub use controller::*;
#[cfg(all(test, feature = "native"))]
mod browser_tests;
#[cfg(all(test, feature = "native"))]
mod controller_tests;
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
pub use worker::{StoreReply, StoreWorker, validate_database_export_destination};
