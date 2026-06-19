//! Bottom-pane UI domain.
//!
//! This groups the interactive composer, the transient overlays, and the shared input/support
//! pieces into stable module boundaries, closer to Codex's `bottom_pane` organization.
pub mod bottom_pane_view;
pub mod chat_composer;
pub mod dialogs;
pub mod input_pane;
pub mod support;
