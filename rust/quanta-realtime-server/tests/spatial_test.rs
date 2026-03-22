use quanta_realtime_server::spatial::{PositionTable, SpatialGrid};
use quanta_realtime_server::types::EntitySlot;

#[test]
fn insert_1000_entities_query_radius_exact() {
    let mut grid = SpatialGrid::new(50.0);
    let mut positions = PositionTable::new();

    for i in 0..1000u32 {
        let x = (i % 100) as f32 * 10.0;
        let z = (i / 100) as f32 * 10.0;
        grid.insert(EntitySlot(i), x, z);
        positions.ensure_capacity(EntitySlot(i));
        positions.set_position(EntitySlot(i), x, 0.0, z);
    }

    assert_eq!(grid.len(), 1000);

    let cx = 500.0f32;
    let cz = 50.0f32;
    let radius = 100.0f32;
    let results = grid.query_radius_exact(cx, cz, radius, &positions);

    // All returned entities must be within exact radius
    for &slot in &results {
        let (x, _, z) = positions.get_position(slot);
        let dx = x - cx;
        let dz = z - cz;
        let dist = (dx * dx + dz * dz).sqrt();
        assert!(
            dist <= radius,
            "Entity {:?} at ({x}, {z}) is {dist} from query center, exceeds radius {radius}",
            slot
        );
    }

    // All entities within radius must be included
    for i in 0..1000u32 {
        let (x, _, z) = positions.get_position(EntitySlot(i));
        let dx = x - cx;
        let dz = z - cz;
        let dist = (dx * dx + dz * dz).sqrt();
        if dist <= radius {
            assert!(
                results.contains(&EntitySlot(i)),
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
    grid.insert(EntitySlot(0), 5.0, 5.0);

    assert!(grid.update(EntitySlot(0), 15.0, 5.0));

    let old_results = grid.query_radius(5.0, 5.0, 1.0);
    assert!(!old_results.contains(&EntitySlot(0)));

    let new_results = grid.query_radius(15.0, 5.0, 1.0);
    assert!(new_results.contains(&EntitySlot(0)));
}

#[test]
fn grid_and_table_update_together() {
    let mut grid = SpatialGrid::new(10.0);
    let mut positions = PositionTable::new();

    positions.ensure_capacity(EntitySlot(0));
    positions.set_position(EntitySlot(0), 5.0, 0.0, 5.0);
    grid.insert(EntitySlot(0), 5.0, 5.0);

    // Move entity — update both table and grid
    positions.set_position(EntitySlot(0), 25.0, 0.0, 5.0);
    let crossed = grid.update(EntitySlot(0), 25.0, 5.0);
    assert!(crossed);

    // Exact query at old position shouldn't find it
    let results = grid.query_radius_exact(5.0, 5.0, 8.0, &positions);
    assert!(!results.contains(&EntitySlot(0)));

    // Exact query at new position should find it
    let results = grid.query_radius_exact(25.0, 5.0, 8.0, &positions);
    assert!(results.contains(&EntitySlot(0)));
}

#[test]
fn query_performance_10k_entities() {
    let mut grid = SpatialGrid::new(50.0);
    let mut positions = PositionTable::new();

    for i in 0..10_000u32 {
        let x = (i % 200) as f32 * 5.0;
        let z = (i / 200) as f32 * 5.0;
        grid.insert(EntitySlot(i), x, z);
        positions.ensure_capacity(EntitySlot(i));
        positions.set_position(EntitySlot(i), x, 0.0, z);
    }

    // Warm up
    let _ = grid.query_radius(500.0, 125.0, 100.0);

    let start = std::time::Instant::now();
    let iterations = 100;
    for _ in 0..iterations {
        let _ = grid.query_radius(500.0, 125.0, 100.0);
    }
    let elapsed = start.elapsed();
    let per_query = elapsed / iterations;

    // Spec target is <50us; allow 200us headroom for debug builds / CI
    assert!(
        per_query.as_micros() < 200,
        "Query took {per_query:?} per iteration, expected < 200us"
    );
}
