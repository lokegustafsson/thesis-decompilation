#include <thread>
#include <chrono>
#include <vector>
#include <unordered_set>
#include <algorithm>
#include <ranges>

#include <s2e/S2E.h>
#include <s2e/S2EExecutionState.h>
#include <klee/Searcher.h>
#include <klee/SolverImpl.h>

#include "StatePrioritisation.h"
#include "Amba.h"
#include "LibambaRs.h"
#include "klee/Common.h"

namespace state_prioritisation {

// These pointers are not a race condition because the thread has to
// join before the AmbaPlugin fields can be destructed
void ipcReceiver(Ipc *ipc, bool *active, s2e::S2E *s2e) {
	using IdSet = std::unordered_set<i32>;
	using StateSet = klee::StateSet;

	std::vector<i32> receive_buffer {};
	StateSet prioritised_states;

	while (*active) {
		std::this_thread::sleep_for(std::chrono::milliseconds(200));
		receive_buffer.clear();

		bool received = rust_ipc_receive_message(ipc, &receive_buffer);
		if (!received) {
			continue;
		}

		const IdSet to_prioritise_ids = ([&]() {
			IdSet s {};
			for (auto id : receive_buffer) {
				s.insert(id);
			}
			return s;
		})();

		auto &executor = *s2e->getExecutor();
		auto searcher = (klee::DFSSearcher *) executor.getSearcher(); // Is this kind of downcast even safe?

		const StateSet &all_states = executor.getStates();
		const StateSet to_prioritise_states = ([&]() {
			StateSet s {};

			for (auto state : all_states) {
				auto id = ((s2e::S2EExecutionState *) state)->getGuid();
				if (!to_prioritise_ids.contains(id)) {
					continue;
				}
				s.insert(state);
			}

			return s;
		})();

		searcher->update(
			// No idea where to get this value from, but looking at the source code, it's unused anyway
			nullptr,
			{},
			{}
		);

	}

	*amba::debug_stream() << "Exited ipc receiver thread\n";
}

}
