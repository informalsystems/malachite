use tokio::sync::mpsc;
use tokio::task;

use crate::{BoxError, CtrlMsg, Event};

pub struct RecvHandle {
    rx_event: mpsc::Receiver<Event>,
}

impl RecvHandle {
    pub async fn recv(&mut self) -> Option<Event> {
        self.rx_event.recv().await
    }
}

pub struct CtrlHandle {
    tx_ctrl: mpsc::Sender<CtrlMsg>,
    task_handle: task::JoinHandle<()>,
}

impl CtrlHandle {
    pub async fn broadcast(&self, data: Vec<u8>) -> Result<(), BoxError> {
        self.tx_ctrl.send(CtrlMsg::Broadcast(data)).await?;
        Ok(())
    }

    pub async fn wait_shutdown(self) -> Result<(), BoxError> {
        self.shutdown().await?;
        self.join().await?;
        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), BoxError> {
        self.tx_ctrl.send(CtrlMsg::Shutdown).await?;
        Ok(())
    }

    pub async fn join(self) -> Result<(), BoxError> {
        self.task_handle.await?;
        Ok(())
    }
}

pub struct Handle {
    recv: RecvHandle,
    ctrl: CtrlHandle,
}

impl Handle {
    pub fn new(
        tx_ctrl: mpsc::Sender<CtrlMsg>,
        rx_event: mpsc::Receiver<Event>,
        task_handle: task::JoinHandle<()>,
    ) -> Handle {
        Self {
            recv: RecvHandle { rx_event },
            ctrl: CtrlHandle {
                tx_ctrl,
                task_handle,
            },
        }
    }

    pub fn split(self) -> (RecvHandle, CtrlHandle) {
        (self.recv, self.ctrl)
    }

    pub async fn recv(&mut self) -> Option<Event> {
        self.recv.recv().await
    }

    pub async fn broadcast(&self, data: Vec<u8>) -> Result<(), BoxError> {
        self.ctrl.broadcast(data).await
    }

    pub async fn wait_shutdown(self) -> Result<(), BoxError> {
        self.ctrl.wait_shutdown().await
    }

    pub async fn shutdown(&self) -> Result<(), BoxError> {
        self.ctrl.shutdown().await
    }

    pub async fn join(self) -> Result<(), BoxError> {
        self.ctrl.join().await
    }
}
