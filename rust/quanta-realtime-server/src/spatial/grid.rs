use super::position_table::PositionTable;
use crate::types::EntitySlot;
use rustc_hash::FxHashMap;

/// Maximum number of cells to scan in each dimension during `query_radius`.
/// Prevents runaway iteration from extreme radius values or NaN.
const MAX_CELL_SPAN: i32 = 100;

/// Grid-based spatial index for fast per-island entity radius queries.
///
/// Entities are bucketed into cells of fixed size. Use `query_radius` for
/// coarse cell-based lookups, or `query_radius_exact` with a `PositionTable`
/// for precise Euclidean distance filtering.
pub struct SpatialGrid {
    cells: FxHashMap<(i32, i32), Vec<EntitySlot>>,
    entity_cells: FxHashMap<EntitySlot, (i32, i32)>,
    cell_size: f32,
}

impl SpatialGrid {
    pub fn new(cell_size: f32) -> Self {
        assert!(
            cell_size > 0.0 && cell_size.is_finite(),
            "cell_size must be positive and finite, got {cell_size}"
        );
        Self {
            cells: FxHashMap::default(),
            entity_cells: FxHashMap::default(),
            cell_size,
        }
    }

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
            self.insert(entity, new_x, new_z);
            return true;
        };

        if old_cell == new_cell {
            return false;
        }

        if let Some(entities) = self.cells.get_mut(&old_cell) {
            entities.retain(|e| *e != entity);
            if entities.is_empty() {
                self.cells.remove(&old_cell);
            }
        }

        self.cells.entry(new_cell).or_default().push(entity);
        self.entity_cells.insert(entity, new_cell);
        true
    }

    /// Return all entities in cells overlapping the query circle.
    ///
    /// This is a coarse filter — results may include entities up to one
    /// `cell_size` beyond the actual radius. Use `query_radius_exact` for
    /// precise Euclidean filtering.
    pub fn query_radius(&self, cx: f32, cz: f32, radius: f32) -> Vec<EntitySlot> {
        let (min_cell, max_cell) = self.clamped_cell_range(cx, cz, radius);

        let mut result = Vec::new();
        for gx in min_cell.0..=max_cell.0 {
            for gz in min_cell.1..=max_cell.1 {
                if let Some(entities) = self.cells.get(&(gx, gz)) {
                    result.extend(entities.iter().copied());
                }
            }
        }

        result
    }

    /// Return all entities within exact Euclidean distance on the XZ plane.
    ///
    /// Uses `positions` for exact coordinates after coarse cell filtering.
    pub fn query_radius_exact(
        &self,
        cx: f32,
        cz: f32,
        radius: f32,
        positions: &PositionTable,
    ) -> Vec<EntitySlot> {
        let r2 = radius * radius;
        let candidates = self.query_radius(cx, cz, radius);

        candidates
            .into_iter()
            .filter(|&entity| {
                let (px, _, pz) = positions.get_position(entity);
                let dx = px - cx;
                let dz = pz - cz;
                dx * dx + dz * dz <= r2
            })
            .collect()
    }

    /// Compute the cell range for a query, clamped to prevent runaway iteration.
    fn clamped_cell_range(
        &self,
        cx: f32,
        cz: f32,
        radius: f32,
    ) -> ((i32, i32), (i32, i32)) {
        let min_cell = self.cell_of(cx - radius, cz - radius);
        let max_cell = self.cell_of(cx + radius, cz + radius);

        let clamp_span = |lo: i32, hi: i32| -> (i32, i32) {
            if hi.saturating_sub(lo) > MAX_CELL_SPAN {
                let mid = lo.saturating_add(hi.saturating_sub(lo) / 2);
                (
                    mid.saturating_sub(MAX_CELL_SPAN / 2),
                    mid.saturating_add(MAX_CELL_SPAN / 2),
                )
            } else {
                (lo, hi)
            }
        };

        let (x0, x1) = clamp_span(min_cell.0, max_cell.0);
        let (z0, z1) = clamp_span(min_cell.1, max_cell.1);
        ((x0, z0), (x1, z1))
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
        grid.insert(EntitySlot(0), 5.0, 5.0);
        assert!(grid.update(EntitySlot(0), 15.0, 5.0));
    }

    #[test]
    fn same_cell_move_returns_false() {
        let mut grid = SpatialGrid::new(10.0);
        grid.insert(EntitySlot(0), 5.0, 5.0);
        assert!(!grid.update(EntitySlot(0), 6.0, 6.0));
    }

    #[test]
    fn update_untracked_entity_inserts() {
        let mut grid = SpatialGrid::new(10.0);
        assert!(grid.update(EntitySlot(42), 5.0, 5.0));
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

    #[test]
    fn query_radius_exact_filters_by_distance() {
        let mut grid = SpatialGrid::new(10.0);
        let mut positions = PositionTable::new();

        // Entity at (5, 5) — distance 0 from query center
        grid.insert(EntitySlot(0), 5.0, 5.0);
        positions.ensure_capacity(EntitySlot(0));
        positions.set_position(EntitySlot(0), 5.0, 0.0, 5.0);

        // Entity at (15, 5) — distance 10 from query center
        grid.insert(EntitySlot(1), 15.0, 5.0);
        positions.ensure_capacity(EntitySlot(1));
        positions.set_position(EntitySlot(1), 15.0, 0.0, 5.0);

        // Query with radius 8 — should include (5,5) but not (15,5)
        let exact = grid.query_radius_exact(5.0, 5.0, 8.0, &positions);
        assert!(exact.contains(&EntitySlot(0)));
        assert!(!exact.contains(&EntitySlot(1)));

        // Coarse query would include both
        let coarse = grid.query_radius(5.0, 5.0, 8.0);
        assert!(coarse.contains(&EntitySlot(0)));
        assert!(coarse.contains(&EntitySlot(1)));
    }

    #[test]
    fn huge_radius_clamped() {
        let mut grid = SpatialGrid::new(10.0);
        grid.insert(EntitySlot(0), 5.0, 5.0);

        // Should not hang — range is clamped
        let result = grid.query_radius(0.0, 0.0, f32::MAX);
        // May or may not find the entity depending on clamp center,
        // but must complete without hanging
        let _ = result;
    }

    #[test]
    #[should_panic(expected = "cell_size must be positive and finite")]
    fn zero_cell_size_panics() {
        SpatialGrid::new(0.0);
    }

    #[test]
    #[should_panic(expected = "cell_size must be positive and finite")]
    fn nan_cell_size_panics() {
        SpatialGrid::new(f32::NAN);
    }
}
