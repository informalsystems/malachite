// -*- mode: Bluespec; -*-
//
// .load consensus.qnt consensus

module consensus {

	type Address = int
	type Round = int
	type Step =
		| Propose
		| Prevote
		| Precommit

	type Vote = (Round, Step, Address)
	pure def round(v: Vote) : Round = v._1
	pure def rstep(v: Vote) : Step = v._2
	pure def sender(v: Vote) : Address = v._3

	type RoundVotes = {
		prevotes: Set[Address],
		precommits: Set[Address],
	}

	pure def NewRoundVotes() : RoundVotes = {
		prevotes: Set(),
		precommits: Set(),
	}

	pure def addRoundVote(r: RoundVotes, v: Vote) : RoundVotes = {
		val new = Set(v.sender())
		match v.rstep() {
			| Prevote => {...r, prevotes: r.prevotes.union(new)}
			| Precommit => {...r, precommits: r.precommits.union(new)}
			| _ => r // Should not happen
		}
	}

	// FIXME: this is a workaround, the quorum should be a const
	// NOTE: a const is not available for pure def methods though
	val quorum = 3

	type Consensus = {
		id: Address,
		messages: List[Vote],

		round: Round,
		step: Step,
		votes: Round -> RoundVotes,
	}

	pure def NewConsensus(id: Address) : Consensus = {
		id: id,
		messages: List(),		

		round: 0,
		step: Propose,
		votes: Map(),
	}

	pure def ensureRound(c: Consensus, r: Round) : Consensus = {
		{...c,
		votes: 0.to(r).fold(c.votes, (v, r) =>
			if (not(v.keys().contains(r))) v.put(r, NewRoundVotes)
			else v
		)}
	}

	pure def addVote(c: Consensus, v: Vote) : Consensus = {
		{...c.ensureRound(v.round()),
		votes: c.votes.setBy(v.round(), (orig => orig.addRoundVote(v)))}
	}

	pure def proposeStep(c: Consensus) : Consensus = {
		val prevote = (c.round, Prevote, c.id)
		{...c.addVote(prevote),
		messages: c.messages.append(prevote),
		step: Prevote}
	}

	pure def prevoteStep(c: Consensus) : Consensus = {
		if (c.votes.get(c.round).prevotes.size() >= quorum)
			val precommit = (c.round, Precommit, c.id)
			{...c.addVote(precommit),
			messages: c.messages.append(precommit),
			step: Precommit}
		else	c
	}

	pure def precommitStep(c: Consensus) : Consensus = {
		if (c.votes.get(c.round).precommits.size() >= quorum)
			{...c,
			step: Propose,
			round: c.round + 1}
		else	c
	}

	pure def stepLogic(c: Consensus) : Consensus = {
		match c.step {
			| Propose => c.proposeStep()
			| Prevote => c.prevoteStep()
			| Precommit => c.precommitStep()
		}
	}

	// State-machine

	var processes : Address -> Consensus

	action init = all {
		processes' = 1.to(quorum+1).mapBy(id => NewConsensus(id)),
	}

	action deliver(id, vote) = all {
		processes' = processes.setBy(id,
			(p => p.addVote(vote).stepLogic())
		)
	}

	action consensusStep(id) = all {
		val p = processes.get(id)
		val nextP = p.stepLogic()
		all {
			nextP != p,
			processes' = processes.put(id, nextP),
		}
	}

	action step = {
		nondet id = oneOf(processes.keys())
		any {
			consensusStep(id),
			all {
				size(allMessages) > 0,
				nondet vote = oneOf(allMessages)
				deliver(id, vote),
			}
		}
	}

	// Helper to collect all messages produced by processes
	def allMessages =
		processes.keys().fold(Set(), (s, id) =>
			s.union(processes.get(id).messages.
				foldl(Set(), (ss, v) => ss.union(Set(v)))
			)
		)

	// Debugging helpers

	def p = processes
	def a = allMessages
	def s =
		processes.keys().fold(Set(), (s, id) =>
			val p = processes.get(id)
			s.union(Set((id, p.round, p.step)))
		)

	// Invariants and Witnesses

	def allRounds =
		processes.keys().fold(Set(), (s, id) =>
			s.union(Set(processes.get(id).round))
		)

	// Processes are in different rounds (common)
	def diffRoundsWitness =
		size(allRounds) > 1

	// Processes are in different rounds, with delta at least 2
	// Less common, but try with --max-steps 100 [--seed=0xced99098f568]
	def diff2RoundsWitness =
		size(allRounds) > 2

}
