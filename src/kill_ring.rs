pub struct KillRing {
    pub entries: Vec<KillRingEntry>,
}

impl KillRing {
    pub fn new() -> Self {
        Self { entries: vec![] }
    }

    pub fn add_entry(&mut self, entry: KillRingEntry) {
        self.entries.push(entry);
    }

    pub fn get(&self) -> Option<&KillRingEntry> {
        self.entries.last()
    }

    pub fn rotate_forward(&mut self) {
        if let Some(x) = self.entries.pop() {
            self.entries.insert(0, x);
        }
    }

    pub fn rotate_backward(&mut self) {
        if !self.entries.is_empty() {
            let x = self.entries.remove(0);
            self.entries.push(x);
        }
    }
}

pub struct KillRingEntry {
    pub text: Vec<String>,
}

impl KillRingEntry {
    pub fn new<S: Into<String>, I: IntoIterator<Item = S>>(items: I) -> Self {
        Self {
            text: items.into_iter().map(Into::into).collect(),
        }
    }

    pub fn get_for_cursor_count(&self, count: usize) -> Vec<&str> {
        self.text
            .iter()
            .map(String::as_str)
            .chain(std::iter::from_fn(|| {
                Some(self.text.last().map(String::as_str).unwrap_or(""))
            }))
            .take(count)
            .collect()
    }
}
