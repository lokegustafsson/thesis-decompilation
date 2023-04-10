use std::{borrow::Cow, ffi::CStr, os::unix::net::UnixStream, sync::Mutex};

use data_structures::{ControlFlowGraph, GraphIpc};

static IPC: Mutex<Option<ipc::IpcTx<'static>>> = Mutex::new(None);

fn with_ipc(f: impl FnOnce(&mut ipc::IpcTx<'static>)) {
	let mut guard = IPC.lock().unwrap();
	let ipc = guard.get_or_insert_with(|| match UnixStream::connect("amba-ipc.socket") {
		Ok(stream) => {
			let stream = Box::leak(Box::new(stream));
			let (tx, _rx) = ipc::new_wrapping(&*stream);
			tx
		}
		Err(err) => panic!("libamba failed to connect to IPC socket: {err:?}"),
	});
	f(ipc);
}

#[allow(unsafe_code, clippy::missing_safety_doc)]
mod ffi {
	use super::*;

	/// Create a newly allocated `ControlFlowGraph` and return an
	/// owning raw pointer. This pointer may only be freed with
	/// the `rust_free_control_flow_graph` function.
	#[no_mangle]
	pub extern "C" fn rust_new_control_flow_graph() -> *mut Mutex<ControlFlowGraph> {
		Box::into_raw(Box::new(Mutex::new(ControlFlowGraph::new())))
	}

	/// Free a `ControlFlowGraph` allocated by
	/// `rust_new_control_flow_graph`. After this fuction has been
	/// called the pointer may not be used again.
	#[no_mangle]
	pub unsafe extern "C" fn rust_free_control_flow_graph(ptr: *mut Mutex<ControlFlowGraph>) {
		let _ = Box::from_raw(ptr);
	}

	/// Wrapper around `ControlFlowGraph::update`. May only be
	/// called with a pointer allocated by
	/// `rust_new_control_flow_graph`. Returns true if the graph
	/// has changed.
	#[no_mangle]
	pub unsafe extern "C" fn rust_update_control_flow_graph(
		ptr: *mut Mutex<ControlFlowGraph>,
		from: u64,
		to: u64,
	) -> bool {
		let mutex = &*ptr;
		let mut cfg = mutex.lock().unwrap();
		cfg.update(from, to)
	}

	#[no_mangle]
	pub unsafe extern "C" fn rust_print_graph_size(
		name: *const i8,
		ptr: *mut Mutex<ControlFlowGraph>,
	) {
		let name = CStr::from_ptr(name).to_string_lossy();
		let mutex = &*ptr;
		let cfg = mutex.lock().unwrap();
		println!("\nGraph of: {name}\n{cfg}");
	}

	#[no_mangle]
	pub unsafe extern "C" fn rust_ipc_send_graph(
		name: *const i8,
		graph: *mut Mutex<ControlFlowGraph>,
	) {
		let name = CStr::from_ptr(name).to_string_lossy();
		with_ipc(|ipc| {
			let graph = (&*graph).lock().unwrap();
			ipc.blocking_send(&ipc::IpcMessage::GraphSnapshot {
				name,
				graph: Cow::Owned(GraphIpc::from(&graph.graph)),
			})
			.unwrap_or_else(|err| println!("libamba ipc error: {err:?}"));
		});
	}

	#[no_mangle]
	pub extern "C" fn rust_main() -> std::ffi::c_int {
		println!("Hello world");
		0
	}
}