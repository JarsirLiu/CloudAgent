pub(crate) mod controller;
pub(crate) mod display;
pub(crate) mod lifecycle;
pub(crate) mod r#loop;
pub(crate) mod paste_coordinator;
pub(crate) mod terminal_projection;
pub(crate) mod viewport_height;

#[cfg(test)]
mod controller_tests;
#[cfg(test)]
mod paste_coordinator_tests;
#[cfg(test)]
mod viewport_height_tests;
