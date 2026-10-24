//! Terminal panel (feature-inventory §1.10): a libghostty-vt-backed
//! emulator fed by the engine's PTY stream over the generic RPC client.
//!
//! - [`emulator`] — pure libghostty-vt state machine (bytes in, grid out);
//! - [`keys`] — keystroke→bytes through Ghostty's key encoder;
//! - [`links`] — URLs and `file:line` paths in rows, for Cmd+click;
//! - [`mouse`] — pointer→bytes through Ghostty's mouse encoder, and the
//!   wheel as arrow keys on the alternate screen;
//! - [`view`] — cell palette, paste encoding, input coalescing, and
//!   the custom grid-painting element;
//! - [`panel`] — session-scoped tabs, subscriptions with reconnect backoff,
//!   drag-reorder, and the Cmd/Ctrl+J toggle action.
//!
//! Method names come from `zeron_rpc::methods` and wire types from
//! `zeron_proto` (`TerminalSession`, `TerminalEvent`) — the same contract the
//! engine serves (feature-inventory §2.1).

pub(crate) mod dock;
pub mod emulator;
pub mod keys;
pub mod links;
pub mod mouse;
pub mod panel;
pub mod sprite;
pub mod view;
