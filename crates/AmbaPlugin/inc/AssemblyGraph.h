#pragma once

#include <string>

#include "ControlFlow.h"

namespace assembly_graph {

using namespace control_flow::types;

void updateControlFlowGraph(ControlFlowGraph *, PackedNodeData, PackedNodeData);
Unpacked unpack(PackedNodeData);

class AssemblyGraph : public control_flow::ControlFlow {
  public:
	AssemblyGraph(std::string);

	amba::TranslationFunction translateBlockStart;
	amba::ExecutionFunction onBlockStart;
	amba::SymbolicExecutionFunction onStateFork;
	amba::StateMergeFunction onStateMerge;

  protected:
	StatePC packStatePc(IdS2E, u64);
	PackedNodeData getPacked(s2e::S2EExecutionState *, u64);

	std::unordered_map<StatePC, BasicBlockGeneration> m_generations {};
	std::unordered_map<IdAmba, PackedNodeData> m_last {};
};

}
