/// Commands sent from the manager to an island thread via crossbeam.
#[derive(Debug)]
pub enum IslandCommand {
    /// Tick the simulation (placeholder for T45).
    Tick,
    /// Begin draining: finish current tick, then stop.
    Drain,
    /// Stop immediately.
    Stop,
}
