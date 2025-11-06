// storage/src/cache.rs

use blockchain_core::{Block, BlockNumber, Transaction, WorldState};
use blockchain_crypto::Hash;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

/// LRU cache for blocks
pub struct BlockCache {
    cache: Arc<RwLock<LruCache<Hash, Block>>>,
}

impl BlockCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(LruCache::new(capacity))),
        }
    }

    pub fn get(&self, hash: &Hash) -> Option<Block> {
        self.cache.write().unwrap().get(hash).cloned()
    }

    pub fn insert(&self, hash: Hash, block: Block) {
        self.cache.write().unwrap().insert(hash, block);
    }

    pub fn clear(&self) {
        self.cache.write().unwrap().clear();
    }
}

/// LRU cache for transactions
pub struct TransactionCache {
    cache: Arc<RwLock<LruCache<Hash, Transaction>>>,
}

impl TransactionCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(LruCache::new(capacity))),
        }
    }

    pub fn get(&self, hash: &Hash) -> Option<Transaction> {
        self.cache.write().unwrap().get(hash).cloned()
    }

    pub fn insert(&self, hash: Hash, tx: Transaction) {
        self.cache.write().unwrap().insert(hash, tx);
    }
}

/// State cache
pub struct StateCache {
    cache: Arc<RwLock<HashMap<BlockNumber, WorldState>>>,
    max_entries: usize,
}

impl StateCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            max_entries,
        }
    }

    pub fn get(&self, block_number: BlockNumber) -> Option<WorldState> {
        self.cache.read().unwrap().get(&block_number).cloned()
    }

    pub fn insert(&self, block_number: BlockNumber, state: WorldState) {
        let mut cache = self.cache.write().unwrap();
        
        // Remove oldest if at capacity
        if cache.len() >= self.max_entries {
            if let Some(min_key) = cache.keys().min().copied() {
                cache.remove(&min_key);
            }
        }
        
        cache.insert(block_number, state);
    }
}

/// Simple LRU cache implementation
struct LruCache<K, V> {
    map: HashMap<K, V>,
    order: VecDeque<K>,
    capacity: usize,
}

impl<K: Clone + std::hash::Hash + Eq, V> LruCache<K, V> {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn get(&mut self, key: &K) -> Option<&V> {
        if self.map.contains_key(key) {
            // Move to front
            self.order.retain(|k| k != key);
            self.order.push_front(key.clone());
            self.map.get(key)
        } else {
            None
        }
    }

    fn insert(&mut self, key: K, value: V) {
        if self.map.len() >= self.capacity && !self.map.contains_key(&key) {
            // Remove least recently used
            if let Some(old_key) = self.order.pop_back() {
                self.map.remove(&old_key);
            }
        }

        self.order.retain(|k| k != &key);
        self.order.push_front(key.clone());
        self.map.insert(key, value);
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache() {
        let mut cache = LruCache::new(2);
        
        cache.insert(1, "a");
        cache.insert(2, "b");
        assert_eq!(cache.get(&1), Some(&"a"));
        
        cache.insert(3, "c"); // Should evict 2
        assert_eq!(cache.get(&2), None);
        assert_eq!(cache.get(&1), Some(&"a"));
        assert_eq!(cache.get(&3), Some(&"c"));
    }

    #[test]
    fn test_block_cache() {
        let cache = BlockCache::new(10);
        let block = Block::genesis(Hash::zero());
        let hash = block.hash();
        
        cache.insert(hash, block.clone());
        let retrieved = cache.get(&hash).unwrap();
        
        assert_eq!(retrieved.hash(), hash);
    }
}