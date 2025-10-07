# Fast Tendermint without the gossip property with quadratic complexity

## Overview

Processes rebroadcast PROPOSE messages and their highest PREVOTE message. Decisions are reliably broadcast.

Processes only need to store the highest prevote seen from any other process as well as the latest PROPOSAL message received from each processe. Latest is determinedby the round number. Each process maintains an array max_rounds : {0, . . . , n} → Round, whose j-th entry stores the maximal round received in a PREVOTE message from process p_j. The process also maintains two variables, max_round and max_round+, derived from max_rounds (initially -1): max_round+ (respectively, max_round) is equal to the maximal round such that at least f + 1 processes (respectively, 4f + 1 processes) have prevoted in. The two variables monotonically increase and we always have max_round ≤ max_round+. More formally:
- max_round+ = max{r | ∃k. max_rounds[k] = r ∧ |{j | max_rounds[j] ≥ r}| ≥ f + 1};
- max_round = max{r | ∃k. max_rounds[k] = r ∧ |{j | max_rounds[j] ≥ r}| ≥ 4f + 1}

## Pseudocode

```typescript

Initialization:
    h_p = 0 // current height, or consensus instance we are currently executing
    round_p = 0 // current round number
    step_p = nil
    decision_p[] = nil
    max_rounds[] = -1
    prevotedValue_p = nil
    prevotedProposalMsg_p = nil
    lastPrevote_p = nil

upon start do 
    schedule OnTimeoutRebroadcast() to be executed after timeoutRebroadcast
    StartRound(0)

Function StartRound(round) :
    round_p = round
    step_p = propose
    schedule OnTimeoutPropose(h_p , round_p) to be executed after timeoutPropose(round_p)
    if proposer(h_p, round_p) then
        if round_p == 0 then
            broadcast <PROPOSAL, h_p, round_p, getValue(), {}>
        else 
            step_p = prepropose        

upon <PROPOSAL, h_p, *, v, *> AND 4f+1 <PREVOTE, h_p, r, *> = S while r>=round_p-1 AND
  proposer(h_p, round_p) == p AND step_p == prepropose AND
  ∃ P ⊆ S. |P| >= 2f+1 AND v!=nil AND (∀m \in P. m=<PREVOTE, h_p, r, id(v)>) do 
    step_p = propose
    broadcast <PROPOSAL, h_p, round_p, v, S>

upon <4f+1 <PREVOTE, h_p, round_p-1, *> = S while r>=round_p-1 AND
  proposer(h_p, round_p) == p AND step_p == prepropose AND
  ∄ v, P ⊆ S. |P| >= 2f+1 AND v!=nil AND (∀m \in P. m=<PREVOTE, h_p, r, id(v)>) do
    step_p = propose
    broadcast <PROPOSAL, h_p, round_p, getValue(), S>

upon <PROPOSAL, h_p, round_p, v, S> = M from proposer(h_p, round_p) while step_p == propose do
    step_p = prevote
    if SafeProposal(M) then
        prevotedValue_p = v
        prevotedProposalMsg_p = <PROPOSAL, h_p, round_p, v, {}>
        lastPrevote_p = <PREVOTE, h_p, round_p, id(v)>
    else
        lastPrevote_p = <PREVOTE, h_p, round_p, id(prevotedValue_p)>
    broadcast lastPrevote_p

Function SafeProposal(<PROPOSAL, h, r, v, S>) :
    if ∃ v'', P ⊆ S. |P| >= 2f+1 AND v''!=nil AND (∀m \in P. m=<PREVOTE, h_p, r', id(v'')> AND r'>=round_p-1) then
        return id(v'') == id(v) AND Valid(v)
    else if (|S| == 4f+1 AND (∀m \in S. m=<PREVOTE, h_p, r', *> AND r'>=round_p-1)) OR (S == {} AND r == 0)
        return Valid(v)
    else
        return FALSE

upon max_round = round_p for the first time in round_p do
    schedule OnTimeoutPrevote(h_p , round_p) to be executed after timeoutPrevote(round_p)

upon <PROPOSAL, h_p, r, v, *> from proposer(h_p, r) AND 4f+1 <PREVOTE, h_p, r, id(v)> = S while decision_p[h_p] = nil AND v!=nil do
    reliable_broadcast <DECISION, h_p, v, S>
    OnDecision(v)

upon <DECISION, h_p, r, v, S> while decision_p[h_p] = nil AND
  |S| == 4f+1 AND v!=nil AND (∀m \in S. m=<PREVOTE, h_p, r, id(v)>) do
    OnDecision(v)

Function OnDecision(v) :
    decision_p[h_p] = v
    h_p = h_p + 1
    max_rounds[] = -1
    prevotedValue_p = nil
    prevotedProposalMsg_p = nil
    lastPrevote_p = nil
    empty message log
    StartRound(0)

upon <PREVOTE, h_p, round, *> from q while round > max_round[q] do
    overwrite q highest prevote message
    max_round+ = max{r | ∃k. max_rounds[k] = r ∧ |{j | max_rounds[j] ≥ r}| ≥ f + 1};
    max_round = max{r | ∃k. max_rounds[k] = r ∧ |{j | max_rounds[j] ≥ r}| ≥ 4f + 1}

upon max_round+ > round_p do
    StartRound(round+)

Function OnTimeoutPropose(h, r) :
    if h == h_p AND r == round_p AND (step_p == propose OR step_p == prepropose) then
        step_p = prevote
        lastPrevote_p = <PREVOTE, h_p, round_p, id(prevotedValue_p)>
        broadcast lastPrevote_p

Function OnTimeoutPrevote(h, r) :
    if h == h_p AND r = round_p then
        StartRound(round_p + 1)

Function OnTimeoutRebroadcast() :
    schedule OnTimeoutRebroadcast() to be executed after timeoutRebroadcast
    for lastPrevote_p != nil do
        broadcast lastPrevote_p
    if prevotedValue_p != nil then
        broadcast prevotedProposalMsg_p
```

## Proof of Validity
_Assume that a correct process decides v after receiving 4f+1 PREVOTE messages from round r. Then v is valid._

If a process decides a v, then v!=nil. Furthermore, we have that at least 3f+1 correct processes prevoted for v in round r. Let p be one of them. Consider first the case when p sends a PREVOTE message for v in round r as a result of receiving <PROPOSAL, h, r, v, S> message that satisfies the SafeProposal predicate. The SafeProposal predicate is satisfied only if v is valid. Consider now the case when p sends a PREVOTE message for v in round r as a result of receiving <PROPOSAL, h, r, v, S> message that does not satisfies the SafeProposal predicate or after its timeoutPropose(r) expires. In both cases, the process p prevotes prevotedValue_p. Given that v!=nil, then p must have updated the variable prevotedValue_p at some point. The variable prevotedValue_p is only updated after receiving a PROPOSAL message that satisfies SafeProposal. This guarantees that v is valid in this case as well.

## Proof of Safety

### Lemma 1
_Assume that a process p decides v after receiving 4f+1 PREVOTE messages from round r. Let C be the set of 3f+1 correct processes that send a PREVOTE message for v in round r. Then no correct process in C votes for v’ != v in a round > r_

Assume to contradict that some correct process in C votes for v'!=v in a round > r. Let q be the first correct process in C that prevotes for v'!=v and r'>r the round at which it prevotes. We have that q enters r' with prevotedValue_p=v. Thus, the process q prevotes v' in r' as a result of receiving a <PROPOSAL, h, r', v', S> that satisfies the SafeProposal predicate. This implies that S includes at least 2f+1 PREVOTE messages for v' from rounds >= r'-1, i.e., rounds >= r. Thus, at least a set C' of f+1 correct processes send a PREVOTE message for v' rounds >= r before q sends its PREVOTE message for v'.

The set of correct processes C must intersect with C' in at least one correct process c. Since we assume that no correct process in C votes for v'!=v in a round > r before q does, the process' c PREVOTE message in S for v' must have been sent in round r. Furthermore, any correct process in C (including c) sends a PREVOTE message for v!=v' in round r. Given that correct processes issue at most one PREVOTE message per round, it is impossible that the correct process c sends a PREVOTE message for v' and v in round r, which yields a contradiction.

### Proof of Agreement
_Assume that some correct processes p and q decide v and v' after receiving 4f+1 PREVOTE messages from round r and r' respectively. We now prove that v=v'._

We assume without loss of generality that r<=r'. Consider first the case when r=r'. Both processes have received 4f+1 PREVOTE messages. Those quorums intersect in 2f+1 correct processes. Since correct processes only issue a single PREVOTE message per round, then v’=v.

Consider now the case when r'>r. If q decides, then 4f+1 processes prevoted for v’ in round r'. Then, 3f+1 correct processes prevoted for v’ in round r'. By Lemma 1, there is a set C of 3f+1 correct processes that can only vote for v in r'. Both sets of 3f+1 correct processes must intersect in at least 2f+1 correct processes. Hence, v’=v, as required.

## Proof of Liveness

### Lemma 2

_If a correct process p enters a round r by t, then some correct process has send a PREVOTE message in round r-1 before t._

The process p enters round r either after its timeoutPrevote(r-1) expires while in round r-1 or when max_round+=r while in a round < r.

- In the former case, the process starts timeoutPrevote(r-1) when max_round=r-1, which implies that a correct process has sent a PREVOTE message from a round>=r-1. We have that if r'=r-1, we then get the required. Thus, assume that r'>=r.
- The latter case implies that a process has sent a PREVOTE message from a round>=r.

Therefore, in either case we have that a correct process has sent a PREVOTE message from a round>=r before p enters r, i.e., before t. Let q be the first correct process that sends a PREVOTE message from a round r'>=r.

Assume that the process q enters r' after its timeoutPrevote(r'-1) expires. The process starts timeoutPrevote(r'-1) when max_round=r'-1. This implies that a correct process has sent a PREVOTE message from a round r''>=r'-1. Since q is the first sending a PREVOTE message from a round>=r, then r''<=r-1, i.e., r'-1<=r''<=r-1. This implies that r'-1<=r-1, i.e., r'<=r. From this and r'>=r, we get that r'=r. Finally, from r'-1<=r''<=r-1 and r'=r, we get that r''=r-1. Therefore, a correct process sends a PREVOTE message from round r-1 before t, as required.

Assume now that the process q enters r' when max_round+=r' while in a round<r'. Then a correct process has sent a PREVOTE message from a round r''>=r'. Since q is the first sending a PREVOTE message from a round>=r, then r''<=r-1, i.e., r'<=r''<=r-1. This implies that r'<=r-1. But we have that r'>=r, i.e., r'>r-1. Hence, this case is impossible.

### Lemma 3

_Assume that a correct process enters a round r at time t. Then all rounds < r have been entered by some correct process before t._

Assume to contradict that there exists one or more rounds<r that have not been entered by any correct process by t. Let r' be the highest one that has not been entered. We have that r'+1 has been entered by some correct process. By Lemma 2, some correct process has sent a PREVOTE message from r' before t. This implies that some correct process enters r' before t, which yields a contradiction.

### Lemma 4

_Let p be the first correct process that enters a round r. Then, p enters after its timeoutPrevote(r-1) expires._

Let t be the time when p enters r. A process enters a round r either after its timeoutPrevote(r-1) while in round r-1 or when max_round+=r while in a round < r. Consider the latter case. If max_round+=r, then a correct process has sent a PREVOTE message from a round r' >= r before t. Since p is the first correct process that enters round r, we have that r'>r. By Lemma 3, if some correct process enters r', then all rounds < r' have been entered by a correct process before. This implies that in this case, r must be entered by a correct process before t, which is impossible. Hence, the process r must have entered r after its timeoutPrevote(r-1) expires, as required.

### Lemma 5

_Let t is the earliest time a correct process enters a round r. Then no correct process has entered a round r’>r by t_

Assume to contradict that some correct process enters a round r'>r before t. Let p be the first process that enters a round r'>r before t. By Lemma 3, all rounds < r' have been entered by a correct process before t. Thus, some correct process enters r+1 before t. By Lemma 2, if a correct process enters r+1, then some correct process sends a PREVOTE message from round r. This implies that a correct process enters r before t, which yields a contradiction.

### Lemma 6

_Let r be the highest round entered by some correct process at t. Assume that no correct process decides in a round <=r. We prove that some correct process enters r+1._

By Lemma 3, if some correct process enters a round > r+1, then some correct process enters r+1. Therefore, consider then the case when no correct process enters a round > r+1.

Let p be the first of the correct process that enters r. By Lemma 4, the process enters r after its timeoutPrevote(r-1) expires. Thus, p has received 4f+1 PREVOTE messages for rounds >= r-1, out of which at least 3f+1 are sent by correct processes. Each correct process rebroadcast its highest prevote periodically, then all correct processes will receive 3f+1 PREVOTE message for rounds >= r-1 by t'=max(GST, t)+timeoutRebroadcast+\Delta. Given that a max_rounds entry is only updated if the round of the received PREVOTE message is greater than the already stored, we have that all correct processes have max_round+>=r-1 by t'. We also have that no correct enters a round > r. Thus, no correct process could have max_round+>r by t'. This implies that all correct processes that are in rounds < r enter either r-1 or r by t'.

Any correct process that it is in round r by t' will eventually send a PREVOTE message from round r. This follows trivially from the assumption that no correct process enters a round>r. A correct process that it is in r-1 by t' will eventually issues a PREVOTE message in r-1 unless it enters r before. In the latter case, the process will eventually send a PREVOTE message from round r. Therefore, all correct processes eventually issue a PREVOTE message from rounds r-1 or r. Thus, all correct processes eventually receive 4f+1 PREVOTE messages from rounds r-1 and r. This guarantees that all correct processes have max_round>=r-1 at some point in time after t'. Any correct process that has not entered r (still in round r-1) by then will start its timeoutPrevote(r-1) and enter r eventually. Again, by the assumption that no correct process enters a round>r, all correct processes eventually send a PREVOTE message from round r. This guarantees that all correct processes receive at least 4f+1 PREVOTE messages for round r eventually. At that time, all correct processes start their timeoutPrevote(r) timer and enter r+1 once it expires. This yields a contradiction, proving the required.

### Corollary 1

_While no correct process decides, rounds are entered by some correct processes infinitely often._

Follows trivially from Lemma 6.

### Lemma 7

_Let t be the earliest time a correct process enters a round r. Then the earliest time that a correct process can enter a round r' > r is t + timetoutPrevote(r)._

By Lemma 3, if some correct process enters r', then all rounds < r' have been entered by some correct process before. This implies that (\*) no correct process can enter a round r'>r before the earliest time a correct process enters round r+1. Let p' be the first correct process that enters r+1. By Lemma 4, the process enters r after its timeoutPrevote(r) expires. Let t' be the the time when p' starts its timeoutPrevote(r). We have that t'>=t. Thus, we have then that the earliest time a correct process may enter a round r+1 is t+timeoutPrevote(r). By (\*), we get that the earliest time that a correct process can enter a round r' > r is t+timeoutPrevote(r), as required.

### Theorem 1

_Assume that:_
- _A correct process p is the first correct process to enter a round r > 0 at time t > GST_
- _No correct process decides in a round < r_
- _The proposer of round r is a correct process q_
- _timeoutPropose(r) > timeoutRebroadcast+3\Delta+timeoutPropose(r−1)+timeoutPrevote(r−1) and timeoutPrevote(r) > timeoutRebroadcast+2\Delta_

_then all correct processes decide in round r._

#### Claim 1

_All correct processes enter round r._

By Lemma 5, no correct process has entered a round r'>r by t. Assume to contradict that a correct process c that is in a round < r by t, enters a round r'>r without entering r, i.e., skipping r. We first compute the earliest time c may enter r'. By Lemma 3, if some correct process enters r', then all rounds < r' have been entered by some correct process before. This implies that c cannot enter round r' before the earliest time a correct process enters round r+1. Let p' be the first correct process that enters r+1.

By Lemma 4, the process p' enters r+1 after its timeoutPrevote(r) expires. Let t' be the time when p' starts its timeoutPrevote(r). We have then that the earliest time a correct process may enter a round r+1 is t'+timeoutPrevote(r), i.e, (\*) no correct process (including c) can enter round r'>r before t'+timeoutPrevote(r).

The process p' starts its timeoutPrevote(r) when max_round=r for the first time in round r. This implies that p' has received a set S of 4f+1 PREVOTE messages from rounds >= r by t'. By (\*), any PREVOTE messages in S sent by a correct process is for round r. Since t'>=t>=GST and t' is the earliest time any message in S could have been sent, all correct processes will receive 3f+1 PREVOTE messages from round r by t'+\Delta. Then, a correct process that it is in a round < r by t'+\Delta will enter r. We have that timeoutPrevote(r)>\Delta. By (\*), the process c cannot have entered a round r' by t'+\Delta. Thus, c enters round r, which yields a contradiction.

#### Claim 2

_The latest time a correct process enters is t+timeoutRebroadcast+2\Delta+timeoutPropose(r-1)+timeoutPrevote(r−1). Furthermore, all correct processes receive 4f+1 PREVOTE messages from rounds >=r-1 by t+timeoutRebroadcast+2\Delta+timeoutPropose(r-1)._

By Lemma 4, the process p enters r after its timeoutPrevote(r-1) expires. Thus, p has received 4f+1 PREVOTE messages from rounds >= r-1, out of which at least a set C of 3f+1 are sent by correct processes. All correct processes will rebroadcast their highest PREVOTE message by t+timeoutRebroadcast. By Lemma 7, we have that no correct process may have entered a round r'>r before t+timeoutPrevote(r). Since timeoutPrevote(r)>timeoutRebroadcast, then any correct process that sent a message in C will rebroadcast a PREVOTE message from round r-1 or r. All correct processes will receive 3f+1 PREVOTE messages for rounds r-1 and r by t + timeoutRebroadcast + \Delta.

Consider the case when at least f+1 PREVOTE messages in C were sent in round r. Then all correct processes enter r by t + timeoutRebroadcast + \Delta. Consider now the case when less than f+1 PREVOTE messages in C were sent in round r. A correct process that is in a round < r-1 by t + timeoutRebroadcast + \Delta will enter r-1 and start its timeoutPropose(r-1). Thus by t + timeoutRebroadcast + \Delta, at most f correct processes are in r and the rest are in r-1. Let C' be the set of correct processes that are in round r-1 by t + timeoutRebroadcast + \Delta.

Assume that no correct process in C' enters r before t + timeoutRebroadcast + \Delta + timeoutPropose(r-1). Then, all correct processes in C' send a PREVOTE message from r-1 by t + timeoutRebroadcast + \Delta + timeoutPropose(r-1). Therefore, all correct processes start timeoutPrevote(r-1) by t + timeoutRebroadcast + 2\Delta + timeoutPropose(r-1) and enter r by t + timeoutRebroadcast + 2\Delta + timeoutPropose(r-1) + timeoutPrevote(r-1), as required. Finally, assume that at least a correct process in C' enters r before t + timeoutRebroadcast + \Delta + timeoutPropose(r-1), i.e., before it sends a PREVOTE message from round r-1. This implies that at least one correct process c sends a PREVOTE message for round r before t + timeoutRebroadcast + \Delta + timeoutPropose(r-1). Given that timeoutPropose(r)>t + timeoutRebroadcast + \Delta + timeoutPropose(r-1), then the process c sends its PREVOTE message after receiving the round r's proposal. Thus, any correct process in C' that enters r before issuing a PREVOTE message from round r-1 will send a PREVOTE message from round r by t + timeoutRebroadcast + \Delta + timeoutPropose(r-1) the latest. Therefore, all correct processes send a PREVOTE message for round r-1 or r by t + timeoutRebroadcast + \Delta + timeoutPropose(r-1). Then, all correct processes receive 4f+1 PREVOTE message for rounds >= r-1 by t+timeoutRebroadcast+2\Delta+timeoutPropose(r-1). A correct process that is in round r-1 will either enter r by then if more than f+1 PREVOTE message are from round r, or start its timeoutPrevote(r-1) and enter r when it expires, i.e., by t+timeoutRebroadcast+2\Delta+timeoutPropose(r-1)+timeoutPrevote(r-1) as required.

#### Claim 3

_Assume that no correct process receives q's proposal. Then the earliest time a correct process sends a PREVOTE message from round r or enter a rounds > r is t+timeoutRebroadcast+2\Delta+timeoutPropose(r-1)+timeoutPrevote(r−1).

Assume that a correct process sends a PREVOTE message from round r'>r. By Lemma 3, if some correct process enters a round r', then all rounds < r' have been entered by some correct process before. This implies that a correct process cannot enter a round > r before the earliest time a correct process enters round r+1. Let p' be the first correct process that enters r+1. By Lemma 2, if a correct process enters round r+1, then some correct process has send a PREVOTE message for round r. Therefore, if a correct process sends a PREVOTE message from round r'>r, then some correct process has sent a PREVOTE message for round r. The earliest time a correct process may prevote in r before receiving the leaders proposal is t+timeoutPropose(r). Since timeoutPropose(r)>timeoutRebroadcast+2\Delta+timeoutPropose(r-1)+timeoutPrevote(r−1), no correct process sends a PREVOTE message from a round>=r or enter a round > r by t+timeoutRebroadcast+2\Delta+timeoutPropose(r-1)+timeoutPrevote(r−1), as required.

#### Claim 4

_The process q sends its proposal by t_1<=t+timeoutRebroadcast+2\Delta+timeoutPropose(r-1)+timeoutPrevote(r−1)_

By Claim 1, q enters round r. Let t'>=t be the time when q enters r. By Claim 2, we have that t<=t'<=t_1. Furthermore by Claim 3, we have that q cannot enter a round > r before t_1. To send a PROPOSAL message in a round r>0, q must receive 4f+1 PREVOTE messages for rounds>=r-1 before its timeoutPropose(r) expires. By Claim 2, we have that q receives a set S of 4f+1 PREVOTE messages from rounds >= round r-1 by t+timeoutRebroadcast+2\Delta+timeoutPropose(r-1). Since t is the earliest time q can enter r and timeoutPropose(r)>timeoutRebroadcast+2\Delta+timeoutPropose(r-1), then it is guaranteed that q receives S before its timeoutPropose(r) expires. Consider the case when there exist a time t'' after q enters r and no later than t_1, i.e., t'<=t''<=t_1, by which q stores a set 4f+1 PREVOTE messages from rounds >=r-1 in which no value !=nil is prevoted at least 2f+1 times. Then, q sends its proposal by t_1 as required. Consider now that such t'' does not exist and let S' be any set of 4f+1 PREVOTE messages from rounds >= r-1 that q stores by t_1. We have that there must be a value v!=nil prevoted at least 2f+1 times in S'. We now prove that q stores v by t_1.

We have that at least 2f+1 PREVOTE messages in S' are for value v. Let C be the set of f+1 correct processes with a PREVOTE message for v in S'. By Lemma 4, we also have that p enters r after its timeoutPrevote(r-1) expires. Thus, p has received a set S'' 4f+1 PREVOTE messages for rounds >= r-1, out of which at least 3f+1 are sent by correct processes. Let C' be the set of 3f+1 correct processes. We have that C and C' overlap in at least one correct process c.

By Claim 3, any PREVOTE message in S' and S'' sent by a correct process is from round r-1. Furthermore, any of these PREVOTE messages was sent before t, i.e., the earliest time any correct process enters r. Therefore, the correct process c sends a PREVOTE message for v before t. This implies that c has received the proposal message for value v before t. The process c will rebroadcast the value v by t + timeoutRebroadcast unless it prevotes for value v'!=v in a round > r-1 before t + timeoutRebroadcast.
By Claim 3, this is impossible. Hence, c rebroadcast the value v by t+timeoutRebroadcast. The process q must receive the value no later than t + timeoutRebroadcast + \Delta < t_1. Furthermore by Claim 3, c cannot broadcast a different value before t_1. This guarantees that q stores v by t_1. Hence, q sends its proposal by t_1 as required.

#### Proof of Theorem 1

By Claim 1, all correct processes enter r. The earliest time a correct process enters r is t. When a correct process enters round r, the process starts its timeoutPropose(r). Thus the earliest time timeoutPropose(r) expires at any correct process is t+timeoutPropose(r). By Claim 4 t+timeoutRebroadcast+2\Delta+timeoutPropose(r-1)+timeoutPrevote(r−1) is the latest time q sends its proposal.  Thus, all correct processes receives the leader proposal by timeoutRebroadcast+3\Delta+timeoutPropose(r-1)+timeoutPrevote(r−1). Since timeoutPropose(r)>timeoutRebroadcast+3\Delta+timeoutPropose(r-1)+timeoutPrevote(r−1), then all correct processes receive the leader's proposal before its timeoutPropose(r) expires. Let <PROPOSAL, h_p, r, v, *> be q's proposal message. Since q is correct and we have established that all correct processes receive the leader's proposal before their timeoutPropose(r) expires, a correct process send <PREVOTE, h_p, r, id(v)> in round r.

Therefore, we have established that all correct processes send <PREVOTE, h_p, r, id(v)> in r. To decide, a correct process must receive 4f+1 <PREVOTE, h_p, r, id(v)> messages (as well as the leader's proposal) before its timeoutPrevote(r) expires. Let t_1 the earliest time a correct process c starts its timeoutPrevote(r). Given that all correct processes send <PREVOTE, h_p, r, id(v)>, t_1 is guaranteed to exist.
This implies that the process c  has received <PREVOTE, h_p, r, id(v)> messages from a set C of 3f+1 correct processes by t_1. Furthermore, any process in C has received the leader's proposal t_1. We have that t_1>=t>=GST and that t_1 is the latest time q could have sent its proposal. Thus, all correct processes must have received the leader's proposal by t_1 + \Delta and prevote for it. Note that even if some correct processes were at time t_1 in a round smaller than r, they will enter round r by time t_1 + \delta. Then, all correct processes must have received a PREVOTE message for the leader's proposal from every correct processes by t_1 + 2\Delta. Since t_1 is the earliest time a correct process starts its timeoutPrevote(r) and timeoutPrevote(r) > 2\Delta, all correct processes will decide before the timeout expires, as required.

### Proof of Termination

We assume the following properties of the timers' duration:
1. timeoutPropose increments grow arbitrarily large. More formally: ∀θ. ∃r. ∀r'. r' ≥ r => timeoutPropose(r') > θ + timeoutPropose(r'-1).
2. timeoutPrevote duration exceed any fixed bound. More formally: ∀θ. ∃r. ∀r'. r' ≥ r => timeoutPrevote(r') > θ.
3. timeoutPropose minus timeoutPrevote grows beyond any bound. More formally: ∀θ. ∃r. ∀r'. r' ≥ r => timeoutPropose(r') − timeoutPrevote(r') > θ
4. timeoutRebroadcast is constant

Property 1 requires a timeoutPropose(r) function with superlinear growth, e.g., exponential (2^r) or quadratic (r^2). In contrast, a timeoutPrevote function with linear growth is sufficient to implement Property 2. Property 3 would be guaranteed as far as the duration of timeoutPropose grows faster than the duration of timeoutPrevote. Thus, it is guaranteed if for instance the protocol uses a timeoutPropose(r) function with superlinear growth (as required by Property 1) and a timeoutPrevote function that grows no faster than linear. Finally, Property 4 requires that the duration of timeoutRebroadcast is constant.

Assume that all correct processes eventually start height 0.

- Let r_0>=0 be the highest round entered by a correct process by GST.
- Let r_1>=r_0 be a round such that all correct processes have started height 0.
- Let r_2>=r_1 be a round such that timeoutPropose(r_2) > timeoutRebroadcast+timeoutPropose(r-1)+3\Delta+timeoutPrevote(r_2-1) and timeoutPrevote(r_2) > timeoutRebroadcast+2\Delta. By the timers' duration properties, such a round exists.
- Let r_3>=r_2 a round with a correct leader: we are always guaranteed to encounter a correct leader after at most f rounds.

Consider first the case when some correct process decides in a round < r_3. Since decisions are reliably broadcast, then all correct will eventually decide. Consider now the case when no process decides in any round < r_3. By Corollary 1, some correct process enters r_3. By Theorem 1, all correct processes decide in r_3, as required.

## Discussion

### On message complexity

Let |V| the value size. The protocol has O(|V|n^2) per-view communication complexity:

  - O(|V|n^2) due to the PROPOSAL message from leader to followers including 4f+1 PREVOTE messages
  - O(n^2) due to the all-to-all PREVOTE message
  - O(|V|n^2) due to the DECISION being reliably broadcast, which is the complexity of a non-optimized Bracha broadcast. We assume that the set of 4f+1 PREVOTE messages included in the DECISION message are aggregated.
  - O(|V|n^2) due to PROPOSAL messages rebroadcast: all-to-all PROPOSAL messages with an empty set of PREVOTE messages.
  - O(n^2) due to all-to-all highest PREVOTE messages.

### On termination and rebroadcasting the proposal

Ideally we would like to avoid rebroadcasting PROPOSAL messages. This seems to be required for termination. Let us show it with an example:

- We have that if f >=5 then n = 5f+1 is insufficient to guarantee that we always iterate over f+1 correct processes consecutively. For instance, assume that f=5, i.e., n=26. An arrangement that avoids any run of 5 correct processes (f+1) would be CCCCC F CCCC F CCCC F CCCC F CCCC F.
- Let r_0 be the highest round after GST. Assume that the leader of r_0 is Byzantine and it does not send a proposal.
- By Lemma 6, some correct process enters r_0+1.
- Assume that 2f+1 correct send <PREVOTE h_p, r0, id(v)>, 2f correct send <PREVOTE h_p, r_0, id(v')> and that faulty do not participate.
- Assume that the leader of r_0+1 is correct. Then it can only propose v but it has never received the proposal, so its timeoutPropose eventually expires.
- By Lemma 6, some correct process enters r_0+2
- The above scenario may repeat at most f times: up to r_0+f included. This is because the value v may have only be received by 2f+1 correct and we are iterating over f different correct processes.
- Assume now that in r_0+f (the last round of the previous round), Byzantine processes send <PREVOTE h_p, r0, id(v'')>.
- Assume that the leader of r_0+f+1 is Byzantine. The faulty leader could then propose its own value and send f <PREVOTE h_p, r0, id(v)>, 2f <PREVOTE h_p, r_0, id(v')> and a <PREVOTE h_p, r0, id(v'')> as justification.
- Assume that it sends its proposal x to only 2f+1 correct, including the f we iterated over before (which won;t be iterated over in immediately after).
- The scenario can repeat forever: then next f correct process would have to propose x, but haven;t received the PROPOSE message.

### On message loss and bounded space

The protocol runs in bounded space while tolerating message loss:

- Processes rebroadcast a finite number of messages: a round certificate (f+1 or 4f+1 PREVOTE messages) and a proposal.
- Processes only store the highest prevote seen from any other process as well as one proposal.

### On minimizing validity checks

The protocol currently includes two validity checks via the Valid predicate. The question of whether the validity check in the SafeProposal predicate when the leader is reproposing a value v (if clause) is required or not remains open.

### Implementation details

- The reliable broadcast mechanism does not need to be implemented. One could argue that ValueSync implements it.
- The max_rounds variable should be part of the vote keeper.
- The quorum counting requires that the messages are from different processes. For instance, the `upon <PROPOSAL, h_p, *, v, *> AND 4f+1 <PREVOTE, h_p, r, *> = S while r>=round_p-1` clause requires that the 4f+1 PREVOTE messages are from different processes.
