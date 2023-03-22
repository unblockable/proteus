pub struct Format {
    min_size: u64,
    max_size: u64
}

impl Format {
    pub fn new(min_size: u64, max_size: u64) -> Self {
        Self {min_size, max_size}
    }
}
