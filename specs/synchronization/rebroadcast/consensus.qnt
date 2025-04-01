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
		timeout: bool,
	}

	pure def NewConsensus(id: Address) : Consensus = {
		id: id,
		messages: List(),		

		round: 0,
		step: Propose,
		votes: Map(),
		timeout: false,
	}

	pure def startRound(c: Consensus, r: Round) : Consensus = {
		...c,
		step: Propose,
		round: r,
		timeout: false,
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
			c.startRound(c.round + 1)
		else	c
	}

	pure def precommitExitStep(c: Consensus) : Consensus = {
		if (c.votes.get(c.round).precommits.size() >= quorum)
			{...c,
			timeout: true}
		else	c
	}

	pure def stepLogic(c: Consensus) : Consensus = {
		if (c.timeout)
			c.startRound(c.round + 1)
		else
			match c.step {
				| Propose => c.proposeStep().precommitExitStep()
				| Prevote => c.prevoteStep().precommitExitStep()
				| Precommit => c.precommitStep()
			}
	}

	pure def ensureRound(c: Consensus, r: Round) : Consensus = {
		{...c,
		votes: 0.to(r).fold(c.votes, (v, r) =>
			if (not(v.keys().contains(r))) v.put(r, NewRoundVotes)
			else v
		)}
	}

	pure def addVote(c: Consensus, v: Vote) : Consensus = {
		val out = c.ensureRound(v.round())
		{...out,
		votes: out.votes.setBy(v.round(), (orig => orig.addRoundVote(v)))}
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

	action deliverAll(id) = all {
		val p = allMessages.fold(processes.get(id), (out, m) =>
			out.addVote(m).stepLogic())
		processes' = processes.put(id, p)
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

	run precommitExitTest =
		init
		.then(consensusStep(1))
		.then(consensusStep(2))
		.then(consensusStep(3))
		// Deliver prevotes to 1 and 2 only
		.then(deliver(2, processes.get(1).messages[0]))
		.then(deliver(1, processes.get(2).messages[0]))
		.then(deliver(1, processes.get(3).messages[0]))
		.then(deliver(2, processes.get(3).messages[0]))
		// Deliver precommits from 1 and 2
		.then(deliver(2, processes.get(1).messages[1]))
		.then(deliver(3, processes.get(1).messages[1]))
		.then(deliver(1, processes.get(2).messages[1]))
		.then(deliver(3, processes.get(2).messages[1]))
		// Deliver precommit from Byzantine 4 to 3
		.then(deliver(3, (0, Precommit, 4)))
		.then(all{
			assert(processes.get(3).timeout),
			processes' = processes,
		})
		.then(consensusStep(3))
		.then(all{
			assert(processes.get(3).timeout == false),
			assert(processes.get(3).round == 1),
			processes' = processes,
		})
		// Other processes should be able to proceed to round 1
		// NOTE: but this does not happen
		.then(deliverAll(1))
		.then(deliverAll(2))
		.then(all{
			assert(processes.get(1).round == 1),
			assert(processes.get(2).round == 1),
			processes' = processes,
		})

}
