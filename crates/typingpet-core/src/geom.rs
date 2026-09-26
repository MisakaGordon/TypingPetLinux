//! 与平台无关的几何与物理逻辑。
//!
//! 逐行移植自 macOS 版 `Sources/TypingPet/PetWindowGeometry.swift`，
//! 坐标系约定保持一致：原点在左下角、y 轴向上（X11 与 Wayland 的 layer-shell margin 需自行翻转）。

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub dx: f64,
    pub dy: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { dx: 0.0, dy: 0.0 };

    pub fn new(dx: f64, dy: f64) -> Self {
        Self { dx, dy }
    }

    pub fn length(&self) -> f64 {
        hypot(self.dx, self.dy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

impl Size {
    pub fn new(w: f64, h: f64) -> Self {
        Self { w, h }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self::new(origin.x, origin.y, size.w, size.h)
    }

    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }

    pub fn min_x(&self) -> f64 {
        self.x
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }

    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }

    pub fn min_y(&self) -> f64 {
        self.y
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }

    pub fn mid_y(&self) -> f64 {
        self.y + self.h / 2.0
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.min_x()
            && point.x <= self.max_x()
            && point.y >= self.min_y()
            && point.y <= self.max_y()
    }
}

pub fn hypot(a: f64, b: f64) -> f64 {
    (a * a + b * b).sqrt()
}

/// 常驻/悬停透明度选择（无过渡，立即生效）。
pub struct PetOpacityBehavior;

impl PetOpacityBehavior {
    pub fn opacity(is_hovering: bool, resting_opacity: f64, hover_opacity: f64) -> f64 {
        let value = if is_hovering {
            hover_opacity
        } else {
            resting_opacity
        };
        value.clamp(0.0, 1.0)
    }
}

/// 光标靠近时的躲避目标点计算。
pub struct PetPointerAvoidance;

impl PetPointerAvoidance {
    pub const DEFAULT_TRIGGER_DISTANCE: f64 = 90.0;

    /// 点到矩形的最短距离（矩形内部为 0）。
    pub fn distance(point: Point, rect: Rect) -> f64 {
        let horizontal = (rect.min_x() - point.x).max(0.0).max(point.x - rect.max_x());
        let vertical = (rect.min_y() - point.y).max(0.0).max(point.y - rect.max_y());
        hypot(horizontal, vertical)
    }

    pub fn target_origin(mouse_location: Point, pet_frame: Rect, bounds: Rect) -> Option<Point> {
        Self::target_origin_with(mouse_location, pet_frame, bounds, Self::DEFAULT_TRIGGER_DISTANCE)
    }

    /// 返回新的窗口左上角（左下原点坐标系）；光标较远时返回 `None`。
    pub fn target_origin_with(
        mouse_location: Point,
        pet_frame: Rect,
        bounds: Rect,
        trigger_distance: f64,
    ) -> Option<Point> {
        let current_distance = Self::distance(mouse_location, pet_frame);
        if current_distance >= trigger_distance {
            return None;
        }

        let center = Point::new(pet_frame.mid_x(), pet_frame.mid_y());
        let mut away = Vec2::new(center.x - mouse_location.x, center.y - mouse_location.y);
        let away_length = away.length();
        if away_length > 0.001 {
            away.dx /= away_length;
            away.dy /= away_length;
        } else {
            // 光标与中心重合：选最远的角落方向
            let corners = [
                Point::new(bounds.min_x(), bounds.min_y()),
                Point::new(bounds.min_x(), bounds.max_y()),
                Point::new(bounds.max_x(), bounds.min_y()),
                Point::new(bounds.max_x(), bounds.max_y()),
            ];
            let mut farthest = corners[0];
            let mut farthest_distance = hypot(farthest.x - center.x, farthest.y - center.y);
            for corner in corners.iter().skip(1) {
                let distance = hypot(corner.x - center.x, corner.y - center.y);
                if distance > farthest_distance {
                    farthest = *corner;
                    farthest_distance = distance;
                }
            }
            away = Vec2::new(farthest.x - center.x, farthest.y - center.y);
            let length = away.length().max(0.001);
            away.dx /= length;
            away.dy /= length;
        }

        let angles = [0.0_f64, 28.0, -28.0, 55.0, -55.0, 90.0, -90.0];
        let travel = (trigger_distance - current_distance + 46.0).clamp(46.0, 130.0);

        let mut best: Option<(Point, f64)> = None;
        for degrees in angles {
            let radians = degrees * std::f64::consts::PI / 180.0;
            let direction = Vec2::new(
                away.dx * radians.cos() - away.dy * radians.sin(),
                away.dx * radians.sin() + away.dy * radians.cos(),
            );
            let proposed = Point::new(
                pet_frame.x + direction.dx * travel,
                pet_frame.y + direction.dy * travel,
            );
            let origin = Self::clamped_origin(proposed, pet_frame.size(), bounds);
            let moved_frame = Rect::from_origin_size(origin, pet_frame.size());
            let alignment = direction.dx * away.dx + direction.dy * away.dy;
            let diagonalness = 2.0 * direction.dx.abs().min(direction.dy.abs());
            let actual_travel = hypot(origin.x - pet_frame.x, origin.y - pet_frame.y);
            let score = Self::distance(mouse_location, moved_frame)
                + alignment.max(0.0) * 8.0
                + diagonalness * 18.0
                + actual_travel * 0.03;

            // 与 Swift `max(by:)` 一致：只在严格更大时替换（并列取第一个）
            match best {
                Some((_, best_score)) if score <= best_score => {}
                _ => best = Some((origin, score)),
            }
        }

        best.map(|(origin, _)| origin)
    }

    pub fn clamped_origin(origin: Point, size: Size, bounds: Rect) -> Point {
        Point::new(
            origin
                .x
                .clamp(bounds.min_x(), bounds.min_x().max(bounds.max_x() - size.w)),
            origin
                .y
                .clamp(bounds.min_y(), bounds.min_y().max(bounds.max_y() - size.h)),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PetDragSample {
    pub point: Point,
    pub timestamp: f64,
}

impl PetDragSample {
    pub fn new(point: Point, timestamp: f64) -> Self {
        Self { point, timestamp }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PetMotionStep {
    pub origin: Point,
    pub velocity: Vec2,
}

/// 松手惯性：速度采样、衰减、边界反弹。
pub struct PetMotionPhysics;

impl PetMotionPhysics {
    pub const DEFAULT_LOOKBACK: f64 = 0.12;
    pub const DEFAULT_MAXIMUM_SPEED: f64 = 900.0;
    pub const DEFAULT_VELOCITY_RETENTION_PER_SECOND: f64 = 0.004;
    pub const DEFAULT_EDGE_RESTITUTION: f64 = 0.14;

    pub fn release_velocity(samples: &[PetDragSample]) -> Vec2 {
        Self::release_velocity_with(samples, Self::DEFAULT_LOOKBACK, Self::DEFAULT_MAXIMUM_SPEED)
    }

    pub fn release_velocity_with(
        samples: &[PetDragSample],
        lookback: f64,
        maximum_speed: f64,
    ) -> Vec2 {
        let last = match samples.last() {
            Some(sample) => sample,
            None => return Vec2::ZERO,
        };
        let cutoff = last.timestamp - lookback;
        let first = match samples.iter().find(|sample| sample.timestamp >= cutoff) {
            Some(sample) => sample,
            None => return Vec2::ZERO,
        };
        if last.timestamp <= first.timestamp {
            return Vec2::ZERO;
        }

        let elapsed = last.timestamp - first.timestamp;
        let mut velocity = Vec2::new(
            (last.point.x - first.point.x) / elapsed,
            (last.point.y - first.point.y) / elapsed,
        );
        let speed = velocity.length();
        if speed > maximum_speed {
            let factor = maximum_speed / speed;
            velocity.dx *= factor;
            velocity.dy *= factor;
        }
        velocity
    }

    pub fn advance(
        origin: Point,
        size: Size,
        velocity: Vec2,
        elapsed: f64,
        bounds: Rect,
    ) -> PetMotionStep {
        Self::advance_with(
            origin,
            size,
            velocity,
            elapsed,
            bounds,
            Self::DEFAULT_VELOCITY_RETENTION_PER_SECOND,
            Self::DEFAULT_EDGE_RESTITUTION,
        )
    }

    pub fn advance_with(
        origin: Point,
        size: Size,
        velocity: Vec2,
        elapsed: f64,
        bounds: Rect,
        velocity_retention_per_second: f64,
        edge_restitution: f64,
    ) -> PetMotionStep {
        let delta = elapsed.max(0.0);
        let mut next_origin = Point::new(origin.x + velocity.dx * delta, origin.y + velocity.dy * delta);
        let decay = velocity_retention_per_second.powf(delta);
        let mut next_velocity = Vec2::new(velocity.dx * decay, velocity.dy * decay);

        let maximum_x = bounds.min_x().max(bounds.max_x() - size.w);
        let maximum_y = bounds.min_y().max(bounds.max_y() - size.h);

        if next_origin.x < bounds.min_x() {
            next_origin.x = bounds.min_x();
            next_velocity.dx = next_velocity.dx.abs() * edge_restitution;
        } else if next_origin.x > maximum_x {
            next_origin.x = maximum_x;
            next_velocity.dx = -next_velocity.dx.abs() * edge_restitution;
        }

        if next_origin.y < bounds.min_y() {
            next_origin.y = bounds.min_y();
            next_velocity.dy = next_velocity.dy.abs() * edge_restitution;
        } else if next_origin.y > maximum_y {
            next_origin.y = maximum_y;
            next_velocity.dy = -next_velocity.dy.abs() * edge_restitution;
        }

        PetMotionStep {
            origin: next_origin,
            velocity: next_velocity,
        }
    }
}

/// 右上角手柄的等比缩放。
pub struct PetResizeGeometry;

impl PetResizeGeometry {
    pub const MINIMUM_SCALE: f64 = 0.35;
    pub const MAXIMUM_SCALE: f64 = 1.25;

    pub fn scale(initial_scale: f64, initial_size: Size, drag_delta: Point) -> f64 {
        Self::scale_with(
            initial_scale,
            initial_size,
            drag_delta,
            Self::MINIMUM_SCALE,
            Self::MAXIMUM_SCALE,
        )
    }

    pub fn scale_with(
        initial_scale: f64,
        initial_size: Size,
        drag_delta: Point,
        minimum_scale: f64,
        maximum_scale: f64,
    ) -> f64 {
        if initial_size.w <= 0.0 || initial_size.h <= 0.0 {
            return initial_scale.clamp(minimum_scale, maximum_scale);
        }

        let horizontal_factor = (initial_size.w + drag_delta.x) / initial_size.w;
        let vertical_factor = (initial_size.h + drag_delta.y) / initial_size.h;
        let normalized_horizontal = (drag_delta.x / initial_size.w).abs();
        let normalized_vertical = (drag_delta.y / initial_size.h).abs();
        let factor = if normalized_horizontal >= normalized_vertical {
            horizontal_factor
        } else {
            vertical_factor
        };

        (initial_scale * factor).clamp(minimum_scale, maximum_scale)
    }
}

/// 以长边归一化基准尺寸（对应 macOS 版 `normalizedBaseSize`）。
pub fn normalized_base_size(image: Size, longest_side: f64) -> Size {
    if image.w <= 0.0 || image.h <= 0.0 {
        return Size::new(453.0, 354.0);
    }
    if image.w >= image.h {
        Size::new(longest_side, longest_side * image.h / image.w)
    } else {
        Size::new(longest_side * image.w / image.h, longest_side)
    }
}

pub fn scaled_size(base: Size, scale: f64) -> Size {
    Size::new(base.w * scale, base.h * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }

    // 移植自 Tests/TypingPetTests/PetWindowGeometryTests.swift
    #[test]
    fn pointer_avoidance_moves_away_from_nearby_pointer() {
        let frame = Rect::new(100.0, 100.0, 100.0, 100.0);
        let mouse = Point::new(80.0, 150.0);
        let target = PetPointerAvoidance::target_origin(mouse, frame, Rect::new(0.0, 0.0, 500.0, 500.0))
            .expect("target");

        assert!(target.x > frame.x);
        assert!(
            PetPointerAvoidance::distance(mouse, Rect::from_origin_size(target, frame.size()))
                > PetPointerAvoidance::distance(mouse, frame)
        );
    }

    #[test]
    fn pointer_avoidance_can_choose_diagonal_escape_for_horizontal_approach() {
        let frame = Rect::new(180.0, 180.0, 100.0, 100.0);
        let target = PetPointerAvoidance::target_origin(
            Point::new(160.0, 230.0),
            frame,
            Rect::new(0.0, 0.0, 500.0, 500.0),
        )
        .expect("target");

        assert!(target.x > frame.x);
        assert!((target.y - frame.y).abs() > 0.001);
    }

    #[test]
    fn pointer_avoidance_does_nothing_when_pointer_is_far_away() {
        assert!(PetPointerAvoidance::target_origin(
            Point::new(0.0, 0.0),
            Rect::new(300.0, 300.0, 100.0, 100.0),
            Rect::new(0.0, 0.0, 500.0, 500.0),
        )
        .is_none());
    }

    #[test]
    fn pointer_avoidance_chooses_an_available_direction_at_screen_edge() {
        let bounds = Rect::new(0.0, 0.0, 500.0, 500.0);
        let frame = Rect::new(0.0, 180.0, 100.0, 100.0);
        let target =
            PetPointerAvoidance::target_origin(Point::new(60.0, 230.0), frame, bounds).expect("target");

        assert_ne!(target, frame.origin());
        assert!(target.x >= bounds.min_x());
        assert!(target.y >= bounds.min_y());
        assert!(target.x + frame.w <= bounds.max_x());
        assert!(target.y + frame.h <= bounds.max_y());
    }

    #[test]
    fn opacity_selects_resting_and_hover_values_immediately() {
        assert_close(
            PetOpacityBehavior::opacity(false, 1.0, 0.3),
            1.0,
        );
        assert_close(
            PetOpacityBehavior::opacity(true, 1.0, 0.3),
            0.3,
        );
    }

    #[test]
    fn opacity_values_are_clamped_to_valid_range() {
        assert_close(PetOpacityBehavior::opacity(false, 1.4, 0.3), 1.0);
        assert_close(PetOpacityBehavior::opacity(true, 1.0, -0.2), 0.0);
    }

    #[test]
    fn release_velocity_uses_recent_pointer_movement() {
        let velocity = PetMotionPhysics::release_velocity(&[
            PetDragSample::new(Point::new(0.0, 0.0), 1.0),
            PetDragSample::new(Point::new(10.0, 5.0), 1.1),
            PetDragSample::new(Point::new(40.0, 20.0), 1.2),
        ]);

        assert_close(velocity.dx, 300.0);
        assert_close(velocity.dy, 150.0);
    }

    #[test]
    fn release_velocity_is_capped() {
        let velocity = PetMotionPhysics::release_velocity(&[
            PetDragSample::new(Point::ZERO, 1.0),
            PetDragSample::new(Point::new(1000.0, 0.0), 1.01),
        ]);

        assert_close(hypot(velocity.dx, velocity.dy), 900.0);
    }

    #[test]
    fn motion_decays_and_stays_inside_visible_bounds() {
        let free_step = PetMotionPhysics::advance(
            Point::new(100.0, 100.0),
            Size::new(100.0, 100.0),
            Vec2::new(600.0, 300.0),
            0.1,
            Rect::new(0.0, 0.0, 500.0, 500.0),
        );
        assert_close(free_step.origin.x, 160.0);
        assert_close(free_step.origin.y, 130.0);
        assert!(free_step.velocity.dx < 600.0);

        let edge_step = PetMotionPhysics::advance(
            Point::new(390.0, 100.0),
            Size::new(100.0, 100.0),
            Vec2::new(600.0, 0.0),
            0.1,
            Rect::new(0.0, 0.0, 500.0, 500.0),
        );
        assert_close(edge_step.origin.x, 400.0);
        assert!(edge_step.velocity.dx < 0.0);
    }

    #[test]
    fn horizontal_resize_uses_width_and_preserves_scale_relationship() {
        let scale = PetResizeGeometry::scale(
            0.5,
            Size::new(200.0, 100.0),
            Point::new(100.0, 0.0),
        );
        assert_close(scale, 0.75);
    }

    #[test]
    fn vertical_resize_uses_height() {
        let scale = PetResizeGeometry::scale(0.5, Size::new(200.0, 100.0), Point::new(0.0, 50.0));
        assert_close(scale, 0.75);
    }

    #[test]
    fn resize_clamps_to_supported_range() {
        assert_close(
            PetResizeGeometry::scale(0.5, Size::new(200.0, 100.0), Point::new(-500.0, 0.0)),
            0.35,
        );
        assert_close(
            PetResizeGeometry::scale(1.0, Size::new(200.0, 100.0), Point::new(500.0, 0.0)),
            1.25,
        );
    }
}
