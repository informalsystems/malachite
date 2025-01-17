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
restart, so that it operates correctly upon recovery.

WIP, main reference: https://github.com/informalsystems/malachite/issues/578.
