pub struct Fnv(pub u64);

impl Default for Fnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv {
    pub fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    pub fn bytes(&mut self, data: &[u8]) {
        for &b in data {
            self.0 = (self.0 ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }

    pub fn f32s(&mut self, data: &[f32]) {
        for v in data {
            self.bytes(&v.to_bits().to_le_bytes());
        }
    }

    pub fn f64s(&mut self, data: &[f64]) {
        for v in data {
            self.bytes(&v.to_bits().to_le_bytes());
        }
    }

    pub fn i32s(&mut self, data: &[i32]) {
        for v in data {
            self.bytes(&v.to_le_bytes());
        }
    }
}

pub fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

pub fn fixture_gray(name: &str) -> crate::img::Image {
    crate::img::decode(&fixture_bytes(name)).unwrap().1
}
