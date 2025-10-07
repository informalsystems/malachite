// -*- mode: Bluespec; -*-


module tendermint5f_algorithm {
  // ============================================================================
  // 1. BASIC TYPES - Network and process identifiers
  // ============================================================================
  
  type ProcessID = str
  type Height = int
  type Round = int
  
  type Value = 
    | ValueStr(str)
    | Nil
  
  type ValueID = Val(Value)

  type Option[a] =
    | Some(a)
    | None

  // ============================================================================
  // 2. DOMAIN-SPECIFIC TYPES - Consensus messages and structures
  // ============================================================================
  
  type PrevoteMessage = {
    sender: ProcessID,
    height: Height,
    round: Round,
    valueId: ValueID
  }

  type ProposalMessage = {
    sender: ProcessID,
    height: Height,
    round: Round,
    value: Value,
    justification: Set[PrevoteMessage]
  }

  type DecisionMessage = {
    sender: ProcessID,
    height: Height,
    round: Round,
    valueId: ValueID, // TODO: this changed to a Value in the English spec
    justification: Set[PrevoteMessage]
  }

  type Message =
    | ProposalMsg(ProposalMessage)
    | PrevoteMsg(PrevoteMessage)
    | DecisionMsg(DecisionMessage)

  type Step = 
    | Propose 
    | PrePropose
    | Prevote

  // ============================================================================
  // 3. LOCAL STATE - What each process maintains
  // ============================================================================
  
  type LocalState = {
    processId: ProcessID,
    height: Height,
    round: Round,
    step: Step,
    decisions: Height -> ValueID,
    max_rounds: ProcessID -> Round,
    prevotedValue: Value,
    prevotedProposalMsg: Option[ProposalMessage],
    roundCertificate: Set[PrevoteMessage]
  }

  // ============================================================================
  // 4. CONSENSUS INPUTS/OUTPUTS - Interface with environment
  // ============================================================================
  
  type ConsensusInput =
    | StartInput
    | LeaderProposeWithLockInput((Value, Set[PrevoteMessage], Round))
    | LeaderProposeWithoutLockInput(Set[PrevoteMessage])
    | FollowerReceiveProposalInput(ProposalMessage)
    | EnoughPrevotesForRoundInput
    | CanDecideInput((ProposalMessage, Set[PrevoteMessage]))
    | ReceiveDecisionInput((DecisionMessage, ProposalMessage))
    | UpdateMaxRoundsInput((ProcessID, PrevoteMessage))
    | CanSkipRoundInput(Set[PrevoteMessage])
    | TimeoutProposeInput((Height, Round))
    | TimeoutPrevoteInput((Height, Round, Set[PrevoteMessage]))
    | TimeoutRebroadcastInput
    | NewHeightInput

  type TimeoutType =
    | ProposeTimeout
    | PrevoteTimeout
    | RebroadcastTimeout

  type ActiveTimeout = {
    height: Height,
    round: Round,
    timeoutType: TimeoutType
  }

  type ConsensusOutput =
    | BroadcastProposal(ProposalMessage)
    | BroadcastPrevote(PrevoteMessage)
    | ReliableBroadcastDecision(DecisionMessage)
    | ScheduleTimeout(ActiveTimeout)
    | GetValueBroadcastProposal(Set[PrevoteMessage])
    | Decide(ValueID)
    | NoOutput

  type Result = {
    output: Set[ConsensusOutput],
    post: LocalState
  }

  // ============================================================================
  // 5. CONSTANTS - System parameters
  // ============================================================================
  
  pure val F = 1
  pure val N = 5 * F + 1
  pure val QUORUM_SIZE = 4 * F + 1
  pure val WEAK_QUORUM_SIZE = F + 1
  pure val LOCK_QUORUM_SIZE = 2 * F + 1
  pure val CORRECT_IN_QUORUM = 3 * F + 1

  pure val INITIAL_HEIGHT = 0
  pure val INITIAL_ROUND = 0



  // ============================================================================
  // 6. PURE FUNCTIONS - All consensus logic
  // ============================================================================
  
  // ---------- Helper Functions ----------
  
  /// Deterministic proposer selection (round-robin)
  pure def proposer(height: Height, round: Round, allProcesses: List[ProcessID]): ProcessID = {
    val index = (height + round) % N
    allProcesses[index]
  }

  /// Get value ID from a value
  pure def valueId(value: Value): ValueID = Val(value)

  /// Compute max_round: highest round with at least 4f+1 prevotes
  pure def computeMaxRound(max_rounds: ProcessID -> Round, allProcesses: List[ProcessID]): Round = {
    val maxPossible = allProcesses.foldl(0, (acc, p) =>
      val r = max_rounds.get(p)
      if (r > acc) r else acc
    )
    0.to(maxPossible).fold(0, (best, r) =>
      val count = allProcesses.foldl(0, (cnt, p) => if (max_rounds.get(p) >= r) cnt + 1 else cnt)
      if (count >= QUORUM_SIZE and r > best) r else best
    )
  }

  /// Compute max_round+: highest round with at least f+1 prevotes
  pure def computeMaxRoundPlus(max_rounds: ProcessID -> Round, allProcesses: List[ProcessID]): Round = {
    val maxPossible = allProcesses.foldl(0, (acc, p) =>
      val r = max_rounds.get(p)
      if (r > acc) r else acc
    )
    0.to(maxPossible).fold(0, (best, r) =>
      val count = allProcesses.foldl(0, (cnt, p) => if (max_rounds.get(p) >= r) cnt + 1 else cnt)
      if (count >= WEAK_QUORUM_SIZE and r > best) r else best
    )
  }

  // ---------- SafeProposal Validation ----------
  
  /// Check if a proposal has valid justification (SafeProposal function)
  pure def safeProposal(proposal: ProposalMessage, state: LocalState): bool = {
    val justification = proposal.justification

    val hasLockedValue = justification.exists(pv1 =>
      val votesForValue = justification.filter(pv2 =>
        pv2.valueId == pv1.valueId and pv2.round >= state.round - 1 and pv2.valueId != Val(Nil)
      )
      votesForValue.size() >= LOCK_QUORUM_SIZE
    )

    if (hasLockedValue)
      justification.exists(pv =>
        val votesForValue = justification.filter(pv2 =>
          pv2.valueId == pv.valueId and pv2.round >= state.round - 1
        )
        votesForValue.size() >= LOCK_QUORUM_SIZE and pv.valueId == valueId(proposal.value)
      )
    else
      (justification.size() == QUORUM_SIZE and
       justification.forall(pv => pv.round >= state.round - 1)) or
      (justification.size() == 0 and proposal.round == 0)
  }

  // ---------- Message Construction ----------
  
  /// Create a PROPOSAL message
  pure def createProposal(
    sender: ProcessID,
    height: Height,
    round: Round,
    value: Value,
    justification: Set[PrevoteMessage]
  ): ProposalMessage = {
    { sender: sender,
      height: height,
      round: round,
      value: value,
      justification: justification }
  }

  /// Create a PREVOTE message
  pure def createPrevote(
    sender: ProcessID,
    height: Height,
    round: Round,
    valueId: ValueID
  ): PrevoteMessage = {
    { sender: sender,
      height: height,
      round: round,
      valueId: valueId }
  }

  /// Create a DECISION message
  pure def createDecision(
    sender: ProcessID,
    height: Height,
    round: Round,
    valueId: ValueID,
    justification: Set[PrevoteMessage]
  ): DecisionMessage = {
    { sender: sender,
      height: height,
      round: round,
      valueId: valueId,
      justification: justification }
  }

  // ---------- Core Consensus Logic ----------
  
  /// Helper: Start a new round
  pure def startRound(
    state: LocalState,
    newRound: Round,
    allProcesses: List[ProcessID]
  ): Result = {
    val isProposer = proposer(state.height, newRound, allProcesses) == state.processId
    val baseOutputs = Set(ScheduleTimeout({ height: state.height, round: newRound, timeoutType: ProposeTimeout }))
    val outputs = 
      if (isProposer and newRound == 0) // initial round, proposer can immediately propose
        baseOutputs.union(Set(GetValueBroadcastProposal(Set())))
      else
        baseOutputs

    val newStep = 
      if (not(isProposer))
        Propose
      else if (newRound == 0) // initial round, proposer can immediately propose
        Propose 
      else 
        PrePropose

    { output: outputs,
      post: { ...state, round: newRound, step: newStep } }
  }
  
  /// Process StartInput
  pure def processStartInput(
    state: LocalState,
    allProcesses: List[ProcessID]
  ): Result = {
    val result = startRound(state, 0, allProcesses)
    { output: result.output.union(Set(ScheduleTimeout({ height: state.height, round: 0, timeoutType: RebroadcastTimeout }))),
      post: result.post }
  }
  
  /// Process LeaderProposeWithLockInput
  pure def processLeaderProposeWithLock(
    state: LocalState,
    value: Value,
    justification: Set[PrevoteMessage],
    justificationRound: Round,
    allProcesses: List[ProcessID]
  ): Result = {
    val isProposer = proposer(state.height, state.round, allProcesses) == state.processId
    val hasLock = justification.exists(pv1 =>
      val votesForValue = justification.filter(pv2 => pv2.valueId == pv1.valueId)
      votesForValue.size() >= LOCK_QUORUM_SIZE and pv1.valueId == valueId(value) and pv1.valueId != Val(Nil)
    )
    val canPropose = isProposer and 
                     state.step == PrePropose and
                     justificationRound >= state.round - 1 and
                     justification.size() == QUORUM_SIZE and
                     hasLock


    if (canPropose)
      val proposal = createProposal(state.processId, state.height, state.round, value, justification)
      { output: Set(BroadcastProposal(proposal)),
        post: { ...state, step: Propose } }
    else
      { output: Set(), post: state }
  }

  /// Process LeaderProposeWithoutLockInput
  pure def processLeaderProposeWithoutLock(
    state: LocalState,
    justification: Set[PrevoteMessage],
    allProcesses: List[ProcessID]
  ): Result = {
    val isProposer = proposer(state.height, state.round, allProcesses) == state.processId
    val hasLock = justification.exists(pv1 =>
        val votesForValue = justification.filter(pv2 => pv2.valueId == pv1.valueId)
        votesForValue.size() >= LOCK_QUORUM_SIZE and pv1.valueId != Val(Nil)
      )
    val canPropose = isProposer and 
                     state.step == PrePropose and
                     justification.size() == QUORUM_SIZE and
                     not(hasLock)

    if (canPropose)
      { output: Set(GetValueBroadcastProposal(justification)),
        post: { ...state, step: Propose } }
    else
      { output: Set(), post: state }
  }
  
  /// Process UpdateMaxRoundsInput
  pure def processUpdateMaxRounds(
    state: LocalState,
    sender: ProcessID,
    prevoteMsg: PrevoteMessage,
    allProcesses: List[ProcessID]
  ): Result = {
    val currentMaxRound = state.max_rounds.get(sender)
    val shouldUpdate = prevoteMsg.height == state.height and prevoteMsg.round > currentMaxRound

    if (shouldUpdate)
      val newMaxRounds = state.max_rounds.set(sender, prevoteMsg.round)
      { output: Set(), post: { ...state, max_rounds: newMaxRounds } }
    else
      { output: Set(), post: state }
  }

  /// Process EnoughPrevotesForRoundInput
  pure def processEnoughPrevotesForRound(
    state: LocalState,
    allProcesses: List[ProcessID]
  ): Result = {
    val maxRound = computeMaxRound(state.max_rounds, allProcesses)
    val shouldSchedule = maxRound == state.round

    if (shouldSchedule)
      { output: Set(ScheduleTimeout({ height: state.height, round: state.round, timeoutType: PrevoteTimeout })),
        post: state }
    else
      { output: Set(), post: state }
  }

  /// Process CanSkipRoundInput
  pure def processCanSkipRound(
    state: LocalState,
    roundCertificate: Set[PrevoteMessage],
    allProcesses: List[ProcessID]
  ): Result = {
    val maxRoundPlus = computeMaxRoundPlus(state.max_rounds, allProcesses)
    val canSkip = maxRoundPlus > state.round and roundCertificate.size() >= WEAK_QUORUM_SIZE

    if (canSkip)
      val result = startRound(state, maxRoundPlus, allProcesses)
      { output: result.output,
        post: { ...result.post, roundCertificate: roundCertificate } }
    else
      { output: Set(), post: state }
  }
  
  /// Process CanDecideInput
  pure def processCanDecide(
    state: LocalState,
    proposal: ProposalMessage,
    justification: Set[PrevoteMessage]
  ): Result = {
    val hasDecided = state.decisions.keys().contains(state.height)
    val senders = justification.map(pv => pv.sender)
    val hasQuorum = senders.size() >= QUORUM_SIZE
    val allForValue = justification.forall(pv =>
      pv.height == proposal.height and pv.round == proposal.round and pv.valueId == valueId(proposal.value)
    )
    val nonNilValue = proposal.value != Nil
    val canDecide = not(hasDecided) and hasQuorum and allForValue and nonNilValue

    if (canDecide)
      val decision = createDecision(state.processId, state.height, proposal.round, valueId(proposal.value), justification)
      val newDecisions = state.decisions.put(state.height, valueId(proposal.value))

      { output: Set(
          ReliableBroadcastDecision(decision),
          Decide(valueId(proposal.value)) ),
        post: { ...state, decisions: newDecisions } }
    else
      { output: Set(), post: state }
  }

  /// Process ReceiveDecisionInput
  pure def processReceiveDecision(
    state: LocalState,
    decision: DecisionMessage,
    proposal: ProposalMessage // TODO: is this needed? it disappeared in the English spec
  ): Result = {
    val hasDecided = state.decisions.keys().contains(state.height)
    val valuesMatch = decision.valueId == valueId(proposal.value) and decision.height == proposal.height
    val senders = decision.justification.map(pv => pv.sender)
    val hasQuorum = senders.size() >= QUORUM_SIZE
    val allForValue = decision.justification.forall(pv =>
      pv.height == proposal.height and pv.round == proposal.round and pv.valueId == valueId(proposal.value)
    )
    val nonNilValue = proposal.value != Nil

    val canDecide = not(hasDecided) and valuesMatch and hasQuorum and allForValue and nonNilValue

    if (canDecide)
      val newDecisions = state.decisions.put(state.height, decision.valueId)
      { output: Set(Decide(decision.valueId)),
        post: { ...state, decisions: newDecisions } }
    else
      { output: Set(), post: state }
  }

  /// Process NewHeightInput
  pure def processNewHeight(
    state: LocalState,
    allProcesses: List[ProcessID]
  ): Result = {
    val newHeight = state.height + 1

    { output: Set(),
      post: { ...state,
        height: newHeight,
        round: 0,
        step: Propose,
        max_rounds: allProcesses.foldl(Map(), (acc, proc) => acc.put(proc, INITIAL_ROUND)), // TODO check if is complete. This seems to have been added in English spec. Also initialize to -1?
        prevotedValue: Nil,
        prevotedProposalMsg: None,
        roundCertificate: Set() } }
  }

  /// Process TimeoutProposeInput
  pure def processTimeoutPropose(
    state: LocalState,
    height: Height,
    round: Round
  ): Result = {
    val shouldTimeout = height == state.height and
                       round == state.round and
                       state.step == Propose

    if (shouldTimeout)
      val prevoteMsg = createPrevote(state.processId, state.height, state.round, valueId(state.prevotedValue))
      { output: Set(BroadcastPrevote(prevoteMsg)), post: { ...state, step: Prevote } }
    else
      { output: Set(), post: state }
  }

  /// Process TimeoutPrevoteInput
  pure def processTimeoutPrevote(
    state: LocalState,
    height: Height,
    round: Round,
    roundCertificate: Set[PrevoteMessage],
    allProcesses: List[ProcessID]
  ): Result = {
    val shouldTimeout = height == state.height and round == state.round

    if (shouldTimeout)
      val result = startRound(state, state.round + 1, allProcesses)
      { output: result.output,
        post: { ...result.post, roundCertificate: roundCertificate } }
    else
      { output: Set(), post: state }
  }

  /// Process TimeoutRebroadcastInput
  pure def processTimeoutRebroadcast(state: LocalState): Result = {
    val certificateOutputs = state.roundCertificate.map(pv =>
      BroadcastPrevote(pv)
    )
    
    val proposalOutputs = match state.prevotedProposalMsg {
      | Some(proposal) => Set(BroadcastProposal(proposal))
      | None => Set()
    }
    
    val allOutputs = certificateOutputs.union(proposalOutputs).union(
      Set(ScheduleTimeout({ height: state.height, round: state.round, timeoutType: RebroadcastTimeout }))
    )

    { output: allOutputs, post: state }
  }

  /// Process FollowerReceiveProposalInput
  pure def processFollowerReceiveProposal(
    state: LocalState,
    proposal: ProposalMessage
  ): Result = {
    val canProcess = state.step == Propose and
                    proposal.height == state.height and
                    proposal.round == state.round

    if (canProcess)
      val isSafe = safeProposal(proposal, state)

      if (isSafe)
        val proposalWithoutJustification = createProposal(state.processId, proposal.height, proposal.round, proposal.value, Set())
        val prevoteMsg = createPrevote(state.processId, state.height, state.round, valueId(proposal.value))

        { output: Set(BroadcastPrevote(prevoteMsg)),
          post: { ...state,
            step: Prevote,
            prevotedValue: proposal.value,
            prevotedProposalMsg: Some(proposalWithoutJustification) } }
      else
        val prevoteMsg = createPrevote(state.processId, state.height, state.round, valueId(state.prevotedValue))

        { output: Set(BroadcastPrevote(prevoteMsg)),
          post: { ...state, step: Prevote } }
    else
      { output: Set(), post: state }
  }
  
/// Main consensus step - dispatches to specific handlers
  pure def processConsensusInput(
    state: LocalState,
    input: ConsensusInput,
    allProcesses: List[ProcessID]
  ): Result = {
    match input {
      | StartInput => processStartInput(state, allProcesses)
      | LeaderProposeWithLockInput(params) =>
        val value = params._1
        val justification = params._2
        val justificationRound = params._3
        processLeaderProposeWithLock(state, value, justification, justificationRound, allProcesses)
      | LeaderProposeWithoutLockInput(justification) =>
        processLeaderProposeWithoutLock(state, justification, allProcesses)
      | FollowerReceiveProposalInput(proposal) =>
        processFollowerReceiveProposal(state, proposal)
      | UpdateMaxRoundsInput(params) =>
        val sender = params._1
        val prevoteMsg = params._2
        processUpdateMaxRounds(state, sender, prevoteMsg, allProcesses)
      | EnoughPrevotesForRoundInput =>
        processEnoughPrevotesForRound(state, allProcesses)
      | CanSkipRoundInput(roundCertificate) =>
        processCanSkipRound(state, roundCertificate, allProcesses)
      | CanDecideInput(params) =>
        val proposal = params._1
        val justification = params._2
        processCanDecide(state, proposal, justification)
      | ReceiveDecisionInput(params) =>
        val decision = params._1
        val proposal = params._2
        processReceiveDecision(state, decision, proposal)
      | NewHeightInput =>
        processNewHeight(state, allProcesses)
      | TimeoutProposeInput(params) =>
        val height = params._1
        val round = params._2
        processTimeoutPropose(state, height, round)
      | TimeoutPrevoteInput(params) =>
        val height = params._1
        val round = params._2
        val roundCertificate = params._3
        processTimeoutPrevote(state, height, round, roundCertificate, allProcesses)
      | TimeoutRebroadcastInput =>
        processTimeoutRebroadcast(state)


    }
  }  







}