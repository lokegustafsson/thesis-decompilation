#include "AssemblyGraph.h"
#include "ControlFlow.h"
#include "AmbaException.h"

namespace assembly_graph {

Unpacked unpack(Packed packed) {
	const u64 val = packed.val;
	// Addresses either live at the bottom or top of the address
	// space, so we sign extend from the 48 bits we have kept
	u64 vaddr =  val & 0x0000'FFFF'FFFF'FFFF;
	if (vaddr & 1L << 47) {
		vaddr |= 0xFFFF'0000'0000'0000;
	}
	const u64 gen   = (val & 0x000F'0000'0000'0000) >> 48;
	const u64 state = (val & 0xFFF0'0000'0000'0000) >> 52;

	return (Unpacked) {
		.vaddr = vaddr,
		.gen = (u8) gen,
		.state = state,
	};
}

AssemblyGraph::AssemblyGraph(std::string name)
	: ControlFlow(name)
{}

StatePC AssemblyGraph::toAlias(UidS2E uid, u64 pc) {
	return pc << 4 | (u64) this->m_uuids[uid].val;
}

Packed AssemblyGraph::getBlockId(
	s2e::S2EExecutionState *s2e_state,
	u64 pc
) {
	const UidS2E state = UidS2E(s2e_state->getID());
	const StatePC state_pc = this->toAlias(state, pc);
	const Generation gen = this->m_generations[state_pc];
	const u64 vaddr = pc;

	const u64 packed
		= (0x0000'FFFF'FFFF'FFFF & vaddr)
		| (0x000F'0000'0000'0000 & ((u64) gen.val << 48))
		| (0xFFF0'0000'0000'0000 & ((u64) state.val << 52));

	{
		const Unpacked unpacked = unpack(packed);
		AMBA_ASSERT(vaddr == unpacked.vaddr);
		AMBA_ASSERT(gen == unpacked.gen);
		AMBA_ASSERT((u64) state.val == unpacked.state);
	}

	return Packed(packed);
}

void AssemblyGraph::translateBlockStart(
	s2e::ExecutionSignal *signal,
	s2e::S2EExecutionState *state,
	TranslationBlock *tb,
	u64 pc
) {
	const StatePC key = this->toAlias(state->getID(), pc);
	++this->m_generations[key].val;
}

void AssemblyGraph::onBlockStart(
	s2e::S2EExecutionState *state,
	u64 pc
) {
	const Packed curr = this->getBlockId(state, pc);
	// Will insert 0 if value doesn't yet exist
	auto &last = this->m_last[curr];
	control_flow::updateControlFlowGraph(
		this->m_cfg,
		last,
		curr
	);
	last = curr;
}
	
}
