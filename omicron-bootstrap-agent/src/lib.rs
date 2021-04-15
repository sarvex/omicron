/*!
 * Library interface to the bootstrap agent
 */

/*
 * We only use rustdoc for internal documentation, including private items, so
 * it's expected that we'll have links to private items in the docs.
 */
#![allow(private_intra_doc_links)]
/* Clippy's style lints are useful, but not worth running automatically. */
#![allow(clippy::style)]

mod bootstrap_agent;
mod bootstrap_agent_client;
mod config;
mod http_entrypoints;
mod server;

pub use config::Config;
pub use server::Server;

#[macro_use]
extern crate slog;
