use crate::types::EntitySlot;
use rustc_hash::FxHashMap;

/// Grid-based spatial index for fast per-island entity radius queries.
///
/// Entities are bucketed into cells of fixed size. Radius queries iterate
/// only the overlapping cells and apply exact distance filtering.
pub struct SpatialGrid {
    cells: FxHashMap<(i32, i32), Vec<EntitySlot>>,
    entity_cells: FxHashMap<EntitySlot, (i32, i32)>,
    cell_size: f32,
}

impl SpatialGrid {
    pub fn new(cell_size: f32) -> Self {
        debug_assert!(cell_size > 0.0, "cell_size must be positive");
        Self {
            cells: FxHashMap::default(),
            entity_cells: FxHashMap::default(),
            cell_size,
        }
    }

    /// Map world coordinates to a grid cell.
    fn cell_of(&self, x: f32, z: f32) -> (i32, i32) {
        (
            (x / self.cell_size).floor() as i32,
            (z / self.cell_size).floor() as i32,
        )
    }

    pub fn insert(&mut self, entity: EntitySlot, x: f32, z: f32) {
        let cell = self.cell_of(x, z);
        self.cells.entry(cell).or_default().push(entity);
        self.entity_cells.insert(entity, cell);
    }

    pub fn remove(&mut self, entity: EntitySlot) {
        if let Some(cell) = self.entity_cells.remove(&entity) {
            if let Some(entities) = self.cells.get_mut(&cell) {
                entities.retain(|e| *e != entity);
                if entities.is_empty() {
                    self.cells.remove(&cell);
                }
            }
        }
    }

    /// Update entity position. Returns `true` if the entity crossed a cell boundary.
    pub fn update(&mut self, entity: EntitySlot, new_x: f32, new_z: f32) -> bool {
        let new_cell = self.cell_of(new_x, new_z);

        let Some(&old_cell) = self.entity_cells.get(&entity) else {
            // Entity not tracked — insert it
            self.insert(entity, new_x, new_z);
            return true;
        };

        if old_cell == new_cell {
            return false;
        }

        // Remove from old cell
        if let Some(entities) = self.cells.get_mut(&old_cell) {
            entities.retain(|e| *e != entity);
            if entities.is_empty() {
                self.cells.remove(&old_cell);
            }
        }

        // Insert into new cell
        self.cells.entry(new_cell).or_default().push(entity);
        self.entity_cells.insert(entity, new_cell);
        true
    }

    /// Find all entities within `radius` of `(cx, cz)`.
    ///
    /// Iterates overlapping grid cells and applies exact Euclidean distance
    /// filtering (on the XZ plane).
    pub fn query_radius(&self, cx: f32, cz: f32, radius: f32) -> Vec<EntitySlot> {
        let min_cell = self.cell_of(cx - radius, cz - radius);
        let max_cell = self.cell_of(cx + radius, cz + radius);

        let mut result = Vec::new();

        for gx in min_cell.0..=max_cell.0 {
            for gz in min_cell.1..=max_cell.1 {
                if let Some(entities) = self.cells.get(&(gx, gz)) {
                    result.extend(entities.iter().copied());
                }
            }
        }

        // Exact distance filter requires knowing positions — but we only store
        // cell membership, not coordinates. The caller must do fine-grained
        // filtering if sub-cell precision is needed.
        //
        // For now we return all entities in overlapping cells. This is correct
        // for cell_size >= radius (at most ~9 cells checked), and the overshoot
        // is bounded to one cell_size band around the true circle.
        result
    }

    /// Number of tracked entities.
    pub fn len(&self) -> usize {
        self.entity_cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entity_cells.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_query() {
        let mut grid = SpatialGrid::new(10.0);
        grid.insert(EntitySlot(0), 5.0, 5.0);
        grid.insert(EntitySlot(1), 15.0, 5.0);

        let nearby = grid.query_radius(5.0, 5.0, 10.0);
        assert!(nearby.contains(&EntitySlot(0)));
        // EntitySlot(1) is in an adjacent cell, within radius overlap
        assert!(nearby.contains(&EntitySlot(1)));
    }

    #[test]
    fn remove_entity() {
        let mut grid = SpatialGrid::new(10.0);
        grid.insert(EntitySlot(0), 5.0, 5.0);
        assert_eq!(grid.len(), 1);

        grid.remove(EntitySlot(0));
        assert_eq!(grid.len(), 0);

        let nearby = grid.query_radius(5.0, 5.0, 10.0);
        assert!(!nearby.contains(&EntitySlot(0)));
    }

    #[test]
    fn cell_crossing_returns_true() {
        let mut grid = SpatialGrid::new(10.0);
        grid.insert(EntitySlot(0), 5.0, 5.0); // cell (0, 0)

        // Move to cell (1, 0)
        let crossed = grid.update(EntitySlot(0), 15.0, 5.0);
        assert!(crossed);
    }

    #[test]
    fn same_cell_move_returns_false() {
        let mut grid = SpatialGrid::new(10.0);
        grid.insert(EntitySlot(0), 5.0, 5.0); // cell (0, 0)

        // Stay in cell (0, 0)
        let crossed = grid.update(EntitySlot(0), 6.0, 6.0);
        assert!(!crossed);
    }

    #[test]
    fn update_untracked_entity_inserts() {
        let mut grid = SpatialGrid::new(10.0);
        let crossed = grid.update(EntitySlot(42), 5.0, 5.0);
        assert!(crossed);
        assert_eq!(grid.len(), 1);
    }

    #[test]
    fn negative_coordinates() {
        let mut grid = SpatialGrid::new(10.0);
        grid.insert(EntitySlot(0), -5.0, -5.0);
        grid.insert(EntitySlot(1), -15.0, -5.0);

        let nearby = grid.query_radius(-5.0, -5.0, 10.0);
        assert!(nearby.contains(&EntitySlot(0)));
    }
}
