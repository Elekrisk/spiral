pub struct Selection {
    pub start: usize,
    pub end: usize,
}

impl Selection {
    pub fn new() -> Self {
        Self { start: 0, end: 0 }
    }
}
