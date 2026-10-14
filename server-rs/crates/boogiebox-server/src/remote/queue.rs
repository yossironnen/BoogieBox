//! Pure queue logic for a renderer session: play order, shuffle, repeat, and edits. No I/O.

use serde::{Deserialize, Serialize};

/// What a queue entry plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QueueItem {
    /// A library track.
    #[serde(rename_all = "camelCase")]
    Track {
        /// Track id.
        id: String,
        /// Title.
        title: String,
        /// Artist.
        artist: Option<String>,
        /// Album.
        album: Option<String>,
        /// Album id (artwork).
        album_id: Option<String>,
        /// Duration in seconds.
        duration: Option<f64>,
        /// Artist Radio "why is this playing" reason, passed through for the UI.
        #[serde(skip_serializing_if = "Option::is_none")]
        radio_reason: Option<serde_json::Value>,
    },
    /// A rendered BoogieMix output.
    #[serde(rename_all = "camelCase")]
    Mix {
        /// Output id.
        id: String,
        /// Mix name.
        title: String,
        /// Duration in seconds.
        duration: Option<f64>,
    },
}

impl QueueItem {
    /// Display title.
    pub fn title(&self) -> &str {
        match self {
            QueueItem::Track { title, .. } | QueueItem::Mix { title, .. } => title,
        }
    }

    /// Known duration in seconds.
    pub fn duration(&self) -> Option<f64> {
        match self {
            QueueItem::Track { duration, .. } | QueueItem::Mix { duration, .. } => *duration,
        }
    }
}

/// One queued item with a stable id (`qid`) that survives reordering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    /// Stable entry id within the session.
    pub qid: u64,
    /// The item.
    #[serde(flatten)]
    pub item: QueueItem,
}

/// Repeat mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    /// Stop after the last item.
    #[default]
    Off,
    /// Repeat the current item.
    One,
    /// Wrap around to the first item.
    All,
}

/// Where `enqueue` inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnqueueMode {
    /// After the last item.
    End,
    /// Right after the current item.
    Next,
}

/// Session queue. Entries are kept in **play order** (shuffle reorders the upcoming part in place), so the UI
/// list and playback agree; `unshuffled` remembers the original order for turning shuffle off.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Queue {
    entries: Vec<QueueEntry>,
    unshuffled: Vec<u64>,
    pos: usize,
    shuffle: bool,
    repeat: Repeat,
    next_qid: u64,
    seed: u64,
}

/// Serializable queue state, saved so a session survives a server restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    /// Entries in play order.
    pub entries: Vec<QueueEntry>,
    /// Original order (qids), for turning shuffle off.
    pub unshuffled: Vec<u64>,
    /// Current index in play order.
    pub pos: usize,
    /// Shuffle flag.
    pub shuffle: bool,
    /// Repeat mode.
    pub repeat: Repeat,
    /// Next qid to hand out.
    pub next_qid: u64,
}

fn xorshift(seed: &mut u64) -> u64 {
    let mut x = (*seed).max(1);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *seed = x;
    x
}

impl Queue {
    /// Empty queue with a shuffle seed (tests pass a fixed one).
    pub fn with_seed(seed: u64) -> Self {
        Self {
            seed,
            next_qid: 1,
            ..Self::default()
        }
    }

    /// Saveable copy of the queue.
    pub fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            entries: self.entries.clone(),
            unshuffled: self.unshuffled.clone(),
            pos: self.pos,
            shuffle: self.shuffle,
            repeat: self.repeat,
            next_qid: self.next_qid,
        }
    }

    /// Rebuilds a queue from a snapshot, repairing anything inconsistent (unknown or missing qids, an
    /// out-of-range position, a qid counter that would reuse an id).
    pub fn from_snapshot(snapshot: QueueSnapshot, seed: u64) -> Self {
        let mut seen = std::collections::HashSet::new();
        let entries: Vec<QueueEntry> = snapshot
            .entries
            .into_iter()
            .filter(|e| e.qid > 0 && seen.insert(e.qid))
            .collect();
        let mut unshuffled: Vec<u64> = snapshot
            .unshuffled
            .into_iter()
            .filter(|q| seen.remove(q))
            .collect();
        // Entries missing from the original order go to its end, in play order.
        unshuffled.extend(entries.iter().map(|e| e.qid).filter(|q| seen.contains(q)));
        let max_qid = entries.iter().map(|e| e.qid).max().unwrap_or(0);
        Self {
            pos: snapshot.pos.min(entries.len().saturating_sub(1)),
            entries,
            unshuffled,
            shuffle: snapshot.shuffle,
            repeat: snapshot.repeat,
            next_qid: snapshot.next_qid.max(max_qid + 1),
            seed,
        }
    }

    fn wrap(&mut self, items: Vec<QueueItem>) -> Vec<QueueEntry> {
        items
            .into_iter()
            .map(|item| {
                let qid = self.next_qid.max(1);
                self.next_qid = qid + 1;
                QueueEntry { qid, item }
            })
            .collect()
    }

    fn shuffle_after(&mut self, from: usize) {
        if from >= self.entries.len() {
            return;
        }
        let tail = &mut self.entries[from..];
        for i in (1..tail.len()).rev() {
            let j = (xorshift(&mut self.seed) % (i as u64 + 1)) as usize;
            tail.swap(i, j);
        }
    }

    /// Replaces the queue. `start` indexes `items` as given; with shuffle on, the start item plays first and
    /// the rest are shuffled after it.
    pub fn load(&mut self, items: Vec<QueueItem>, start: usize, shuffle: bool) {
        self.entries = self.wrap(items);
        self.unshuffled = self.entries.iter().map(|e| e.qid).collect();
        self.pos = start.min(self.entries.len().saturating_sub(1));
        self.shuffle = shuffle;
        if shuffle && !self.entries.is_empty() {
            let current = self.entries.remove(self.pos);
            self.entries.insert(0, current);
            self.pos = 0;
            self.shuffle_after(1);
        }
    }

    /// Entries in play order.
    pub fn entries(&self) -> &[QueueEntry] {
        &self.entries
    }

    /// True when the queue has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Index of the current entry in play order.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Current entry.
    pub fn current(&self) -> Option<&QueueEntry> {
        self.entries.get(self.pos)
    }

    /// Shuffle flag.
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Repeat mode.
    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    fn index_of(&self, qid: u64) -> Option<usize> {
        self.entries.iter().position(|e| e.qid == qid)
    }

    /// The entry that plays after the current one ends naturally (honors repeat).
    pub fn peek_next(&self) -> Option<&QueueEntry> {
        if self.entries.is_empty() {
            return None;
        }
        match self.repeat {
            Repeat::One => self.current(),
            Repeat::All => self.entries.get((self.pos + 1) % self.entries.len()),
            Repeat::Off => self.entries.get(self.pos + 1),
        }
    }

    /// Natural end of the current item: moves to [`Queue::peek_next`]. Returns false at the end of the queue.
    pub fn advance(&mut self) -> bool {
        let Some(next_qid) = self.peek_next().map(|e| e.qid) else {
            return false;
        };
        self.pos = self.index_of(next_qid).unwrap_or(self.pos);
        true
    }

    /// User "next": ignores repeat-one; wraps only with repeat-all. Returns false at the end.
    pub fn skip_next(&mut self) -> bool {
        if self.pos + 1 < self.entries.len() {
            self.pos += 1;
            true
        } else if self.repeat == Repeat::All && !self.entries.is_empty() {
            self.pos = 0;
            true
        } else {
            false
        }
    }

    /// User "previous": wraps only with repeat-all. Returns false at the start.
    pub fn skip_prev(&mut self) -> bool {
        if self.pos > 0 {
            self.pos -= 1;
            true
        } else if self.repeat == Repeat::All && !self.entries.is_empty() {
            self.pos = self.entries.len() - 1;
            true
        } else {
            false
        }
    }

    /// Makes `qid` current. Returns false when unknown.
    pub fn jump(&mut self, qid: u64) -> bool {
        match self.index_of(qid) {
            Some(i) => {
                self.pos = i;
                true
            }
            None => false,
        }
    }

    /// Adds items after the current one or at the end. Returns the new entries' qids.
    pub fn enqueue(&mut self, items: Vec<QueueItem>, mode: EnqueueMode) -> Vec<u64> {
        let new = self.wrap(items);
        let qids: Vec<u64> = new.iter().map(|e| e.qid).collect();
        let at = match mode {
            EnqueueMode::End => self.entries.len(),
            EnqueueMode::Next if self.entries.is_empty() => 0,
            EnqueueMode::Next => self.pos + 1,
        };
        let un_at = match mode {
            EnqueueMode::End => self.unshuffled.len(),
            EnqueueMode::Next => self
                .current()
                .and_then(|c| self.unshuffled.iter().position(|q| *q == c.qid))
                .map_or(0, |i| i + 1),
        };
        for (i, q) in qids.iter().enumerate() {
            self.unshuffled.insert(un_at + i, *q);
        }
        self.entries.splice(at..at, new);
        qids
    }

    /// Removes an entry. Returns `Some(true)` when the removed entry was current (the following entry, if any,
    /// becomes current), `Some(false)` otherwise, `None` when unknown.
    pub fn remove(&mut self, qid: u64) -> Option<bool> {
        let i = self.index_of(qid)?;
        self.entries.remove(i);
        self.unshuffled.retain(|q| *q != qid);
        let was_current = i == self.pos;
        if i < self.pos {
            self.pos -= 1;
        }
        if self.pos >= self.entries.len() {
            self.pos = self.entries.len().saturating_sub(1);
        }
        Some(was_current)
    }

    /// Moves an entry to `to` (play-order index, clamped). The current entry stays current.
    pub fn move_to(&mut self, qid: u64, to: usize) -> bool {
        let Some(from) = self.index_of(qid) else {
            return false;
        };
        let current = self.current().map(|e| e.qid);
        let entry = self.entries.remove(from);
        let to = to.min(self.entries.len());
        self.entries.insert(to, entry);
        if let Some(c) = current {
            self.pos = self.index_of(c).unwrap_or(0);
        }
        true
    }

    /// Removes everything.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.unshuffled.clear();
        self.pos = 0;
    }

    /// Turns shuffle on (upcoming entries reshuffled) or off (original order restored, current kept).
    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.shuffle_after(self.pos + 1);
        } else {
            let current = self.current().map(|e| e.qid);
            let mut by_qid: std::collections::HashMap<u64, QueueEntry> =
                self.entries.drain(..).map(|e| (e.qid, e)).collect();
            self.entries = self
                .unshuffled
                .iter()
                .filter_map(|q| by_qid.remove(q))
                .collect();
            self.pos = current.and_then(|c| self.index_of(c)).unwrap_or(0);
        }
    }

    /// Sets the repeat mode.
    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn track(id: &str) -> QueueItem {
        QueueItem::Track {
            id: id.into(),
            title: format!("Title {id}"),
            artist: None,
            album: None,
            album_id: None,
            duration: Some(10.0),
            radio_reason: None,
        }
    }

    fn ids(q: &Queue) -> Vec<String> {
        q.entries()
            .iter()
            .map(|e| match &e.item {
                QueueItem::Track { id, .. } | QueueItem::Mix { id, .. } => id.clone(),
            })
            .collect()
    }

    fn cur(q: &Queue) -> String {
        q.current()
            .unwrap()
            .item
            .title()
            .trim_start_matches("Title ")
            .to_string()
    }

    fn items(n: usize) -> Vec<QueueItem> {
        (0..n).map(|i| track(&i.to_string())).collect()
    }

    #[test]
    fn snapshot_round_trips_through_json_and_keeps_behaving() {
        let mut q = Queue::with_seed(3);
        q.load(items(6), 2, true);
        q.set_repeat(Repeat::All);
        q.enqueue(vec![track("x")], EnqueueMode::Next);
        let json = serde_json::to_string(&q.snapshot()).unwrap();
        let mut back = Queue::from_snapshot(serde_json::from_str(&json).unwrap(), 9);
        assert_eq!(back.snapshot(), q.snapshot());
        assert_eq!(cur(&back), cur(&q));
        // New entries never reuse a restored qid, and shuffle off restores the original order.
        let new = back.enqueue(vec![track("y")], EnqueueMode::End);
        assert!(new[0] > q.entries().iter().map(|e| e.qid).max().unwrap());
        back.set_shuffle(false);
        assert_eq!(ids(&back)[..3], ["0", "1", "2"]);
    }

    #[test]
    fn snapshot_repairs_inconsistent_state() {
        let mut q = Queue::with_seed(1);
        q.load(items(3), 0, false);
        let mut snap = q.snapshot();
        snap.entries.push(snap.entries[0].clone()); // duplicate qid
        snap.unshuffled = vec![snap.entries[2].qid, 999]; // unknown + missing qids
        snap.pos = 42;
        snap.next_qid = 0;
        let back = Queue::from_snapshot(snap, 1);
        assert_eq!(ids(&back), ["0", "1", "2"]);
        assert_eq!(back.position(), 2);
        assert_eq!(back.snapshot().unshuffled.len(), 3);
        assert!(back.snapshot().next_qid > 3);
        let empty = Queue::from_snapshot(Queue::with_seed(1).snapshot(), 1);
        assert!(empty.is_empty() && empty.current().is_none());
    }

    #[test]
    fn load_and_navigate_without_repeat() {
        let mut q = Queue::with_seed(7);
        assert!(q.is_empty() && q.current().is_none() && q.peek_next().is_none());
        assert!(!q.advance() && !q.skip_next() && !q.skip_prev());
        q.load(items(3), 1, false);
        assert_eq!(cur(&q), "1");
        assert_eq!(q.peek_next().unwrap().item.title(), "Title 2");
        assert!(q.advance());
        assert_eq!(cur(&q), "2");
        assert!(!q.advance(), "end of queue");
        assert!(!q.skip_next());
        assert!(q.skip_prev());
        assert_eq!(cur(&q), "1");
        assert!(q.skip_prev() && !q.skip_prev());
        q.load(items(2), 99, false);
        assert_eq!(q.position(), 1, "start clamped");
    }

    #[test]
    fn repeat_modes() {
        let mut q = Queue::with_seed(1);
        q.load(items(2), 1, false);
        q.set_repeat(Repeat::One);
        assert_eq!(q.repeat(), Repeat::One);
        assert!(q.advance());
        assert_eq!(cur(&q), "1", "repeat one replays");
        assert!(
            !q.skip_next(),
            "user next still stops at the end without repeat-all"
        );
        q.set_repeat(Repeat::All);
        assert!(q.advance());
        assert_eq!(cur(&q), "0", "wraps");
        assert!(q.skip_prev());
        assert_eq!(cur(&q), "1");
        assert!(q.skip_next());
        assert_eq!(cur(&q), "0");
    }

    #[test]
    fn shuffle_keeps_current_first_and_restores_order() {
        let mut q = Queue::with_seed(42);
        q.load(items(8), 3, true);
        assert!(q.shuffle());
        assert_eq!(cur(&q), "3");
        assert_eq!(q.position(), 0);
        let mut sorted = ids(&q);
        sorted.sort();
        let mut expected: Vec<String> = (0..8).map(|i| i.to_string()).collect();
        expected.sort();
        assert_eq!(sorted, expected, "same items, different order");
        assert_ne!(ids(&q), (0..8).map(|i| i.to_string()).collect::<Vec<_>>());
        assert!(q.advance());
        let now = cur(&q);
        q.set_shuffle(false);
        assert_eq!(ids(&q), (0..8).map(|i| i.to_string()).collect::<Vec<_>>());
        assert_eq!(cur(&q), now, "current survives unshuffle");
        q.set_shuffle(false); // no-op
        q.set_shuffle(true);
        assert_eq!(cur(&q), now, "current survives reshuffle");
    }

    #[test]
    fn enqueue_next_and_end_also_update_unshuffled_order() {
        let mut q = Queue::with_seed(3);
        let first = q.enqueue(vec![track("a")], EnqueueMode::Next);
        assert_eq!(first.len(), 1);
        assert_eq!(cur(&q), "a", "enqueue into an empty queue makes it current");
        q.enqueue(vec![track("c")], EnqueueMode::End);
        q.enqueue(vec![track("b")], EnqueueMode::Next);
        assert_eq!(ids(&q), vec!["a", "b", "c"]);
        q.set_shuffle(true);
        q.set_shuffle(false);
        assert_eq!(
            ids(&q),
            vec!["a", "b", "c"],
            "unshuffled order includes enqueued items"
        );
    }

    #[test]
    fn remove_move_jump_clear() {
        let mut q = Queue::with_seed(9);
        q.load(items(4), 1, false);
        let qid = |q: &Queue, i: usize| q.entries()[i].qid;
        // remove before current shifts position
        let q0 = qid(&q, 0);
        assert_eq!(q.remove(q0), Some(false));
        assert_eq!(cur(&q), "1");
        // remove current: next becomes current
        let q1 = qid(&q, 0);
        assert_eq!(q.remove(q1), Some(true));
        assert_eq!(cur(&q), "2");
        assert_eq!(q.remove(9999), None);
        // remove last while current is last clamps
        let last = qid(&q, 1);
        q.jump(last);
        assert_eq!(q.remove(last), Some(true));
        assert_eq!(cur(&q), "2");

        q.load(items(4), 0, false);
        let moving = qid(&q, 0);
        assert!(q.move_to(moving, 3));
        assert_eq!(ids(&q), vec!["1", "2", "3", "0"]);
        assert_eq!(cur(&q), "0", "current follows the move");
        assert!(q.move_to(qid(&q, 0), 100));
        assert!(!q.move_to(12345, 0));
        assert!(!q.jump(12345));
        assert!(q.jump(qid(&q, 2)));
        q.clear();
        assert!(q.is_empty() && q.current().is_none());
    }

    #[test]
    fn item_helpers_and_serde_shape() {
        let mix = QueueItem::Mix {
            id: "m1".into(),
            title: "Friday".into(),
            duration: Some(3600.0),
        };
        assert_eq!(mix.title(), "Friday");
        assert_eq!(mix.duration(), Some(3600.0));
        let json = serde_json::to_value(QueueEntry {
            qid: 5,
            item: track("t1"),
        })
        .unwrap();
        assert_eq!(json["qid"], 5);
        assert_eq!(json["kind"], "track");
        assert_eq!(json["id"], "t1");
        assert_eq!(json["albumId"], serde_json::Value::Null);
        assert!(json.get("radioReason").is_none());
        let back: QueueItem = serde_json::from_value(
            serde_json::json!({"kind":"mix","id":"m","title":"T","duration":null}),
        )
        .unwrap();
        assert!(matches!(back, QueueItem::Mix { .. }));
        assert_eq!(serde_json::to_value(Repeat::All).unwrap(), "all");
        assert_eq!(xorshift(&mut 0), xorshift(&mut 1));
    }
}
