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
	pure def step(v: Vote) : Step = v._2
	pure def sender(v: Vote) : Address = v._3

	type RoundVotes = {
		prevotes: Set[Address],
		precommits: Set[Address],
	}

	pure def addRoundVote(r: RoundVotes, v: Vote) : RoundVotes = {
		val new = Set(v.sender())
		match v.step() {
			| Propose => r // Should not happen
			| Prevote => {...r, prevotes: r.prevotes.union(new)}
			| Precommit => {...r, precommits: r.precommits.union(new)}
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

	pure def addVote(c: Consensus, v: Vote) : Consensus = {
		// Ensure that RoundVotes exists for
		val votes = 0.to(v.round()).fold(c.votes, (v, r) =>
			if (v.keys().contains(r))
				v
			else
				v.put(r, {prevotes: Set(), precommits: Set()})
		)
		{...c,
		votes: votes.setBy(v.round(), (orig => orig.addRoundVote(v)))}
	}

	pure def proposeStep(c: Consensus) : Consensus = {
		val prevote = (c.round, Prevote, c.id)
		{...c.addVote(prevote),
		messages: c.messages.append(prevote),
		step: Prevote}
	}

	pure def prevoteStep(c: Consensus) : Consensus = {
		val numVotes = c.votes.get(c.round).prevotes.size()
		if (numVotes < quorum)
			c
		else
			val precommit = (c.round, Precommit, c.id)
			{...c.addVote(precommit),
			messages: c.messages.append(precommit),
			step: Precommit}
	}

	pure def precommitStep(c: Consensus) : Consensus = {
		val numVotes = c.votes.get(c.round).precommits.size()
		if (numVotes < quorum)
			c
		else
			{...c,
			step: Propose,
			round: c.round + 1}
	}

	pure def stepLogic(c: Consensus) : Consensus = {
		match c.step {
			| Propose => c.proposeStep()
			| Prevote => c.prevoteStep()
			| Precommit => c.precommitStep()
		}
	}

}
