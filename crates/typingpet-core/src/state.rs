//! 应用状态机：按键 → 挑图 → 弹跳 → 0.75s 回待机。
//!
//! 这一层刻意不依赖任何 GUI/输入实现，便于 headless 全链路验证。

use crate::keys::{KeyReactionStore, KeyStroke};
use crate::picker::{ImagePicker, Rng};
use std::path::PathBuf;

pub const IDLE_DELAY: f64 = 0.75;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// 显示某张图片
    ShowImage(PathBuf),
    /// 播放弹跳动画，参数为振幅（像素）
    Bounce(f64),
}

#[derive(Debug)]
pub struct PetState {
    idle_url: Option<PathBuf>,
    reaction_urls: Vec<PathBuf>,
    current_image: Option<PathBuf>,
    idle_deadline: Option<f64>,
    shake_amplitude: f64,
    reaction_count: usize,
}

impl PetState {
    pub fn new(
        idle_url: Option<PathBuf>,
        reaction_urls: Vec<PathBuf>,
        shake_amplitude: f64,
    ) -> Self {
        Self {
            reaction_count: reaction_urls.len(),
            idle_url,
            reaction_urls,
            current_image: None,
            idle_deadline: None,
            shake_amplitude,
        }
    }

    pub fn update_sources(
        &mut self,
        idle_url: Option<PathBuf>,
        reaction_urls: Vec<PathBuf>,
        shake_amplitude: f64,
    ) {
        self.reaction_count = reaction_urls.len();
        self.idle_url = idle_url;
        self.reaction_urls = reaction_urls;
        self.shake_amplitude = shake_amplitude;
        self.current_image = None;
        self.idle_deadline = None;
    }

    pub fn current_image(&self) -> Option<&PathBuf> {
        self.current_image.as_ref()
    }

    pub fn reaction_count(&self) -> usize {
        self.reaction_count
    }

    pub fn is_showing_idle(&self) -> bool {
        match (&self.current_image, &self.idle_url) {
            (Some(current), Some(idle)) => current == idle,
            (None, _) => true,
            _ => false,
        }
    }

    /// 待机图 URL（用于启动时显示）。
    pub fn idle_url(&self) -> Option<&PathBuf> {
        self.idle_url.as_ref()
    }

    /// 首次显示：返回待机图动作。
    pub fn initial_actions(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        if let Some(idle) = self.idle_url.clone() {
            self.current_image = Some(idle.clone());
            actions.push(Action::ShowImage(idle));
        }
        actions
    }

    /// 键输入处理。`stroke` 为 `None` 时表示"测试按钮"（跳过精确规则匹配）。
    pub fn on_key(
        &mut self,
        stroke: Option<KeyStroke>,
        rules: &KeyReactionStore,
        rng: &mut impl Rng,
        now: f64,
    ) -> Vec<Action> {
        let chosen = match stroke {
            Some(stroke) => match rules.image_path(stroke) {
                Some(path) => Some(path),
                None => self.pick_random(rng),
            },
            None => self.pick_random(rng),
        };

        let Some(chosen) = chosen else {
            return Vec::new();
        };

        let mut actions = Vec::new();
        self.current_image = Some(chosen.clone());
        actions.push(Action::ShowImage(chosen));
        if self.shake_amplitude > 0.0 {
            actions.push(Action::Bounce(self.shake_amplitude));
        }
        self.idle_deadline = Some(now + IDLE_DELAY);
        actions
    }

    /// 定时推进：超时后回待机。
    pub fn tick(&mut self, now: f64) -> Vec<Action> {
        let Some(deadline) = self.idle_deadline else {
            return Vec::new();
        };
        if now < deadline {
            return Vec::new();
        }
        self.idle_deadline = None;

        let Some(idle) = self.idle_url.clone() else {
            return Vec::new();
        };
        if self.current_image.as_ref() == Some(&idle) {
            return Vec::new();
        }
        self.current_image = Some(idle.clone());
        vec![Action::ShowImage(idle)]
    }

    fn pick_random(&self, rng: &mut impl Rng) -> Option<PathBuf> {
        let names: Vec<String> = self
            .reaction_urls
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();
        let picker = ImagePicker::new(names);
        let current_key = self
            .current_image
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());
        picker
            .next_excluding(current_key.as_deref(), rng)
            .map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picker::FixedRng;

    fn state() -> PetState {
        PetState::new(
            Some(PathBuf::from("/img/pet-idle.png")),
            vec![
                PathBuf::from("/img/pet-left.png"),
                PathBuf::from("/img/pet-right.png"),
                PathBuf::from("/img/pet-question.png"),
            ],
            9.0,
        )
    }

    #[test]
    fn initial_actions_show_idle_image() {
        let mut state = state();
        assert_eq!(
            state.initial_actions(),
            vec![Action::ShowImage(PathBuf::from("/img/pet-idle.png"))]
        );
        assert!(state.is_showing_idle());
    }

    #[test]
    fn keypress_shows_reaction_then_returns_to_idle() {
        let mut state = state();
        state.initial_actions();
        let rules = KeyReactionStore::new("/nonexistent-rules");
        let mut rng = FixedRng(0);

        let actions = state.on_key(None, &rules, &mut rng, 100.0);
        assert_eq!(actions.len(), 2);
        assert!(matches!(actions[0], Action::ShowImage(_)));
        assert_eq!(actions[1], Action::Bounce(9.0));
        assert!(!state.is_showing_idle());

        // 0.75s 之前不回待机
        assert!(state.tick(100.7).is_empty());
        // 到点后回待机
        assert_eq!(
            state.tick(100.76),
            vec![Action::ShowImage(PathBuf::from("/img/pet-idle.png"))]
        );
        assert!(state.is_showing_idle());
        // 已回待机后不再重复
        assert!(state.tick(101.0).is_empty());
    }

    #[test]
    fn reaction_never_repeats_the_image_shown_right_before() {
        let mut state = state();
        state.initial_actions();
        let rules = KeyReactionStore::new("/nonexistent-rules");
        let mut rng = FixedRng(0);
        let mut previous = state.current_image().cloned();

        for _ in 0..50 {
            let actions = state.on_key(None, &rules, &mut rng, 200.0);
            let shown = match actions.first() {
                Some(Action::ShowImage(path)) => path.clone(),
                _ => panic!("expected ShowImage"),
            };
            assert_ne!(Some(shown.clone()), previous, "连续两次显示了同一张反应图");
            previous = Some(shown);
        }
    }

    #[test]
    fn exact_rule_wins_over_random_reaction() {
        let root = std::env::temp_dir().join(format!("typingpet-state-{}", crate::unique_id()));
        let source = root.join("special.png");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(&source, b"\x89PNG\r\n\x1a\n").expect("write");

        let store_dir = root.join("rules");
        let mut store = KeyReactionStore::new(&store_dir);
        let stroke = KeyStroke::new(37, crate::keys::KeyModifiers::empty());
        store.set_rule(stroke, &source).expect("set rule");
        let expected = store.image_path(stroke).expect("rule image");

        let mut state = state();
        state.initial_actions();
        let mut rng = FixedRng(0);
        let actions = state.on_key(Some(stroke), &store, &mut rng, 0.0);
        assert_eq!(actions[0], Action::ShowImage(expected));

        // 未注册的按键走随机反应
        let other = KeyStroke::new(38, crate::keys::KeyModifiers::empty());
        let actions = state.on_key(Some(other), &store, &mut rng, 0.0);
        assert!(matches!(actions.first(), Some(Action::ShowImage(_))));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn zero_shake_level_skips_bounce() {
        let mut state = PetState::new(
            Some(PathBuf::from("/img/pet-idle.png")),
            vec![PathBuf::from("/img/a.png")],
            0.0,
        );
        state.initial_actions();
        let rules = KeyReactionStore::new("/nonexistent-rules");
        let mut rng = FixedRng(0);
        let actions = state.on_key(None, &rules, &mut rng, 0.0);
        assert_eq!(actions.len(), 1);
    }
}
