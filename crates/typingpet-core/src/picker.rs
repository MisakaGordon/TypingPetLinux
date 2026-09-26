//! 随机挑图（与 macOS 版 `ImagePicker.swift` 行为一致：排除当前图，单图时返回自身）。

pub trait Rng {
    fn next_u64(&mut self) -> u64;

    /// 返回 `0..len` 内的索引；`len == 0` 返回 0。
    fn index(&mut self, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        (self.next_u64() % len as u64) as usize
    }
}

/// 固定返回同一个索引，便于测试。
#[derive(Debug, Clone, Copy)]
pub struct FixedRng(pub usize);

impl Rng for FixedRng {
    fn next_u64(&mut self) -> u64 {
        self.0 as u64
    }

    fn index(&mut self, len: usize) -> usize {
        self.0.min(len.saturating_sub(1))
    }
}

/// 无依赖的 xorshift64* 随机数，用系统时间做种子。
pub struct SystemRng {
    state: u64,
}

impl SystemRng {
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self::from_seed(nanos ^ (std::process::id() as u64) << 32)
    }

    pub fn from_seed(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x2545_F491_4F6C_DD1D } else { seed },
        }
    }
}

impl Default for SystemRng {
    fn default() -> Self {
        Self::new()
    }
}

impl Rng for SystemRng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImagePicker {
    pub names: Vec<String>,
}

impl ImagePicker {
    pub fn new(names: Vec<String>) -> Self {
        Self { names }
    }

    pub fn next_excluding(&self, current: Option<&str>, rng: &mut impl Rng) -> Option<String> {
        self.next_excluding_with(current, |len| rng.index(len))
    }

    pub fn next_excluding_with<F>(
        &self,
        current: Option<&str>,
        mut random_index: F,
    ) -> Option<String>
    where
        F: FnMut(usize) -> usize,
    {
        let choices: Vec<&String> = self
            .names
            .iter()
            .filter(|name| match current {
                Some(current) => name.as_str() != current,
                None => true,
            })
            .collect();

        if choices.is_empty() {
            return self.names.first().cloned();
        }

        let index = random_index(choices.len()).min(choices.len() - 1);
        Some(choices[index].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 移植自 Tests/TypingPetTests/ImagePickerTests.swift
    #[test]
    fn picker_excludes_current_image() {
        let picker = ImagePicker::new(vec![
            "left".to_string(),
            "right".to_string(),
            "question".to_string(),
        ]);
        let result = picker.next_excluding_with(Some("left"), |_| 0);
        assert_eq!(result.as_deref(), Some("right"));
    }

    #[test]
    fn picker_returns_only_image_when_no_alternative_exists() {
        let picker = ImagePicker::new(vec!["left".to_string()]);
        let result = picker.next_excluding_with(Some("left"), |_| 0);
        assert_eq!(result.as_deref(), Some("left"));
    }

    #[test]
    fn empty_picker_returns_none() {
        let picker = ImagePicker::new(Vec::new());
        assert_eq!(picker.next_excluding_with(None, |_| 0), None);
    }

    #[test]
    fn picker_never_returns_current_when_alternatives_exist() {
        let picker = ImagePicker::new(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ]);
        let mut rng = SystemRng::from_seed(42);
        for _ in 0..200 {
            let picked = picker.next_excluding(Some("b"), &mut rng).expect("picked");
            assert_ne!(picked, "b");
        }
    }
}
