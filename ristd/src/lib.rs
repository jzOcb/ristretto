//! Daemon core modules shared by the binary and tests.

pub mod agent_adapter;
pub mod context_injector;
pub mod context_monitor;
pub mod file_ownership;
pub mod git_manager;
pub mod handoff;
pub mod hooks;
pub mod output_filter;
pub mod planner;
pub mod pty_manager;
pub mod recovery;
pub mod review_engine;
pub mod ring_buffer;
pub mod session_store;
pub mod socket_server;
