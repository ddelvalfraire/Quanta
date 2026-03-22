use crate::types::EntitySlot;

/// Maximum supported entity slots. Prevents accidental OOM from huge slot values.
const MAX_ENTITIES: u32 = 65_536;

/// SoA (Structure of Arrays) position and velocity table for cache-friendly access.
///
/// Positions alone are 12 bytes per entity (3 x f32) — 24KB for 2000 entities
/// fits L1 cache. With velocity the full table is 24 bytes per entity (~47KB
/// for 2000 entities).
pub struct PositionTable {
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub z: Vec<f32>,
    pub vx: Vec<f32>,
    pub vy: Vec<f32>,
    pub vz: Vec<f32>,
}

impl PositionTable {
    pub fn new() -> Self {
        Self {
            x: Vec::new(),
            y: Vec::new(),
            z: Vec::new(),
            vx: Vec::new(),
            vy: Vec::new(),
            vz: Vec::new(),
        }
    }

    /// Grow all vectors so that `slot.0` is a valid index.
    ///
    /// Panics if `slot.0 >= MAX_ENTITIES` (65536).
    pub fn ensure_capacity(&mut self, slot: EntitySlot) {
        assert!(
            slot.0 < MAX_ENTITIES,
            "EntitySlot({}) exceeds MAX_ENTITIES ({})",
            slot.0,
            MAX_ENTITIES
        );
        let needed = slot.0 as usize + 1;
        if self.x.len() < needed {
            self.x.resize(needed, 0.0);
            self.y.resize(needed, 0.0);
            self.z.resize(needed, 0.0);
            self.vx.resize(needed, 0.0);
            self.vy.resize(needed, 0.0);
            self.vz.resize(needed, 0.0);
        }
    }

    pub fn set_position(&mut self, slot: EntitySlot, x: f32, y: f32, z: f32) {
        let i = slot.0 as usize;
        self.x[i] = x;
        self.y[i] = y;
        self.z[i] = z;
    }

    pub fn set_velocity(&mut self, slot: EntitySlot, vx: f32, vy: f32, vz: f32) {
        let i = slot.0 as usize;
        self.vx[i] = vx;
        self.vy[i] = vy;
        self.vz[i] = vz;
    }

    pub fn get_position(&self, slot: EntitySlot) -> (f32, f32, f32) {
        let i = slot.0 as usize;
        (self.x[i], self.y[i], self.z[i])
    }

    pub fn get_velocity(&self, slot: EntitySlot) -> (f32, f32, f32) {
        let i = slot.0 as usize;
        (self.vx[i], self.vy[i], self.vz[i])
    }

    /// Zero out position and velocity for a removed entity.
    pub fn clear(&mut self, slot: EntitySlot) {
        let i = slot.0 as usize;
        if i < self.x.len() {
            self.x[i] = 0.0;
            self.y[i] = 0.0;
            self.z[i] = 0.0;
            self.vx[i] = 0.0;
            self.vy[i] = 0.0;
            self.vz[i] = 0.0;
        }
    }
}

impl Default for PositionTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_position() {
        let mut table = PositionTable::new();
        table.ensure_capacity(EntitySlot(5));
        table.set_position(EntitySlot(5), 1.0, 2.0, 3.0);

        assert_eq!(table.get_position(EntitySlot(5)), (1.0, 2.0, 3.0));
    }

    #[test]
    fn set_get_velocity() {
        let mut table = PositionTable::new();
        table.ensure_capacity(EntitySlot(0));
        table.set_velocity(EntitySlot(0), 10.0, -5.0, 0.5);

        assert_eq!(table.get_velocity(EntitySlot(0)), (10.0, -5.0, 0.5));
    }

    #[test]
    fn clear_zeroes_out() {
        let mut table = PositionTable::new();
        table.ensure_capacity(EntitySlot(0));
        table.set_position(EntitySlot(0), 1.0, 2.0, 3.0);
        table.set_velocity(EntitySlot(0), 4.0, 5.0, 6.0);

        table.clear(EntitySlot(0));
        assert_eq!(table.get_position(EntitySlot(0)), (0.0, 0.0, 0.0));
        assert_eq!(table.get_velocity(EntitySlot(0)), (0.0, 0.0, 0.0));
    }

    #[test]
    fn ensure_capacity_grows() {
        let mut table = PositionTable::new();
        assert_eq!(table.x.len(), 0);

        table.ensure_capacity(EntitySlot(99));
        assert_eq!(table.x.len(), 100);
        assert_eq!(table.vz.len(), 100);

        // Calling with a smaller slot doesn't shrink
        table.ensure_capacity(EntitySlot(10));
        assert_eq!(table.x.len(), 100);
    }

    #[test]
    #[should_panic(expected = "exceeds MAX_ENTITIES")]
    fn ensure_capacity_rejects_huge_slot() {
        let mut table = PositionTable::new();
        table.ensure_capacity(EntitySlot(MAX_ENTITIES));
    }

    #[test]
    fn soa_contiguity() {
        let mut table = PositionTable::new();
        table.ensure_capacity(EntitySlot(9));

        for i in 0..9 {
            unsafe {
                assert_eq!(
                    table.x.as_ptr().add(i + 1),
                    &table.x[i + 1] as *const f32,
                    "x array not contiguous at index {i}"
                );
            }
        }
    }
}
