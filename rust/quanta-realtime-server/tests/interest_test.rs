use quanta_realtime_server::interest::{InterestConfig, InterestManager, LodTier};
use quanta_realtime_server::spatial::PositionTable;
use quanta_realtime_server::types::{ClientIndex, EntitySlot};

fn setup_positions(entities: &[(EntitySlot, f32, f32, f32)]) -> PositionTable {
    let mut pt = PositionTable::new();
    for &(slot, x, y, z) in entities {
        pt.ensure_capacity(slot);
        pt.set_position(slot, x, y, z);
    }
    pt
}

fn default_manager() -> InterestManager {
    InterestManager::new(InterestConfig::default(), 4, 64)
}

#[test]
fn visibility_by_distance() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities = [EntitySlot(0), EntitySlot(1)];
    let positions = setup_positions(&[
        (EntitySlot(0), 80.0, 0.0, 0.0),
        (EntitySlot(1), 160.0, 0.0, 0.0),
    ]);

    let results = mgr.update(0, &positions, &entities);
    let r = &results[0];

    assert!(r.enters.contains(&EntitySlot(0)));
    assert!(!r.enters.contains(&EntitySlot(1)));
}

#[test]
fn hysteresis_prevents_flicker() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities = [EntitySlot(0)];

    let positions = setup_positions(&[(EntitySlot(0), 99.0, 0.0, 0.0)]);
    let results = mgr.update(0, &positions, &entities);
    assert!(results[0].enters.contains(&EntitySlot(0)));

    let positions = setup_positions(&[(EntitySlot(0), 101.0, 0.0, 0.0)]);
    let results = mgr.update(1, &positions, &entities);
    assert!(!results[0].leaves.contains(&EntitySlot(0)));
    assert!(!results[0].enters.contains(&EntitySlot(0)));

    let positions = setup_positions(&[(EntitySlot(0), 99.0, 0.0, 0.0)]);
    let results = mgr.update(2, &positions, &entities);
    assert!(!results[0].enters.contains(&EntitySlot(0)));
}

#[test]
fn lod_tier_boundaries() {
    assert_eq!(LodTier::from_distance(0.0), LodTier::Full);
    assert_eq!(LodTier::from_distance(30.0), LodTier::Full);
    assert_eq!(LodTier::from_distance(30.1), LodTier::High);
    assert_eq!(LodTier::from_distance(70.0), LodTier::High);
    assert_eq!(LodTier::from_distance(70.1), LodTier::Medium);
    assert_eq!(LodTier::from_distance(100.0), LodTier::Medium);
    assert_eq!(LodTier::from_distance(100.1), LodTier::Low);
    assert_eq!(LodTier::from_distance(150.0), LodTier::Low);
}

#[test]
fn tick_divisor_filters_medium_tier() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities = [EntitySlot(0)];
    let positions = setup_positions(&[(EntitySlot(0), 85.0, 0.0, 0.0)]);

    let results = mgr.update(0, &positions, &entities);
    assert!(results[0].sends.iter().any(|pe| pe.entity == EntitySlot(0)));

    let results = mgr.update(1, &positions, &entities);
    assert!(!results[0].sends.iter().any(|pe| pe.entity == EntitySlot(0)));

    let results = mgr.update(2, &positions, &entities);
    assert!(!results[0].sends.iter().any(|pe| pe.entity == EntitySlot(0)));

    let results = mgr.update(4, &positions, &entities);
    assert!(results[0].sends.iter().any(|pe| pe.entity == EntitySlot(0)));
}

#[test]
fn priority_convergence() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities = [EntitySlot(0), EntitySlot(1)];
    let positions = setup_positions(&[
        (EntitySlot(0), 10.0, 0.0, 0.0),
        (EntitySlot(1), 90.0, 0.0, 0.0),
    ]);

    for tick in 1..8 {
        mgr.update(tick, &positions, &entities);
    }

    let results = mgr.update(8, &positions, &entities);
    let sends = &results[0].sends;
    assert!(sends.iter().any(|pe| pe.entity == EntitySlot(1)));
}

#[test]
fn batch_enter_threshold() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities: Vec<EntitySlot> = (0..6).map(EntitySlot).collect();
    let pos_data: Vec<(EntitySlot, f32, f32, f32)> = (0..6)
        .map(|i| (EntitySlot(i), 10.0 + i as f32, 0.0, 0.0))
        .collect();
    let positions = setup_positions(&pos_data);

    let results = mgr.update(0, &positions, &entities);
    assert_eq!(results[0].enters.len(), 6);
    assert!(results[0].batch_enters);
}

#[test]
fn no_batch_enter_below_threshold() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities: Vec<EntitySlot> = (0..4).map(EntitySlot).collect();
    let pos_data: Vec<(EntitySlot, f32, f32, f32)> = (0..4)
        .map(|i| (EntitySlot(i), 10.0 + i as f32, 0.0, 0.0))
        .collect();
    let positions = setup_positions(&pos_data);

    let results = mgr.update(0, &positions, &entities);
    assert_eq!(results[0].enters.len(), 4);
    assert!(!results[0].batch_enters);
}

#[test]
fn leave_repeated_three_ticks() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities = [EntitySlot(0)];

    let positions = setup_positions(&[(EntitySlot(0), 50.0, 0.0, 0.0)]);
    mgr.update(0, &positions, &entities);

    let positions = setup_positions(&[(EntitySlot(0), 200.0, 0.0, 0.0)]);

    for tick in 1..=3 {
        let results = mgr.update(tick, &positions, &entities);
        assert!(
            results[0].leaves.contains(&EntitySlot(0)),
            "leave should repeat on tick {tick}"
        );
    }

    let results = mgr.update(4, &positions, &entities);
    assert!(!results[0].leaves.contains(&EntitySlot(0)));
}

#[test]
fn priority_factor_distance() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities = [EntitySlot(0), EntitySlot(1)];
    let positions = setup_positions(&[
        (EntitySlot(0), 10.0, 0.0, 0.0),
        (EntitySlot(1), 90.0, 0.0, 0.0),
    ]);

    let results = mgr.update(0, &positions, &entities);
    let sends = &results[0].sends;

    let p0 = sends.iter().find(|pe| pe.entity == EntitySlot(0)).unwrap();
    let p1 = sends.iter().find(|pe| pe.entity == EntitySlot(1)).unwrap();
    assert!(p0.priority > p1.priority);
}

#[test]
fn priority_factor_velocity() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities = [EntitySlot(0), EntitySlot(1)];
    let mut positions = setup_positions(&[
        (EntitySlot(0), 20.0, 0.0, 0.0),
        (EntitySlot(1), 20.0, 0.0, 0.0),
    ]);
    positions.set_velocity(EntitySlot(1), 10.0, 0.0, 0.0);

    let results = mgr.update(0, &positions, &entities);
    let sends = &results[0].sends;

    let p0 = sends.iter().find(|pe| pe.entity == EntitySlot(0)).unwrap();
    let p1 = sends.iter().find(|pe| pe.entity == EntitySlot(1)).unwrap();
    assert!(p1.priority > p0.priority);
}

#[test]
fn priority_factor_interaction() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities = [EntitySlot(0), EntitySlot(1)];
    let positions = setup_positions(&[
        (EntitySlot(0), 20.0, 0.0, 0.0),
        (EntitySlot(1), 20.0, 0.0, 0.0),
    ]);

    mgr.record_interaction(ClientIndex(0), EntitySlot(1));

    let results = mgr.update(0, &positions, &entities);
    let sends = &results[0].sends;

    let p0 = sends.iter().find(|pe| pe.entity == EntitySlot(0)).unwrap();
    let p1 = sends.iter().find(|pe| pe.entity == EntitySlot(1)).unwrap();
    assert!(p1.priority > p0.priority);
}

#[test]
fn unregister_client_removes_results() {
    let mut mgr = default_manager();
    mgr.register_client(ClientIndex(0), 0.0, 0.0);

    let entities = [EntitySlot(0)];
    let positions = setup_positions(&[(EntitySlot(0), 50.0, 0.0, 0.0)]);

    let results = mgr.update(0, &positions, &entities);
    assert_eq!(results.len(), 1);

    mgr.unregister_client(ClientIndex(0));
    let results = mgr.update(1, &positions, &entities);
    assert!(results.is_empty());
}
