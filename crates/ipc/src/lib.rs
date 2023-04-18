mod graph;
mod ipc;
mod metadata;

pub use crate::{
	graph::{GraphIpc, GraphIpcBuilder},
	ipc::{new_wrapping, IpcError, IpcMessage, IpcRx, IpcTx},
	metadata::NodeMetadata,
};
