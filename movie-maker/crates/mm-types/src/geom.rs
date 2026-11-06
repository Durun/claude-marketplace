//! 矩形と大きさ。座標は画素、原点は左上。

use serde::{Deserialize, Serialize};

/// 2 次元のベクトル。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    /// 成分から。
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// 画素の大きさ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    /// 幅と高さから。
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// 画素の数。
    pub const fn area(self) -> usize {
        self.width as usize * self.height as usize
    }

    /// 全体を覆う矩形。
    pub fn to_rect(self) -> Rect {
        Rect::new(0.0, 0.0, self.width as f32, self.height as f32)
    }
}

/// 矩形。`region_of_definition` と描く領域に使う。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// 空の矩形。何も描かないことを表す。
    pub const EMPTY: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    /// 左上と大きさから。
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// 面積が 0 か。
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// 右端。
    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    /// 下端。
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// 重なり。重ならなければ `EMPTY`。
    pub fn intersect(&self, other: &Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            return Self::EMPTY;
        }
        Self::new(x, y, right - x, bottom - y)
    }

    /// 2 つを含む最小の矩形。
    pub fn union(&self, other: &Self) -> Self {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self::new(x, y, right - x, bottom - y)
    }

    /// 他の矩形を完全に含むか。
    pub fn contains_rect(&self, other: &Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 重ならない矩形は空() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 0.0, 10.0, 10.0);
        assert!(a.intersect(&b).is_empty());
    }

    #[test]
    fn 和は両方を含む() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        let u = a.union(&b);
        assert!(u.contains_rect(&a) && u.contains_rect(&b));
    }
}
