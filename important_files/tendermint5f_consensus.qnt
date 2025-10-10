// -*- mode: Bluespec; -*-

module tendermint5f_consensus {
  import tendermint5f_algorithm.* from "./tendermint5f_algorithm"
  export tendermint5f_algorithm.*

  // ============================================================================
  // 1. GLOBAL ENVIRONMENT - Distributed system state
  // ============================================================================

  type Environment = {
    allProcesses: List[ProcessID],
    processes: ProcessID -> LocalState,
    messageBuffer: Set[Message],
    reliableBroadcastBuffer: Set[Message],
    activeTimeouts: ProcessID -> Set[ActiveTimeout],
    pendingValueRequests: ProcessID -> Option[Set[PrevoteMessage]]
  }
  
  // ============================================================================
  // 2. PROCESS SETS - Fault model
  // ============================================================================
  


  pure val correctProcesses: List[ProcessID] = ["p1", "p2", "p3", "p4", "p5"]
  pure val byzantineProcesses: List[ProcessID] = ["b1"]
  
  // ============================================================================
  // 3. SYSTEM PARAMETERS
  // ============================================================================
  
  pure val PROPOSAL_VALUES_PER_PROCESS: ProcessID -> Set[Value] =
    Map(
      "p1" -> Set(ValueStr("v1"), ValueStr("v2")),
      "p2" -> Set(ValueStr("v3"), ValueStr("v4")),
      "p3" -> Set(ValueStr("v5"), ValueStr("v6")),
      "p4" -> Set(ValueStr("v7"), ValueStr("v8")),
      "p5" -> Set(ValueStr("v9"), ValueStr("v10"))
    )

  pure val BYZANTINE_MAX_HEIGHT = 0
  pure val BYZANTINE_MAX_ROUND = 2
  pure val BYZANTINE_VALUES = Set(ValueStr("byz1"), ValueStr("byz2"), Nil)

  // All values that correct processes can propose
  pure val ALL_CORRECT_VALUES = PROPOSAL_VALUES_PER_PROCESS.keys().fold(Set(), (acc, p) =>
    acc.union(PROPOSAL_VALUES_PER_PROCESS.get(p))
  )

  // Values Byzantine nodes use for prevotes (includes correct values for equivocation)
  pure val BYZANTINE_PREVOTE_VALUES = BYZANTINE_VALUES.union(ALL_CORRECT_VALUES)

  // ============================================================================
  // 4. DERIVED VALUES
  // ============================================================================

  pure val honestProcesses = correctProcesses

  pure val honestProcessesSet = honestProcesses.foldl(Set(), (acc, p) => acc.union(Set(p)))

  // ============================================================================
  // 5. FAULT TOLERANCE ASSUMPTIONS
  // ============================================================================

  pure val correctProcessesSet = correctProcesses.foldl(Set(), (acc, p) => acc.union(Set(p)))
  pure val byzantineProcessesSet = byzantineProcesses.foldl(Set(), (acc, p) => acc.union(Set(p)))
  pure val allProcessesSet = correctProcessesSet.union(byzantineProcessesSet)

  assume faultTolerance = all {
    correctProcessesSet.intersect(byzantineProcessesSet) == Set(),
    correctProcesses.length() + byzantineProcesses.length() == N,
    byzantineProcesses.length() <= F,
    correctProcesses.length() + byzantineProcesses.length() == 5 * byzantineProcesses.length() + 1
  }
  
  // ============================================================================
  // 6. ENVIRONMENT UPDATES - How consensus affects global state
  // ============================================================================
  
  /// Check if a set of messages has a quorum of distinct senders
  pure def hasQuorum(messages: Set[Message], quorumSize: int): bool = {
    val senders = messages.fold(Set(), (acc, msg) =>
      match msg {
        | ProposalMsg(p) => acc.union(Set(p.sender))
        | PrevoteMsg(pv) => acc.union(Set(pv.sender))
        | DecisionMsg(d) => acc.union(Set(d.sender))
      }
    )
    senders.size() >= quorumSize
  }
  
  /// Find a locked value (quorum of distinct senders voting for same value)
  pure def findLockedValue(
    messages: Set[Message],
    quorumSize: int
  ): Option[ValueID] = {
    val valueIds = messages.fold(Set(), (acc, msg) =>
      match msg {
        | PrevoteMsg(pv) => acc.union(Set(pv.valueId))
        | _ => acc
      }
    )
    
    valueIds.fold(None, (acc, vid) =>
      match acc {
        | Some(_) => acc
        | None =>
          val messagesForValue = messages.filter(msg =>
            match msg {
              | PrevoteMsg(pv) => pv.valueId == vid
              | _ => false
            }
          )
          if (hasQuorum(messagesForValue, quorumSize))
            Some(vid)
          else
            None
      }
    )
  }
  
  /// Apply consensus result to environment
  pure def applyConsensusResult(
    env: Environment,
    processId: ProcessID,
    result: Result
  ): Environment = {
    val updatedProcesses = env.processes.set(processId, result.post)
    
    result.output.fold(
      { ...env, processes: updatedProcesses },
      (newEnv, output) => match output {
        | BroadcastProposal(msg) =>
          { ...newEnv, messageBuffer: newEnv.messageBuffer.union(Set(ProposalMsg(msg))) }

        | BroadcastPrevote(msg) =>
          { ...newEnv, messageBuffer: newEnv.messageBuffer.union(Set(PrevoteMsg(msg))) }

        | ReliableBroadcastDecision(msg) =>
          { ...newEnv, reliableBroadcastBuffer: newEnv.reliableBroadcastBuffer.union(Set(DecisionMsg(msg))) }

        | ScheduleTimeout(timeout) =>
          val currentTimeouts = newEnv.activeTimeouts.get(processId)
          { ...newEnv, activeTimeouts: newEnv.activeTimeouts.set(processId, currentTimeouts.union(Set(timeout))) }

        | GetValueBroadcastProposal(justification) =>
          { ...newEnv, pendingValueRequests: newEnv.pendingValueRequests.set(processId, Some(justification)) }

        | Decide(valueId) => newEnv

        | NoOutput => newEnv
      }
    )
  }
  
  // ============================================================================
  // 7. STATE VARIABLES
  // ============================================================================
  
  var environment: Environment
  
  // ============================================================================
  // 8. INVARIANTS - Consensus properties
  // ============================================================================
  
  /// Helper: Check if two processes agree on decision for a given height
  pure def agreeInHeight(
    env: Environment,
    p1: ProcessID,
    p2: ProcessID,
    height: Height
  ): bool = {
    val dec1 = env.processes.get(p1).decisions.get(height)
    val dec2 = env.processes.get(p2).decisions.get(height)
    dec1 == dec2
  }
  
  /// Safety/Agreement: No two correct processes decide different values for same height
  val safety = 
    correctProcessesSet.forall(p1 =>
      correctProcessesSet.forall(p2 =>
        val heights1 = environment.processes.get(p1).decisions.keys()
        val heights2 = environment.processes.get(p2).decisions.keys()
        val commonHeights = heights1.intersect(heights2)
        commonHeights.forall(h => agreeInHeight(environment, p1, p2, h))
      )
    )
  
  /// Integrity: A correct process does not decide Nil
  val integrity = 
    correctProcessesSet.forall(p =>
      environment.processes.get(p).decisions.keys().forall(h =>
          environment.processes.get(p).decisions.get(h) != Val(Nil)
    ))
  
  /// Validity: If a correct process decides v, then v was proposed by some process
  val validity = correctProcessesSet.forall(p =>
    environment.processes.get(p).decisions.keys().forall(h =>
      environment.messageBuffer.exists(msg =>
        match msg {
          | ProposalMsg(prop) => Val(prop.value) == environment.processes.get(p).decisions.get(h) and prop.height == h
          | _ => false
        }
  
    )))
  
  // ============================================================================
  // 9. WITNESSES - Interesting scenarios to capture
  // ============================================================================
  
  /// Someone has decided for height 0
  val someoneDecidedHeight0 = 
    not(honestProcessesSet.exists(p =>
      environment.processes.get(p).decisions.keys().contains(INITIAL_HEIGHT)
    ))

  /// Someone has decided for height 0
  val someoneDecidedHeight1 = 
    not(honestProcessesSet.exists(p =>
      environment.processes.get(p).decisions.keys().contains(1)
    ))

  /// Everyone has decided on height 0
  val everyoneDecidedHeight0 = 
    not(honestProcessesSet.forall(p => 
      environment.processes.get(p).decisions.keys().contains(INITIAL_HEIGHT)
    ))
  
  /// Multiple rounds have been executed
  val multipleRounds = 
    not(honestProcessesSet.exists(p =>
      environment.processes.get(p).round > 0
    ))
  
  /// Multiple rounds have been executed
  val multipleHeights = 
    not(honestProcessesSet.exists(p =>
      environment.processes.get(p).height > 0
    )) 
  
  /// A process has prevoted Nil
  val someonePrevotedNil = 
    not(environment.messageBuffer.exists(msg =>
      match msg {
        | PrevoteMsg(pv) => pv.valueId == Val(Nil) and correctProcessesSet.contains(pv.sender)
        | _ => false
      }
    ))
  
  /// A decision has been broadcast
  val decisionBroadcast =
    not(environment.reliableBroadcastBuffer.size() > 0)
  
  val decisionButMovedOn =
    not(honestProcessesSet.exists(p =>
          honestProcessesSet.exists(q => and {
            environment.processes.get(p).decisions.keys().contains(INITIAL_HEIGHT),
            not(environment.processes.get(q).decisions.keys().contains(INITIAL_HEIGHT)),
            environment.processes.get(q).height > INITIAL_HEIGHT
            }
    )))

  // ============================================================================
  // 10. ACTIONS - System behaviors
  // ============================================================================
  
  action unchanged_all = 
    environment' = environment
  
  /// Process starts consensus (height 0, round 0)
  action processStart(processId: ProcessID): bool = {
    val currentState = environment.processes.get(processId)
    val result = processConsensusInput(currentState, StartInput, environment.allProcesses)
    
    all {
      currentState.height == INITIAL_HEIGHT,
      currentState.round == INITIAL_ROUND,
      currentState.step == Propose,
      environment' = applyConsensusResult(environment, processId, result)
    }
  }

  action FollowerReceiveProposalSpecific(processId: ProcessID, proposal: ProposalMessage): bool = {
    val currentState = environment.processes.get(processId)
    
    all {
      currentState.step == Propose,
      proposal.height == currentState.height,
      proposal.round == currentState.round,
      proposal.sender == proposer(currentState.height, currentState.round, environment.allProcesses),
      environment.messageBuffer.contains(ProposalMsg(proposal)),
      val result = processConsensusInput(
        currentState,
        FollowerReceiveProposalInput(proposal),
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }

  /// Follower receives and processes a proposal
  action followerReceiveProposal(processId: ProcessID, messageBuffer: Set[Message]): bool = {
    val currentState = environment.processes.get(processId)

    val validProposals = messageBuffer.filter(msg =>
      match msg {
        | ProposalMsg(p) => all {
          p.height == currentState.height,
          p.round == currentState.round,
          p.sender == proposer(currentState.height, currentState.round, environment.allProcesses)
        }
        | _ => false
      }
    )
    
    all {
      currentState.step == Propose,
      validProposals.size() > 0,
      nondet msg = validProposals.oneOf()
      val proposalMsg = match msg {
        | ProposalMsg(p) => p
        | _ => { sender: "", height: 0, round: 0, value: Nil, justification: Set() }  // unreachable due to filter
      }
    FollowerReceiveProposalSpecific(processId, proposalMsg)
    }
  }
  
  action UpdateMaxRoundsSpecific(processId: ProcessID, prevote: PrevoteMessage): bool = {
    val currentState = environment.processes.get(processId)

    all {
      prevote.height == currentState.height,
      prevote.round > currentState.max_rounds.get(prevote.sender),
      environment.messageBuffer.contains(PrevoteMsg(prevote)),
      val result = processConsensusInput(
        currentState,
        UpdateMaxRoundsInput((prevote.sender, prevote)),
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }

  /// Process receives and records a prevote message
  action updateMaxRounds(processId: ProcessID, messageBuffer: Set[Message]): bool = {
    val currentState = environment.processes.get(processId)

    val validPrevotes = messageBuffer.filter(msg =>
      match msg {
        | PrevoteMsg(pv) => all {
          pv.height == currentState.height,
          pv.round > currentState.max_rounds.get(pv.sender)
        }
        | _ => false
      }
    )

    all {
      validPrevotes.size() > 0,
      nondet msg = validPrevotes.oneOf()
      val prevoteMsg = match msg {
        | PrevoteMsg(pv) => pv
        | _ => { sender: "", height: 0, round: 0, valueId: Val(Nil) }  // unreachable due to filter
      }
      UpdateMaxRoundsSpecific(processId, prevoteMsg)
    }
  }
  
  action LeaderProposeWithLockSpecific(processId: ProcessID, proposal: ProposalMessage, prevoteMessages: Set[PrevoteMessage]): bool = {
    val currentState = environment.processes.get(processId)
    val isProposer = proposer(currentState.height, currentState.round, environment.allProcesses) == processId
    val justificationRound = prevoteMessages.fold(0, (maxR, pv) =>
      if (pv.round > maxR) pv.round else maxR
    )

    all {
      isProposer,
      currentState.step == PrePropose,
      proposal.height == currentState.height,
      environment.messageBuffer.contains(ProposalMsg(proposal)),
      val result = processConsensusInput(
        currentState,
        LeaderProposeWithLockInput((proposal.value, prevoteMessages, justificationRound)),
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }

  /// Leader proposes with a locked value from justification
  action leaderProposeWithLock(processId: ProcessID, messageBuffer: Set[Message]): bool = {
    val currentState = environment.processes.get(processId)
    val isProposer = proposer(currentState.height, currentState.round, environment.allProcesses) == processId

    val relevantPrevotes = messageBuffer.filter(msg =>
      match msg {
        | PrevoteMsg(pv) => all {
          pv.height == currentState.height,
          pv.round >= currentState.round - 1
        }
        | _ => false
      }
    )

    val prevoteMessages = relevantPrevotes.fold(Set(), (acc, msg) =>
      match msg {
        | PrevoteMsg(pv) => acc.union(Set(pv))
        | _ => acc
      }
    )

    val lockedValue = findLockedValue(relevantPrevotes, LOCK_QUORUM_SIZE)

    val proposalForValue = match lockedValue {
      | Some(valueId) =>
        match valueId {
          | Val(v) =>
            messageBuffer.filter(msg =>
              match msg {
                | ProposalMsg(p) => p.value == v and p.height == currentState.height
                | _ => false
              }
            )
        }
      | None => Set()
    }

    all {
      isProposer,
      currentState.step == PrePropose,
      hasQuorum(relevantPrevotes, QUORUM_SIZE),
      match lockedValue { | Some(_) => true | None => false },
      proposalForValue.size() == 1,
      nondet propMsg = proposalForValue.oneOf()
      val proposalMsg = match propMsg {
        | ProposalMsg(p) => p
        | _ => { sender: "", height: 0, round: 0, value: Nil, justification: Set() }  // unreachable
      }
      LeaderProposeWithLockSpecific(processId, proposalMsg, prevoteMessages)
    }
  }
  
  action LeaderProposeWithoutLockSpecific(processId: ProcessID, prevoteMessages: Set[PrevoteMessage]): bool = {
    val currentState = environment.processes.get(processId)
    val isProposer = proposer(currentState.height, currentState.round, environment.allProcesses) == processId

    all {
      isProposer,
      currentState.step == PrePropose,
      val result = processConsensusInput(
        currentState,
        LeaderProposeWithoutLockInput(prevoteMessages),
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }

  /// Leader proposes without a locked value
  action leaderProposeWithoutLock(processId: ProcessID, messageBuffer: Set[Message]): bool = {
    val currentState = environment.processes.get(processId)
    val isProposer = proposer(currentState.height, currentState.round, environment.allProcesses) == processId

    val relevantPrevotes = messageBuffer.filter(msg =>
      match msg {
        | PrevoteMsg(pv) => all {
          pv.height == currentState.height,
          pv.round >= currentState.round - 1
        }
        | _ => false
      }
    )
    
    val prevoteMessages = relevantPrevotes.fold(Set(), (acc, msg) => 
      match msg {
        | PrevoteMsg(pv) => acc.union(Set(pv))
        | _ => acc
      }
    )
    
    val lockedValue = findLockedValue(relevantPrevotes, LOCK_QUORUM_SIZE)
    
    all {
      isProposer,
      currentState.step == PrePropose, // TODO: check whether we should do the check here. It will anyways be done in the function call
      hasQuorum(relevantPrevotes, QUORUM_SIZE),
      match lockedValue { | None => true | Some(_) => false },
      val result = processConsensusInput(
        currentState,
        LeaderProposeWithoutLockInput(prevoteMessages),
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }
  
  /// Process schedules prevote timeout when enough prevotes collected
  action enoughPrevotesForRound(processId: ProcessID): bool = {
    val currentState = environment.processes.get(processId)
    val maxRound = computeMaxRound(currentState.max_rounds, environment.allProcesses)
    
    all {
      maxRound == currentState.round,
      val result = processConsensusInput(
        currentState,
        EnoughPrevotesForRoundInput,
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }
  
  action CanSkipRoundSpecific(processId: ProcessID, prevoteMessages: Set[PrevoteMessage]): bool = {
    val currentState = environment.processes.get(processId)

    all {
      val result = processConsensusInput(
        currentState,
        CanSkipRoundInput(prevoteMessages),
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }

  /// Process skips to higher round when max_round+ > current round
  action canSkipRound(processId: ProcessID, messageBuffer: Set[Message]): bool = {
    val currentState = environment.processes.get(processId)
    val maxRoundPlus = computeMaxRoundPlus(currentState.max_rounds, environment.allProcesses)

    val roundCertificate = messageBuffer.filter(msg =>
      match msg {
        | PrevoteMsg(pv) => all {
          pv.height == currentState.height,
          pv.round >= maxRoundPlus
        }
        | _ => false
      }
    )

    val prevoteMessages = roundCertificate.fold(Set(), (acc, msg) =>
      match msg {
        | PrevoteMsg(pv) => acc.union(Set(pv))
        | _ => acc
      }
    )

    all {
      maxRoundPlus > currentState.round,
      hasQuorum(roundCertificate, WEAK_QUORUM_SIZE),
      CanSkipRoundSpecific(processId, prevoteMessages)
    }
  }
  
  action CanDecideSpecific(processId: ProcessID, proposal: ProposalMessage, prevoteMessages: Set[PrevoteMessage]): bool = {
    val currentState = environment.processes.get(processId)

    all {
      environment.messageBuffer.contains(ProposalMsg(proposal)),
      prevoteMessages.forall(pv => environment.messageBuffer.contains(PrevoteMsg(pv))),
      val result = processConsensusInput(
        currentState,
        CanDecideInput((proposal, prevoteMessages)),
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }

  /// Process can decide when it has proposal + 4f+1 matching prevotes
  action canDecide(processId: ProcessID, messageBuffer: Set[Message]): bool = {
    val currentState = environment.processes.get(processId)

    val proposals = messageBuffer.filter(msg =>
      match msg {
        | ProposalMsg(p) => all {
          p.height == currentState.height,
          p.sender == proposer(currentState.height, p.round, environment.allProcesses)
        }
        | _ => false
      }
    )

    val validProposalWithPrevotes = proposals.filter(propMsg =>
      match propMsg {
        | ProposalMsg(p) => {
          val matchingPrevotes = messageBuffer.filter(msg =>
            match msg {
              | PrevoteMsg(pv) => all {
                pv.height == p.height,
                pv.round == p.round,
                pv.valueId == valueId(p.value)
              }
              | _ => false
            }
          )
          hasQuorum(matchingPrevotes, QUORUM_SIZE)
        }
        | _ => false
      }
    )

    all {
      validProposalWithPrevotes.size() == 1,
      nondet propMsg = validProposalWithPrevotes.oneOf()
      val proposalMsg = match propMsg {
        | ProposalMsg(p) => p
        | _ => { sender: "", height: 0, round: 0, value: Nil, justification: Set() }  // unreachable
      }
      val matchingPrevotes = messageBuffer.filter(msg =>
        match msg {
          | PrevoteMsg(pv) => all {
            pv.height == proposalMsg.height,
            pv.round == proposalMsg.round,
            pv.valueId == valueId(proposalMsg.value)
          }
          | _ => false
        }
      )
      val prevoteMessages = matchingPrevotes.fold(Set(), (acc, msg) =>
        match msg {
          | PrevoteMsg(pv) => acc.union(Set(pv))
          | _ => acc
        }
      )
      CanDecideSpecific(processId, proposalMsg, prevoteMessages)
    }
  }
  
  action ReceiveDecisionSpecific(processId: ProcessID, decision: DecisionMessage, proposal: ProposalMessage): bool = {
    val currentState = environment.processes.get(processId)

    all {
      decision.height == currentState.height,
      environment.reliableBroadcastBuffer.contains(DecisionMsg(decision)),
      proposal.height == decision.height,
      valueId(proposal.value) == decision.valueId,
      environment.messageBuffer.contains(ProposalMsg(proposal)),
      val result = processConsensusInput(
        currentState,
        ReceiveDecisionInput((decision, proposal)),
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }

  /// Process receives decision from another process
  action receiveDecision(processId: ProcessID, messageBuffer: Set[Message]): bool = {
    val currentState = environment.processes.get(processId)

    val decisions = environment.reliableBroadcastBuffer.filter(msg =>
      match msg {
        | DecisionMsg(d) => d.height == currentState.height
        | _ => false
      }
    )

    val validDecisions = decisions.filter(decMsg =>
      match decMsg {
        | DecisionMsg(d) => {
          val matchingProposals = messageBuffer.filter(msg =>
            match msg {
              | ProposalMsg(p) => all {
                p.height == d.height,
                valueId(p.value) == d.valueId,
              }
              | _ => false
            }
          )
          matchingProposals.size() > 0
        }
        | _ => false
      }
    )

    all {
      validDecisions.size() > 0,
      nondet decMsg = validDecisions.oneOf()
      val decisionMsg = match decMsg {
        | DecisionMsg(d) => d
        | _ => { sender: "", height: 0, round: 0, valueId: Val(Nil), justification: Set() }  // unreachable
      }
      nondet matchingProposal = messageBuffer.filter(msg =>
        match msg {
          | ProposalMsg(p) => all {
            p.height == decisionMsg.height,
            valueId(p.value) == decisionMsg.valueId,
          }
          | _ => false
        }
      ).oneOf()
      val proposalMsg = match matchingProposal {
        | ProposalMsg(p) => p
        | _ => { sender: "", height: 0, round: 0, value: Nil, justification: Set() }  // unreachable due to filter
      }
      ReceiveDecisionSpecific(processId, decisionMsg, proposalMsg)
    }
  }
  
  /// Process moves to next height after deciding
  action newHeight(processId: ProcessID): bool = {
    val currentState = environment.processes.get(processId)

    all {
      currentState.decisions.keys().contains(currentState.height),
      val result = processConsensusInput(
        currentState,
        NewHeightInput,
        environment.allProcesses
      )
      environment' = applyConsensusResult(environment, processId, result)
    }
  }
  
  action OnTimeoutProposeSpecific(processId: ProcessID, timeout: ActiveTimeout): bool = {
    val currentState = environment.processes.get(processId)

    all {
      timeout.height == currentState.height,
      timeout.round == currentState.round,
      timeout.timeoutType == ProposeTimeout,
      environment.activeTimeouts.get(processId).contains(timeout),
      val newTimeouts = environment.activeTimeouts.get(processId).exclude(Set(timeout))
      val newActiveTimeouts = environment.activeTimeouts.set(processId, newTimeouts)
      val envWithoutTimeout = { ...environment, activeTimeouts: newActiveTimeouts }
      val result = processConsensusInput(
        currentState,
        TimeoutProposeInput((timeout.height, timeout.round)),
        environment.allProcesses
      )
      environment' = applyConsensusResult(envWithoutTimeout, processId, result)
    }
  }

  /// Timeout fires while waiting for proposal
  action onTimeoutPropose(processId: ProcessID): bool = {
    val currentState = environment.processes.get(processId)

    val matchingTimeouts = environment.activeTimeouts.get(processId).filter(t =>
      t.height == currentState.height and
      t.round == currentState.round and
      t.timeoutType == ProposeTimeout
    )

    all {
      matchingTimeouts.size() == 1,
      val timeout = matchingTimeouts.getOnlyElement()
      OnTimeoutProposeSpecific(processId, timeout)
    }
  }
  
  action OnTimeoutPrevoteSpecific(processId: ProcessID, timeout: ActiveTimeout, roundCertificate: Set[PrevoteMessage]): bool = {
    val currentState = environment.processes.get(processId)

    all {
      timeout.height == currentState.height,
      timeout.round == currentState.round,
      timeout.timeoutType == PrevoteTimeout,
      environment.activeTimeouts.get(processId).contains(timeout),
      val newTimeouts = environment.activeTimeouts.get(processId).exclude(Set(timeout))
      val newActiveTimeouts = environment.activeTimeouts.set(processId, newTimeouts)
      val envWithoutTimeout = { ...environment, activeTimeouts: newActiveTimeouts }
      val result = processConsensusInput(
        currentState,
        TimeoutPrevoteInput((timeout.height, timeout.round, roundCertificate)),
        environment.allProcesses
      )
      environment' = applyConsensusResult(envWithoutTimeout, processId, result)
    }
  }

  /// Timeout fires while waiting for prevotes
  action onTimeoutPrevote(processId: ProcessID): bool = {
    val currentState = environment.processes.get(processId)

    val matchingTimeouts = environment.activeTimeouts.get(processId).filter(t =>
      t.height == currentState.height and
      t.round == currentState.round and
      t.timeoutType == PrevoteTimeout
    )

    val roundCertificateMessages = environment.messageBuffer.filter(msg =>
      match msg {
        | PrevoteMsg(pv) => all {
          pv.height == currentState.height,
          pv.round >= currentState.round
        }
        | _ => false
      }
    )

    val roundCertificate = roundCertificateMessages.fold(Set(), (acc, msg) =>
      match msg {
        | PrevoteMsg(pv) => acc.union(Set(pv))
        | _ => acc
      }
    )

    all {
      matchingTimeouts.size() == 1,
      hasQuorum(roundCertificateMessages, QUORUM_SIZE),
      val timeout = matchingTimeouts.getOnlyElement()
      OnTimeoutPrevoteSpecific(processId, timeout, roundCertificate)
    }
  }
  
  /// Leader selects value and proposes when no locked value exists
  action leaderSelectValueAndPropose(processId: ProcessID, messageBuffer: Set[Message]): bool = {
    val currentState = environment.processes.get(processId)
    val pendingRequest = environment.pendingValueRequests.get(processId)
    val isProposer = proposer(currentState.height, currentState.round, environment.allProcesses) == processId

    val canBroadcast = environment.pendingValueRequests.keys().contains(processId) and
                      pendingRequest != None and
                      isProposer and
                      currentState.step == Propose
    all {
      canBroadcast,
      nondet selectedValue = PROPOSAL_VALUES_PER_PROCESS.get(processId).oneOf()
      val justification = match pendingRequest {
        | Some(j) => j
        | None => Set()
      }
      val proposal = createProposal(
        processId,
        currentState.height,
        currentState.round,
        selectedValue,
        justification
      )
      environment' = {
        ...environment,
        messageBuffer: messageBuffer.union(Set(ProposalMsg(proposal))),
        pendingValueRequests: environment.pendingValueRequests.set(processId, None)
      }
    }
  }

  /// Timeout fires for rebroadcasting
  action onTimeoutRebroadcast(processId: ProcessID): bool = {
    val currentState = environment.processes.get(processId)
    
    val matchingTimeouts = environment.activeTimeouts.get(processId).filter(t =>
      t.height == currentState.height and 
      t.round == currentState.round and 
      t.timeoutType == RebroadcastTimeout
    )
    
    all {
      matchingTimeouts.size() == 1,
      val timeout = matchingTimeouts.getOnlyElement()
      val newTimeouts = environment.activeTimeouts.get(processId).exclude(Set(timeout))
      val newActiveTimeouts = environment.activeTimeouts.set(processId, newTimeouts)
      val envWithoutTimeout = { ...environment, activeTimeouts: newActiveTimeouts }
      val result = processConsensusInput(
        currentState,
        TimeoutRebroadcastInput,
        environment.allProcesses
      )
      environment' = applyConsensusResult(envWithoutTimeout, processId, result)
    }
  }
  
  // ============================================================================
  // 11. INITIALIZATION
  // ============================================================================
  
  /// Create initial local state for a process
  pure def initialLocalState(processId: ProcessID, allProcesses: List[ProcessID]): LocalState = {
    {
      processId: processId,
      height: INITIAL_HEIGHT,
      round: INITIAL_ROUND,
      step: Propose,
      decisions: Map(),
      max_rounds: allProcesses.foldl(Map(), (acc, proc) => acc.put(proc, INITIAL_ROUND)), // TODO: should this be -1 ?f
      prevotedValue: Nil,
      prevotedProposalMsg: None,
      roundCertificate: Set()
    }
  }

  // ============================================================================
  // 11. BYZANTINE MESSAGE GENERATION
  // ============================================================================

  /// Generate Byzantine proposals for all heights/rounds up to bounds
  pure def byzantineProposals(maxHeight: Height, maxRound: Round): Set[Message] = {
    tuples(byzantineProcessesSet, 0.to(maxHeight), 0.to(maxRound), BYZANTINE_VALUES).map(t => {
      val proposal = createProposal(t._1, t._2, t._3, t._4, Set())
      ProposalMsg(proposal)
    })
  }

  /// Generate Byzantine prevotes for all heights/rounds up to bounds
  pure def byzantinePrevotes(maxHeight: Height, maxRound: Round): Set[Message] = {
    tuples(byzantineProcessesSet, 0.to(maxHeight), 0.to(maxRound), BYZANTINE_PREVOTE_VALUES).map(t => {
      val prevote = createPrevote(t._1, t._2, t._3, Val(t._4))
      PrevoteMsg(prevote)
    })
  }

  /// Generate Byzantine decisions for all heights/rounds up to bounds
  pure def byzantineDecisions(maxHeight: Height, maxRound: Round): Set[Message] = {
    tuples(byzantineProcessesSet, 0.to(maxHeight), 0.to(maxRound), BYZANTINE_VALUES).map(t => {
      val decision = createDecision(t._1, t._2, t._3, Val(t._4), Set())
      DecisionMsg(decision)
    })
  }

  // Precomputed Byzantine message sets for init
  pure val BYZANTINE_INIT_MESSAGES =
    byzantineProposals(BYZANTINE_MAX_HEIGHT, BYZANTINE_MAX_ROUND)
      .union(byzantinePrevotes(BYZANTINE_MAX_HEIGHT, BYZANTINE_MAX_ROUND))

  pure val BYZANTINE_INIT_DECISIONS =
    byzantineDecisions(BYZANTINE_MAX_HEIGHT, BYZANTINE_MAX_ROUND)

  action init = all {
    val allProcs = correctProcesses.concat(byzantineProcesses)
    environment' = {
      allProcesses: allProcs,
      processes: honestProcessesSet.mapBy(p => initialLocalState(p, allProcs)),
      messageBuffer: BYZANTINE_INIT_MESSAGES,
      reliableBroadcastBuffer: BYZANTINE_INIT_DECISIONS,
      activeTimeouts: honestProcessesSet.mapBy(p => Set()),
      pendingValueRequests: honestProcessesSet.mapBy(p => None)
    }
  }

  /// Initialize with Byzantine node as first proposer
  action initByzantineProposer = all {
    val allProcs = byzantineProcesses.concat(correctProcesses)
    environment' = {
      allProcesses: allProcs,
      processes: honestProcessesSet.mapBy(p => initialLocalState(p, allProcs)),
      messageBuffer: BYZANTINE_INIT_MESSAGES,
      reliableBroadcastBuffer: BYZANTINE_INIT_DECISIONS,
      activeTimeouts: honestProcessesSet.mapBy(p => Set()),
      pendingValueRequests: honestProcessesSet.mapBy(p => None)
    }
  }

  // ============================================================================
  // 12. STEP ACTION - Non-deterministic system evolution
  // ============================================================================
  
  /// Message-driven steps
  action messageSteps(processId: ProcessID): bool = {
    any {
      followerReceiveProposal(processId, environment.messageBuffer),
      updateMaxRounds(processId, environment.messageBuffer),
      leaderProposeWithLock(processId, environment.messageBuffer),
      leaderProposeWithoutLock(processId, environment.messageBuffer),
      leaderSelectValueAndPropose(processId, environment.messageBuffer),
      enoughPrevotesForRound(processId),
      canSkipRound(processId, environment.messageBuffer),
      canDecide(processId, environment.messageBuffer),
      receiveDecision(processId, environment.messageBuffer)
    }
  }

  /// Timeout-driven steps
  action timeoutSteps(processId: ProcessID): bool = {
    any {
      onTimeoutPropose(processId),
      onTimeoutPrevote(processId),
      onTimeoutRebroadcast(processId)
    }
  }

  /// Non-deterministic system evolution
  action step = {
    nondet processId = honestProcessesSet.oneOf()
    any {
      processStart(processId),
      newHeight(processId),
      timeoutSteps(processId),
      messageSteps(processId)
    }
  }

  /// Single round exploration without timeouts (for testing)
  action singleRound_step_no_timeout = {
    nondet processId = honestProcessesSet.oneOf()
    any {
      processStart(processId),
      messageSteps(processId)
    }
  }
}