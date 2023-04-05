use std::{
	borrow::Cow,
	cmp::Ordering,
	collections::HashMap,
	ffi::CStr,
	mem,
	num::{NonZeroU64, NonZeroUsize},
	os::unix::net::UnixStream,
	ptr::NonNull,
	sync::Mutex,
	thread,
	time::Duration,
};

use data_structures::{Graph, GraphIpc};
use ipc::{IpcError, IpcMessage, IpcTx};

static STATE: Mutex<Option<State>> = Mutex::new(None);

struct State {
	// IPC
	ipc_tx: IpcTx<'static>,

	// Mutable local state
	symbolic_state_alias_to_id: Vec<u32>,
	symbolic_state_id_count: u32,
	symbolic_state_to_last_executed_basic_block_id: Vec<u64>,
	state_predecessor: Vec<u32>,
	basic_block_state_vaddr_to_generation: HashMap<(u32, NonZeroU64), u8>,

	// Payload
	symbolic_state_graph: Graph,
	basic_block_graph: Graph,
}
impl State {
	fn init() {
		let mut slot = STATE.lock().unwrap();
		assert!(slot.is_none());

		*slot = Some(Self {
			ipc_tx: match UnixStream::connect("amba-ipc.socket") {
				Ok(stream) => {
					let stream = Box::leak(Box::new(stream));
					let (tx, mut rx) = ipc::new_wrapping(&*stream);
					thread::spawn(move || loop {
						match rx.blocking_receive() {
							Ok(_) => println!("libamba received ipc message"),
							Err(IpcError::EndOfFile) => return,
							Err(other) => panic!("ipc error: {other:?}"),
						}
					});
					tx
				}
				Err(err) => panic!("libamba failed to connect to IPC socket: {err:?}"),
			},
			symbolic_state_alias_to_id: vec![0],
			symbolic_state_id_count: 1,
			symbolic_state_to_last_executed_basic_block_id: Vec::new(),
			state_predecessor: vec![u32::MAX],
			basic_block_state_vaddr_to_generation: HashMap::new(),

			symbolic_state_graph: Graph::default(),
			basic_block_graph: Graph::default(),
		});
		thread::spawn(|| loop {
			thread::sleep(Duration::from_millis(100));

			let mut guard = STATE.lock().unwrap();
			guard.as_mut().unwrap().send_graph_snapshot();
		});
	}

	fn send_graph_snapshot(&mut self) {
		if self.symbolic_state_graph.is_empty() && self.basic_block_graph.is_empty() {
			return;
		}
		self.ipc_tx
			.blocking_send(&IpcMessage::GraphSnapshot {
				name: Cow::Borrowed("symbolic states"),
				graph: Cow::Owned(GraphIpc::from(&self.symbolic_state_graph)),
			})
			.unwrap_or_else(|err| println!("libamba ipc error sending symbolic graph: {err:?}"));
		self.ipc_tx
			.blocking_send(&IpcMessage::GraphSnapshot {
				name: Cow::Borrowed("basic blocks"),
				graph: Cow::Owned(GraphIpc::from(&self.basic_block_graph)),
			})
			.unwrap_or_else(|err| println!("libamba ipc error sending symbolic graph: {err:?}"));
	}

	fn shutdown(&mut self) {
		self.send_graph_snapshot();
	}

	fn on_state_fork(&mut self, old_state_alias: u32, new_state_aliases: &[u32]) {
		let old_state_id = self.symbolic_state_alias_to_id[old_state_alias as usize];
		for &new_state_alias in new_state_aliases {
			// Newly forked states are always newly allocated
			let new_state_id = self.symbolic_state_id_count;
			self.symbolic_state_id_count += 1;
			// But they might be aliased by an existing state alias
			match new_state_alias.cmp(&(self.symbolic_state_alias_to_id.len() as u32)) {
				Ordering::Less => {
					self.symbolic_state_alias_to_id[new_state_alias as usize] = new_state_id;
				}
				Ordering::Equal => self.symbolic_state_alias_to_id.push(new_state_id),
				Ordering::Greater => unreachable!(),
			}
			self.symbolic_state_graph
				.update(u64::from(old_state_id), u64::from(new_state_id));
			assert_eq!(
				new_state_id as usize,
				self.symbolic_state_to_last_executed_basic_block_id.len()
			);
			assert_eq!(
				new_state_id as usize,
				self.state_predecessor.len(),
			);
			self.symbolic_state_to_last_executed_basic_block_id
				.push(self.symbolic_state_to_last_executed_basic_block_id[old_state_id as usize]);
			self.state_predecessor.push(old_state_id);
		}
	}

	fn on_state_merge(&mut self, base_state_alias: u32, other_state_alias: u32) {
		self.symbolic_state_graph.update(
			u64::from(self.symbolic_state_alias_to_id[other_state_alias as usize]),
			u64::from(self.symbolic_state_alias_to_id[base_state_alias as usize]),
		);
	}

	fn on_translate_block(
		&mut self,
		current_state_alias: u32,
		block_virtual_addr: u64,
		block: Option<&[u8]>,
		module_path: Option<&CStr>,
		module_internal_offset: Option<NonZeroU64>,
	) {
		let current_state_id = self.symbolic_state_alias_to_id[current_state_alias as usize];
		*self
			.basic_block_state_vaddr_to_generation
			.entry((
				current_state_id,
				NonZeroU64::new(block_virtual_addr).unwrap(),
			))
			.or_insert(0) += 1;
		println!("Translating block at {block_virtual_addr} (raw: {block:?}) at offset {module_internal_offset:?} within module {module_path:?}");
	}

	fn on_watched_block_start_execute(
		&mut self,
		current_state_alias: u32,
		block_virtual_addr: NonZeroU64,
	) {
		let current_state_id = self.symbolic_state_alias_to_id[current_state_alias as usize];
		if current_state_id == 0
			&& self
				.symbolic_state_to_last_executed_basic_block_id
				.is_empty()
		{
			// Very first block execution of state 0
			self.symbolic_state_to_last_executed_basic_block_id
				.push(block_virtual_addr.get());
			return;
		}
		let current_id = block_virtual_addr.get();
		let last_id = mem::replace(
			&mut self.symbolic_state_to_last_executed_basic_block_id[current_state_id as usize],
			current_id,
		);

		self.basic_block_graph.update(last_id, current_id);
	}
}

#[allow(unsafe_code, clippy::missing_safety_doc)]
mod ffi {
	use super::*;

	#[no_mangle]
	pub extern "C" fn rust_init() {
		State::init();
	}

	#[no_mangle]
	pub extern "C" fn rust_on_engine_shutdown() {
		STATE.lock().unwrap().as_mut().unwrap().shutdown();
	}

	#[no_mangle]
	pub unsafe extern "C" fn rust_on_state_fork(
		old_state_alias: u32,
		new_state_aliases_ptr: NonNull<u32>,
		new_state_aliases_count: NonZeroUsize,
	) {
		let new_state_aliases: &[u32] = std::slice::from_raw_parts(
			new_state_aliases_ptr.as_ptr(),
			new_state_aliases_count.get(),
		);
		STATE
			.lock()
			.unwrap()
			.as_mut()
			.unwrap()
			.on_state_fork(old_state_alias, new_state_aliases);
	}

	#[no_mangle]
	pub extern "C" fn rust_on_state_merge(base_state_alias: u32, other_state_alias: u32) {
		STATE
			.lock()
			.unwrap()
			.as_mut()
			.unwrap()
			.on_state_merge(base_state_alias, other_state_alias);
	}

	#[no_mangle]
	/// A module is a file on disk, e.g. an ELF executable or library.
	pub unsafe extern "C" fn rust_on_translate_block(
		current_state_alias: u32,
		block_virtual_addr: u64,
		block_data: *const u8,
		block_len: usize,
		module_path_cstr: *const std::ffi::c_char,
		module_internal_offset: u64,
	) {
		let block: Option<&[u8]> = Option::zip(
			NonNull::new(block_data as *mut u8),
			NonZeroUsize::new(block_len),
		)
		.map(|(data, len)| std::slice::from_raw_parts(data.as_ptr(), len.get()));
		let module_path_str = NonNull::new(module_path_cstr as *mut std::ffi::c_char)
			.map(|ptr| CStr::from_ptr(ptr.as_ptr()));

		STATE.lock().unwrap().as_mut().unwrap().on_translate_block(
			current_state_alias,
			block_virtual_addr,
			block,
			module_path_str,
			NonZeroU64::new(module_internal_offset),
		);
	}

	#[no_mangle]
	pub extern "C" fn rust_on_watched_block_start_execute(
		current_state_alias: u32,
		block_virtual_addr: u64,
	) {
		STATE
			.lock()
			.unwrap()
			.as_mut()
			.unwrap()
			.on_watched_block_start_execute(
				current_state_alias,
				NonZeroU64::new(block_virtual_addr).unwrap(),
			);
	}
}
