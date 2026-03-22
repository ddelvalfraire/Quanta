mod common;

use common::*;
use quanta_realtime_server::config::ServerConfig;

#[tokio::test]
async fn get_metrics_reflects_mutations() {
    let tx = spawn_manager(ServerConfig::default());

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 0);
    assert_eq!(m.total_entities, 0);

    activate(&tx, test_manifest("a", 30)).await.unwrap();
    activate(&tx, test_manifest("b", 70)).await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 2);
    assert_eq!(m.total_islands, 2);
    assert_eq!(m.total_entities, 100);

    drain(&tx, "a").await.unwrap();

    let m = get_metrics(&tx).await;
    assert_eq!(m.active_islands, 1);
    assert_eq!(m.total_islands, 1);
    assert_eq!(m.total_entities, 70);

    stop(&tx, "b").await.unwrap();
}
