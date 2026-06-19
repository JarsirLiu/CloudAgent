//! Conversation-domain wiring for the CLI app.
//!
//! This directory stays small on purpose:
//! - `actions/` owns input-to-action handling and server action reduction.
//! - `event_router.rs` bridges app-server events into the conversation reducer.
//! - `facade.rs` coordinates transcript resets, history rebuilds, and view sync.
//! - `projection.rs` turns stored history into renderable cells and copyable output.
//! - `exploration.rs` contains lightweight command classification helpers.
//! - `image_paste.rs` handles clipboard image/text paste into the composer.

pub(crate) mod actions;
pub(crate) mod event_router;
pub(crate) mod exploration;
pub(crate) mod facade;
pub(crate) mod image_paste;
pub(crate) mod projection;
