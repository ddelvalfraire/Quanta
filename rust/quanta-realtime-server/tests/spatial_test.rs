use quanta_realtime_server::spatial::{PositionTable, SpatialGrid};
use quanta_realtime_server::types::EntitySlot;

#[test]
fn insert_1000_entities_query_radius() {
    let mut grid = SpatialGrid::new(50.0);
    let mut positions = Vec::new();

    // Insert 1000 entities in a 1000x1000 area using deterministic positions
    for i in 0..1000u32 {
        // Simple deterministic spread
        let x = (i % 100) as f32 * 10.0;
        let z = (i / 100) as f32 * 10.0;
        grid.insert(EntitySlot(i), x, z);
        positions.push((x, z));
    }

    assert_eq!(grid.len(), 1000);

    // Query around center (500, 50) with radius 100
    let cx = 500.0f32;
    let cz = 50.0f32;
    let radius = 100.0f32;
    let results = grid.query_radius(cx, cz, radius);

    // Verify all returned entities are plausibly near the query center
    // (within radius + cell_size since we only filter by cell overlap)
    let max_dist = radius + 50.0; // one cell_size of overshoot
    for &slot in &results {
        let (x, z) = positions[slot.0 as usize];
        let dx = x - cx;
        let dz = z - cz;
        let dist = (dx * dx + dz * dz).sqrt();
        assert!(
            dist <= max_dist,
            "Entity {:?} at ({x}, {z}) is {dist} from query center, exceeds {max_dist}",
            slot
        );
    }

    // Verify entities that should definitely be in range are included
    for (i, &(x, z)) in positions.iter().enumerate() {
        let dx = x - cx;
        let dz = z - cz;
        let dist = (dx * dx + dz * dz).sqrt();
        if dist <= radius - 50.0 {
            // Well within radius minus one cell
            assert!(
                results.contains(&EntitySlot(i as u32)),
                "Entity {i} at ({x}, {z}) should be in results (dist={dist})"
            );
        }
    }
}

#[test]
fn remove_entity_not_returned() {
    let mut grid = SpatialGrid::new(10.0);
    grid.insert(EntitySlot(0), 5.0, 5.0);
    grid.insert(EntitySlot(1), 6.0, 6.0);

    grid.remove(EntitySlot(0));

    let results = grid.query_radius(5.0, 5.0, 20.0);
    assert!(!results.contains(&EntitySlot(0)));
    assert!(results.contains(&EntitySlot(1)));
}

#[test]
fn cell_crossing_update_returns_true() {
    let mut grid = SpatialGrid::new(10.0);
    grid.insert(EntitySlot(0), 5.0, 5.0); // cell (0, 0)

    // Move across cell boundary to cell (1, 0)
    assert!(grid.update(EntitySlot(0), 15.0, 5.0));

    // Query old cell — should not contain entity
    let old_results = grid.query_radius(5.0, 5.0, 1.0);
    assert!(!old_results.contains(&EntitySlot(0)));

    // Query new cell — should contain entity
    let new_results = grid.query_radius(15.0, 5.0, 1.0);
    assert!(new_results.contains(&EntitySlot(0)));
}

#[test]
fn same_cell_move_returns_false() {
    let mut grid = SpatialGrid::new(10.0);
    grid.insert(EntitySlot(0), 5.0, 5.0); // cell (0, 0)

    // Small move within same cell
    assert!(!grid.update(EntitySlot(0), 6.0, 7.0)); // still cell (0, 0)
}

#[test]
fn soa_contiguity() {
    let mut table = PositionTable::new();
    table.ensure_capacity(EntitySlot(99));

    // Verify Vec elements are contiguous via pointer arithmetic
    for i in 0..99 {
        unsafe {
            assert_eq!(
                table.x.as_ptr().add(i + 1),
                &table.x[i + 1] as *const f32
            );
        }
    }
}

#[test]
fn query_performance_10k_entities() {
    let mut grid = SpatialGrid::new(50.0);
    let mut table = PositionTable::new();

    // Insert 10,000 entities spread across a large area
    for i in 0..10_000u32 {
        let x = (i % 200) as f32 * 5.0;
        let z = (i / 200) as f32 * 5.0;
        grid.insert(EntitySlot(i), x, z);
        table.ensure_capacity(EntitySlot(i));
        table.set_position(EntitySlot(i), x, 0.0, z);
    }

    // Warm up
    let _ = grid.query_radius(500.0, 125.0, 100.0);

    // Measure query time over 100 iterations
    let start = std::time::Instant::now();
    let iterations = 100;
    for _ in 0..iterations {
        let _ = grid.query_radius(500.0, 125.0, 100.0);
    }
    let elapsed = start.elapsed();
    let per_query = elapsed / iterations;

    assert!(
        per_query.as_micros() < 500,
        "Query took {per_query:?} per iteration, expected < 500us"
    );
}

#[test]
fn position_table_roundtrip() {
    let mut table = PositionTable::new();
    table.ensure_capacity(EntitySlot(10));

    table.set_position(EntitySlot(3), 1.0, 2.0, 3.0);
    table.set_velocity(EntitySlot(3), 4.0, 5.0, 6.0);

    assert_eq!(table.get_position(EntitySlot(3)), (1.0, 2.0, 3.0));
    assert_eq!(table.get_velocity(EntitySlot(3)), (4.0, 5.0, 6.0));

    table.clear(EntitySlot(3));
    assert_eq!(table.get_position(EntitySlot(3)), (0.0, 0.0, 0.0));
    assert_eq!(table.get_velocity(EntitySlot(3)), (0.0, 0.0, 0.0));
}
