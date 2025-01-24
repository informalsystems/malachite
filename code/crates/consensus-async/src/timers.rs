use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::RangeFrom;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::trace;

#[derive(Debug)]
struct Timer<Key> {
    /// Message to give to the actor when the timer expires
    key: Key,

    // Task that will notify the actor that the timer has elapsed
    task: JoinHandle<()>,

    /// Generation counter to the timer to check if we received a timeout
    /// message from an old timer that was enqueued in mailbox before canceled
    generation: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TimeoutElapsed<Key> {
    key: Key,
    generation: u64,
}

#[derive(Debug)]
pub struct Timers<Key>
where
    Key: Clone + Eq + Hash + Send + 'static,
{
    tx: Arc<broadcast::Sender<TimeoutElapsed<Key>>>,
    timers: HashMap<Key, Timer<Key>>,
    generations: RangeFrom<u64>,
}

impl<Key> Timers<Key>
where
    Key: Clone + Eq + Hash + Send + 'static,
{
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1);

        Self {
            tx: Arc::new(tx),
            timers: HashMap::new(),
            generations: 1..,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<TimeoutElapsed<Key>> {
        self.tx.subscribe()
    }

    /// Start a timer that will send `msg` once to the actor after the given `timeout`.
    ///
    /// Each timer has a key and if a new timer with same key is started
    /// the previous is cancelled.
    ///
    /// # Warning
    /// It is NOT guaranteed that a message from the previous timer is not received,
    /// as it could already be enqueued in the mailbox when the new timer was started.
    ///
    /// When the actor receives a timeout message for timer from the scheduler, it should
    /// check if the timer is still active by calling [`TimerScheduler::intercept_timer_msg`]
    /// and ignore the message otherwise.
    pub fn start(&mut self, key: Key, timeout: Duration)
    where
        Key: Clone + Send + 'static,
    {
        self.cancel(&key);

        let generation = self
            .generations
            .next()
            .expect("generation counter overflowed");

        let task = {
            let key = key.clone();
            let tx = Arc::clone(&self.tx);

            tokio::spawn(async move {
                tokio::time::sleep(timeout).await;
                let _ = tx.send(TimeoutElapsed { key, generation });
            })
        };

        self.timers.insert(
            key.clone(),
            Timer {
                key,
                task,
                generation,
            },
        );
    }

    /// Cancel a timer with a given `key`.
    ///
    /// If canceling a timer that was already canceled, or key never was used to start a timer
    /// this operation will do nothing.
    ///
    /// # Warning
    /// It is NOT guaranteed that a message from a canceled timer, including its previous incarnation
    /// for the same key, will not be received by the actor, as the message might already
    /// be enqueued in the mailbox when cancel is called.
    ///
    /// When the actor receives a timeout message for timer from the scheduler, it should
    /// check if the timer is still active by calling [`TimerScheduler::intercept_timer_msg`]
    /// and ignore the message otherwise.
    pub fn cancel(&mut self, key: &Key) {
        if let Some(timer) = self.timers.remove(key) {
            timer.task.abort();
        }
    }

    /// Cancel all timers.
    pub fn cancel_all(&mut self) {
        self.timers.drain().for_each(|(_, timer)| {
            timer.task.abort();
        });
    }

    /// Intercepts a timer message and checks the state of the timer associated with the provided `timer_msg`:
    ///
    /// 1. If the timer message was from canceled timer that was already enqueued in mailbox, returns `None`.
    /// 2. If the timer message was from an old timer that was enqueued in mailbox before being canceled, returns `None`.
    /// 3. Otherwise it is a valid timer message, returns the associated `Key` wrapped in `Some`.
    pub fn intercept_timer_msg(&mut self, timer_msg: TimeoutElapsed<Key>) -> Option<Key>
    where
        Key: Debug,
    {
        match self.timers.entry(timer_msg.key) {
            // The timer message was from canceled timer that was already enqueued in mailbox
            Entry::Vacant(entry) => {
                let key = entry.key();
                trace!("Received timer {key:?} that has been removed, discarding");
                None
            }

            // The timer message was from an old timer that was enqueued in mailbox before being canceled
            Entry::Occupied(entry) if timer_msg.generation != entry.get().generation => {
                let (key, timer) = (entry.key(), entry.get());

                trace!(
                    "Received timer {key:?} from old generation {}, expected generation {}, discarding",
                    timer_msg.generation,
                    timer.generation,
                );

                None
            }

            // Valid timer message
            Entry::Occupied(entry) => {
                let timer = entry.remove();
                Some(timer.key)
            }
        }
    }
}

impl<Key> Default for Timers<Key>
where
    Key: Clone + Eq + Hash + Send + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Key> Drop for Timers<Key>
where
    Key: Clone + Eq + Hash + Send + 'static,
{
    fn drop(&mut self) {
        self.cancel_all();
    }
}

#[cfg(test)]
impl<Key> Timers<Key>
where
    Key: Clone + Eq + Hash + Send + 'static,
{
    /// Check if a timer with a given `key` is active, ie. it hasn't been canceled nor has it elapsed yet.
    fn is_timer_active(&self, key: &Key) -> bool {
        self.timers.contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;
    use tokio::time::sleep;

    #[derive(Copy, Debug, Clone, PartialEq, Eq, Hash)]
    struct TestKey(&'static str);

    async fn scheduler() -> Timers<TestKey> {
        Timers::new()
    }

    #[tokio::test]
    async fn test_start_timer() {
        let mut scheduler = scheduler().await;
        let key = TestKey("timer1");

        scheduler.start(key, Duration::from_millis(100));
        assert!(scheduler.is_timer_active(&key));

        sleep(Duration::from_millis(150)).await;
        let elapsed_key = scheduler.intercept_timer_msg(TimeoutElapsed { key, generation: 1 });
        assert_eq!(elapsed_key, Some(key));

        assert!(!scheduler.is_timer_active(&key));
    }

    #[tokio::test]
    async fn test_cancel_timer() {
        let mut scheduler = scheduler().await;
        let key = TestKey("timer1");

        scheduler.start(key, Duration::from_millis(100));
        scheduler.cancel(&key);

        assert!(!scheduler.is_timer_active(&key));
    }

    #[tokio::test]
    async fn test_cancel_all_timers() {
        let mut scheduler = scheduler().await;

        scheduler.start(TestKey("timer1"), Duration::from_millis(100));
        scheduler.start(TestKey("timer2"), Duration::from_millis(200));

        scheduler.cancel_all();

        assert!(!scheduler.is_timer_active(&TestKey("timer1")));
        assert!(!scheduler.is_timer_active(&TestKey("timer2")));
    }

    #[tokio::test]
    async fn test_intercept_timer_msg_valid() {
        let mut scheduler = scheduler().await;
        let key = TestKey("timer1");

        scheduler.start(key, Duration::from_millis(100));
        sleep(Duration::from_millis(150)).await;

        let timer_msg = TimeoutElapsed { key, generation: 1 };

        let intercepted_msg = scheduler.intercept_timer_msg(timer_msg);

        assert_eq!(intercepted_msg, Some(key));
    }

    #[tokio::test]
    async fn test_intercept_timer_msg_invalid_generation() {
        let mut scheduler = scheduler().await;
        let key = TestKey("timer1");

        scheduler.start(key, Duration::from_millis(100));
        scheduler.start(key, Duration::from_millis(200));

        let timer_msg = TimeoutElapsed { key, generation: 1 };

        let intercepted_msg = scheduler.intercept_timer_msg(timer_msg);

        assert_eq!(intercepted_msg, None);
    }

    #[tokio::test]
    async fn test_intercept_timer_msg_cancelled() {
        let mut scheduler = scheduler().await;
        let key = TestKey("timer1");

        scheduler.start(key, Duration::from_millis(100));
        scheduler.cancel(&key);

        let timer_msg = TimeoutElapsed { key, generation: 1 };

        let intercepted_msg = scheduler.intercept_timer_msg(timer_msg);

        assert_eq!(intercepted_msg, None);
    }
}
