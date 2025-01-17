# Crash-Recovery Support

Tendermint can be implemented so that it adheres to the **crash-recovery**
failure model. In this model, a process can **crash**
(that is, it abruptly ceases its participation in the algorithm)
and later **recover**, rejoining the distributed computation from the point it
has left it.

A processe that crashes and later recovers is modelled as a **correct process**,
that goes through a long period of asynchrony, during which it does process
incoming events and does not perform any action.
This statement, however, is only valid as long as the recovered process behaves
and performs actions that are consistent with the algorithm state it had before
it has crashed.

This document discusses what information a process running Tendermint should
**persist** to stable storage during its regular execution,
and how to **replay** the persisted information when recovering from a crash or
restart, so that the process operates correctly upon recovery.

WIP, main reference: https://github.com/informalsystems/malachite/issues/578.

## WAL

A common approach for supporting the crash-recovery failure model is to rely
on a [Write-Ahead Log (WAL)][wal-link], a mechanism originally devised for
transactional databases.
The principle is that all events or inputs processed by the consensus algorithm
are persisted into an append-only log - the WAL - before their processing
produces actions or outputs.
Upon recovery, a process replays all the events or inputs stored in WAL, in the
order in which they were stored, and delivers them to consensus algorithm for
being processed.

Assuming that the consensus algorithm implementation is **deterministic**, the
state that the process has reached before the it has crashed will be restored
once the all events and inputs persisted in the WAL are replayed by the
recovered process.
Notice that the recovered process is expected, except if instructed otherwise,
to produce the same actions or outputs it has produced before it crashed.
This behavior is not _per se_ a problem, as (i) it does not constitute a
misbehavior, and (ii) in some cases it is even desirable.

> Regarding (i), the algorithm is expected to properly handle duplicated
> messages, a situation that may also be produced by the network.
> As an example of (ii), assume that a process has broadcast a message just
> before crashing; there is no guarantee that the message is received by
> correct processes, which can be amended by broadcasting it again.

WIP, main reference: https://github.com/informalsystems/malachite/issues/469.

[wal-link]: https://en.wikipedia.org/wiki/Write-ahead_log
