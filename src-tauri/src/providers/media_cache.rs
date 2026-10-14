use super::ProviderLeafNode;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

type Key = (String, String);
struct Entry {
    nodes: Arc<Vec<ProviderLeafNode>>,
    created: Instant,
    used: Instant,
    bytes: usize,
}

/// Only retains recent URL trees. Cache hits share nodes instead of cloning whole works.
pub(super) struct MediaCache {
    entries: HashMap<Key, Entry>,
    max_entries: usize,
    max_bytes: usize,
    ttl: Duration,
}

impl Default for MediaCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            max_entries: 32,
            max_bytes: 16 * 1024 * 1024,
            ttl: Duration::from_secs(15 * 60),
        }
    }
}

impl MediaCache {
    fn prune(&mut self, now: Instant) {
        self.entries
            .retain(|_, entry| now.duration_since(entry.created) < self.ttl);
    }

    pub fn get(&mut self, key: &Key) -> Option<Arc<Vec<ProviderLeafNode>>> {
        let now = Instant::now();
        self.prune(now);
        self.entries.get_mut(key).map(|entry| {
            entry.used = now;
            entry.nodes.clone()
        })
    }

    pub fn insert(&mut self, key: Key, nodes: Arc<Vec<ProviderLeafNode>>) {
        let now = Instant::now();
        self.prune(now);
        self.entries.remove(&key);
        let bytes = key.0.capacity()
            + key.1.capacity()
            + nodes.capacity() * std::mem::size_of::<ProviderLeafNode>()
            + nodes
                .iter()
                .map(|n| {
                    n.node_type.capacity()
                        + n.title.capacity()
                        + n.logical_dir_path.capacity()
                        + n.relative_path.capacity()
                        + n.remote_file_id.capacity()
                        + n.format.capacity()
                        + n.stream_url.as_ref().map_or(0, String::capacity)
                        + n.download_url.as_ref().map_or(0, String::capacity)
                })
                .sum::<usize>();
        // Large works remain playable but are not retained in the cache.
        if bytes > self.max_bytes || self.max_entries == 0 {
            return;
        }
        while self.entries.len() >= self.max_entries
            || self.entries.values().map(|e| e.bytes).sum::<usize>() + bytes > self.max_bytes
        {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.entries.insert(
            key,
            Entry {
                nodes,
                created: now,
                used: now,
                bytes,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn nodes() -> Arc<Vec<ProviderLeafNode>> {
        Arc::new(vec![ProviderLeafNode {
            node_type: "audio".into(),
            title: "track".into(),
            logical_dir_path: String::new(),
            relative_path: "track.mp3".into(),
            remote_file_id: "1".into(),
            format: "mp3".into(),
            duration_ms: None,
            byte_size: None,
            stream_url: Some("https://example.com/track.mp3".into()),
            download_url: None,
        }])
    }
    fn key(id: usize) -> Key {
        ("asmr_one".into(), id.to_string())
    }

    #[test]
    fn thousands_of_works_stay_bounded_and_hits_do_not_copy_trees() {
        let mut cache = MediaCache::default();
        let first = nodes();
        let weak = Arc::downgrade(&first);
        cache.insert(key(0), first.clone());
        assert!(Arc::ptr_eq(&first, &cache.get(&key(0)).unwrap()));
        drop(first);
        for id in 1..5000 {
            cache.insert(key(id), nodes());
        }
        assert_eq!(cache.entries.len(), 32);
        assert!(cache.entries.values().map(|e| e.bytes).sum::<usize>() <= cache.max_bytes);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn expiry_budget_and_recent_use_evict_the_right_entries() {
        let mut cache = MediaCache {
            max_entries: 2,
            ..Default::default()
        };
        cache.insert(key(1), nodes());
        cache.insert(key(2), nodes());
        cache.entries.get_mut(&key(1)).unwrap().used -= Duration::from_secs(1);
        cache.insert(key(3), nodes());
        assert!(cache.get(&key(1)).is_none());
        cache.entries.get_mut(&key(2)).unwrap().created -= Duration::from_secs(901);
        assert!(cache.get(&key(2)).is_none());
        assert_eq!(cache.entries.len(), 1);
        cache.max_bytes = 1;
        cache.insert(key(4), nodes());
        assert!(cache.get(&key(4)).is_none());
    }
}
