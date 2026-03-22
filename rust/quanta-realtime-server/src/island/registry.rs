use crate::island::handle::IslandHandle;
use crate::island::state_machine::IslandState;
use crate::types::IslandId;
use rustc_hash::FxHashMap;

pub struct IslandRegistry {
    islands: FxHashMap<String, IslandHandle>,
}

impl Default for IslandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl IslandRegistry {
    pub fn new() -> Self {
        Self {
            islands: FxHashMap::default(),
        }
    }

    pub fn insert(&mut self, handle: IslandHandle) {
        self.islands.insert(handle.island_id.0.clone(), handle);
    }

    pub fn remove(&mut self, id: &IslandId) -> Option<IslandHandle> {
        self.islands.remove(&id.0)
    }

    pub fn get(&self, id: &IslandId) -> Option<&IslandHandle> {
        self.islands.get(&id.0)
    }

    pub fn get_mut(&mut self, id: &IslandId) -> Option<&mut IslandHandle> {
        self.islands.get_mut(&id.0)
    }

    pub fn contains(&self, id: &IslandId) -> bool {
        self.islands.contains_key(&id.0)
    }

    pub fn total_entities(&self) -> u64 {
        self.islands.values().map(|h| h.entity_count as u64).sum()
    }

    pub fn active_count(&self) -> u32 {
        self.islands
            .values()
            .filter(|h| matches!(h.state, IslandState::Running | IslandState::Initializing))
            .count() as u32
    }

    pub fn len(&self) -> u32 {
        self.islands.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.islands.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::IslandCommand;
    use crate::island::handle::ThreadModel;

    fn make_handle(id: &str, entity_count: u32, state: IslandState) -> IslandHandle {
        let (tx, _rx) = crossbeam_channel::unbounded::<IslandCommand>();
        let (input_tx, _input_rx) =
            crossbeam_channel::unbounded::<crate::tick::ClientInput>();
        IslandHandle {
            island_id: IslandId::from(id),
            state,
            thread_model: ThreadModel::Pooled,
            entity_count,
            command_tx: tx,
            input_tx,
            join_handle: None,
        }
    }

    #[test]
    fn insert_and_get() {
        let mut reg = IslandRegistry::new();
        reg.insert(make_handle("a", 10, IslandState::Running));
        assert!(reg.get(&IslandId::from("a")).is_some());
        assert!(reg.contains(&IslandId::from("a")));
    }

    #[test]
    fn remove() {
        let mut reg = IslandRegistry::new();
        reg.insert(make_handle("a", 10, IslandState::Running));
        let h = reg.remove(&IslandId::from("a"));
        assert!(h.is_some());
        assert!(!reg.contains(&IslandId::from("a")));
    }

    #[test]
    fn total_entities() {
        let mut reg = IslandRegistry::new();
        reg.insert(make_handle("a", 10, IslandState::Running));
        reg.insert(make_handle("b", 20, IslandState::Initializing));
        assert_eq!(reg.total_entities(), 30);
    }

    #[test]
    fn active_count_excludes_stopped() {
        let mut reg = IslandRegistry::new();
        reg.insert(make_handle("a", 10, IslandState::Running));
        reg.insert(make_handle("b", 20, IslandState::Stopped));
        reg.insert(make_handle("c", 5, IslandState::Initializing));
        assert_eq!(reg.active_count(), 2);
    }

    #[test]
    fn len_and_is_empty() {
        let mut reg = IslandRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        reg.insert(make_handle("a", 1, IslandState::Running));
        assert!(!reg.is_empty());
        assert_eq!(reg.len(), 1);
    }
}
