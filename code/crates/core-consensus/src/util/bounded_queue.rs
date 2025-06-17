use std::collections::BTreeMap;

/// A data structure that maintains a queue of values associated with monotonically increasing indices.
///
/// # Type Parameters
/// - `I`: The type of the index associated with each value in the queue.
/// - `T`: The type of values stored in the queue.
#[derive(Clone, Debug)]
pub struct BoundedQueue<I, T> {
    capacity: usize,
    queue: BTreeMap<I, Vec<T>>,
}

impl<I, T> BoundedQueue<I, T>
where
    I: Ord,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            queue: BTreeMap::new(),
        }
    }

    pub fn push(&mut self, index: I, value: T) -> bool
    where
        I: Ord,
    {
        // If the index already exists, append the value to the existing vector.
        if let Some(values) = self.queue.get_mut(&index) {
            values.push(value);
            return true;
        }

        // If the index does not exist, check if we can add a new entry.
        if self.queue.len() < self.capacity {
            self.queue.insert(index, vec![value]);
            return true;
        }

        // If the queue is full, do not insert the new value.
        false
    }

    /// Combination of `shift` and `take` methods.
    pub fn shift_and_take(&mut self, min_index: &I) -> impl Iterator<Item = T> {
        self.shift(min_index);
        self.take(min_index)
    }

    /// Remove all entries with indices less than `min_index`.
    pub fn shift(&mut self, min_index: &I) {
        self.queue.retain(|index, _| index >= min_index);
    }

    /// Take all entries with indices equal to `index` and return them.
    pub fn take(&mut self, index: &I) -> impl Iterator<Item = T> {
        self.queue
            .remove(index)
            .into_iter()
            .flat_map(|values| values.into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
}
