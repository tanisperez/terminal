//! Split panes (#33, design S1): a tab holds a tree of panes, each one a terminal, split side by
//! side or stacked, and which one has the focus. Plain data, tested without a window: what
//! splitting, closing, moving the focus and resizing do. Each leaf holds a `T`, the view of its
//! terminal in the app.

/// How a split lays its two halves out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Side by side, the first on the left.
    Horizontal,
    /// Stacked, the first on top.
    Vertical,
}

/// A direction to move the focus or a separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// To the left.
    Left,
    /// To the right.
    Right,
    /// Up.
    Up,
    /// Down.
    Down,
}

impl Direction {
    /// The axis this direction moves along.
    pub fn axis(self) -> Axis {
        match self {
            Direction::Left | Direction::Right => Axis::Horizontal,
            Direction::Up | Direction::Down => Axis::Vertical,
        }
    }
}

/// The smallest share of a split a half keeps when resized.
pub const MIN_RATIO: f32 = 0.1;
/// How much a resize shortcut moves a separator, as a share of its split.
pub const RESIZE_STEP: f32 = 0.05;

/// A tree of panes.
#[derive(Debug, Clone, PartialEq)]
pub enum PaneTree<T> {
    /// One pane.
    Leaf(T),
    /// Two halves.
    Split {
        /// How the halves are laid out.
        axis: Axis,
        /// The share of the first half, between [`MIN_RATIO`] and `1 - MIN_RATIO`.
        ratio: f32,
        /// The left or top half.
        first: Box<PaneTree<T>>,
        /// The right or bottom half.
        second: Box<PaneTree<T>>,
    },
}

/// A rectangle in a unit square: where a pane sits in its tab.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl<T: Clone + PartialEq> PaneTree<T> {
    /// Every pane, in order: left to right, top to bottom.
    pub fn leaves(&self) -> Vec<T> {
        match self {
            PaneTree::Leaf(pane) => vec![pane.clone()],
            PaneTree::Split { first, second, .. } => {
                let mut leaves = first.leaves();
                leaves.extend(second.leaves());
                leaves
            }
        }
    }

    /// How many panes.
    pub fn len(&self) -> usize {
        match self {
            PaneTree::Leaf(_) => 1,
            PaneTree::Split { first, second, .. } => first.len() + second.len(),
        }
    }

    /// Whether `pane` is in the tree.
    pub fn contains(&self, pane: &T) -> bool {
        match self {
            PaneTree::Leaf(leaf) => leaf == pane,
            PaneTree::Split { first, second, .. } => first.contains(pane) || second.contains(pane),
        }
    }

    /// Splits `pane` along `axis`, `new` taking the right or bottom half. Returns whether `pane`
    /// was found.
    pub fn split(&mut self, pane: &T, axis: Axis, new: T) -> bool {
        match self {
            PaneTree::Leaf(leaf) if leaf == pane => {
                let old = PaneTree::Leaf(leaf.clone());
                *self = PaneTree::Split {
                    axis,
                    ratio: 0.5,
                    first: Box::new(old),
                    second: Box::new(PaneTree::Leaf(new)),
                };
                true
            }
            PaneTree::Leaf(_) => false,
            PaneTree::Split { first, second, .. } => {
                first.split(pane, axis, new.clone()) || second.split(pane, axis, new)
            }
        }
    }

    /// Removes `pane`: its sibling takes the place of their split. Returns whether it was found
    /// and was not the only pane, which stays.
    pub fn remove(&mut self, pane: &T) -> bool {
        let PaneTree::Split { first, second, .. } = self else {
            return false;
        };
        if matches!(first.as_ref(), PaneTree::Leaf(leaf) if leaf == pane) {
            *self = (**second).clone();
            return true;
        }
        if matches!(second.as_ref(), PaneTree::Leaf(leaf) if leaf == pane) {
            *self = (**first).clone();
            return true;
        }
        first.remove(pane) || second.remove(pane)
    }

    /// Where every pane sits in the unit square of its tab.
    pub fn rects(&self) -> Vec<(T, Rect)> {
        let mut out = Vec::new();
        self.collect_rects(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            &mut out,
        );
        out
    }

    fn collect_rects(&self, area: Rect, out: &mut Vec<(T, Rect)>) {
        match self {
            PaneTree::Leaf(pane) => out.push((pane.clone(), area)),
            PaneTree::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (a, b) = match axis {
                    Axis::Horizontal => (
                        Rect {
                            w: area.w * ratio,
                            ..area
                        },
                        Rect {
                            x: area.x + area.w * ratio,
                            w: area.w * (1.0 - ratio),
                            ..area
                        },
                    ),
                    Axis::Vertical => (
                        Rect {
                            h: area.h * ratio,
                            ..area
                        },
                        Rect {
                            y: area.y + area.h * ratio,
                            h: area.h * (1.0 - ratio),
                            ..area
                        },
                    ),
                };
                first.collect_rects(a, out);
                second.collect_rects(b, out);
            }
        }
    }

    /// The pane next to `pane` in `direction`, as Windows Terminal moves the focus: the closest
    /// one whose edge touches `pane`'s side and overlaps it, `None` at the border of the tab.
    pub fn neighbor(&self, pane: &T, direction: Direction) -> Option<T> {
        const EPSILON: f32 = 1e-4;
        let rects = self.rects();
        let (_, from) = rects.iter().find(|(leaf, _)| leaf == pane)?;
        let overlap = |a0: f32, a1: f32, b0: f32, b1: f32| a0.max(b0) < a1.min(b1) - EPSILON;
        rects
            .iter()
            .filter(|(leaf, _)| leaf != pane)
            .filter(|(_, to)| match direction {
                Direction::Left => {
                    (to.x + to.w - from.x).abs() < EPSILON
                        && overlap(from.y, from.y + from.h, to.y, to.y + to.h)
                }
                Direction::Right => {
                    (from.x + from.w - to.x).abs() < EPSILON
                        && overlap(from.y, from.y + from.h, to.y, to.y + to.h)
                }
                Direction::Up => {
                    (to.y + to.h - from.y).abs() < EPSILON
                        && overlap(from.x, from.x + from.w, to.x, to.x + to.w)
                }
                Direction::Down => {
                    (from.y + from.h - to.y).abs() < EPSILON
                        && overlap(from.x, from.x + from.w, to.x, to.x + to.w)
                }
            })
            // The one nearest to the top or left corner of `pane`, as the eye reads.
            .min_by(|(_, a), (_, b)| {
                let distance = |r: &Rect| (r.x - from.x).abs() + (r.y - from.y).abs();
                distance(a).total_cmp(&distance(b))
            })
            .map(|(leaf, _)| leaf.clone())
    }

    /// Moves the separator nearest to `pane` along `direction`'s axis by `step` (a share of
    /// that split): the innermost split around `pane` of that axis. Returns whether one moved.
    pub fn resize(&mut self, pane: &T, direction: Direction, step: f32) -> bool {
        let PaneTree::Split {
            axis,
            ratio,
            first,
            second,
        } = self
        else {
            return false;
        };
        // A deeper split of the same axis is nearer to the pane: it moves first.
        if first.contains(pane) && first.resize(pane, direction, step) {
            return true;
        }
        if second.contains(pane) && second.resize(pane, direction, step) {
            return true;
        }
        if *axis != direction.axis() || !(first.contains(pane) || second.contains(pane)) {
            return false;
        }
        let delta = match direction {
            Direction::Right | Direction::Down => step,
            Direction::Left | Direction::Up => -step,
        };
        *ratio = (*ratio + delta).clamp(MIN_RATIO, 1.0 - MIN_RATIO);
        true
    }

    /// Sets the ratio of the split found by following `path` from the root (`false` the first
    /// half, `true` the second), as a dragged separator does.
    pub fn set_ratio(&mut self, path: &[bool], value: f32) {
        let PaneTree::Split {
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return;
        };
        match path.split_first() {
            None => *ratio = value.clamp(MIN_RATIO, 1.0 - MIN_RATIO),
            Some((false, rest)) => first.set_ratio(rest, value),
            Some((true, rest)) => second.set_ratio(rest, value),
        }
    }
}

/// How to split a pane (design S1, Windows Terminal's shortcuts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneSplit {
    /// A new pane on the right (Alt+Shift+Plus).
    Right,
    /// A new pane below (Alt+Shift+Minus).
    Down,
    /// The same profile, along the pane's longer side (Alt+Shift+D).
    Duplicate,
}

/// What a pane shortcut asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKey {
    /// Split the pane.
    Split(PaneSplit),
    /// Move the focus to the pane next to it.
    Focus(Direction),
    /// Move the nearest separator.
    Resize(Direction),
}

/// The pane shortcut of a key pressed with Alt, as Windows Terminal's: Alt+arrows move the
/// focus, Alt+Shift+arrows resize, Alt+Shift+Plus and Minus split, Alt+Shift+D duplicates.
/// `key` is gpui's name of the key, which for a symbol is what it types, with Shift dropped:
/// `_` is Shift+Minus on US and Spanish layouts, `+` is Shift+= on US and `*` is Shift+Plus on
/// Spanish. Arrows only count in a tab with several panes (`split`), so a single pane leaves
/// them to the program.
pub fn pane_shortcut(key: &str, shift: bool, split: bool) -> Option<PaneKey> {
    let direction = match key {
        "left" => Some(Direction::Left),
        "right" => Some(Direction::Right),
        "up" => Some(Direction::Up),
        "down" => Some(Direction::Down),
        _ => None,
    };
    if let Some(direction) = direction {
        return split.then_some(if shift {
            PaneKey::Resize(direction)
        } else {
            PaneKey::Focus(direction)
        });
    }
    match key {
        "+" | "*" => Some(PaneKey::Split(PaneSplit::Right)),
        "_" => Some(PaneKey::Split(PaneSplit::Down)),
        "d" if shift => Some(PaneKey::Split(PaneSplit::Duplicate)),
        _ => None,
    }
}

/// The axis that splits a pane of `width` by `height` pixels into the roomiest halves, as
/// Windows Terminal's Alt+Shift+D: side by side when it is wider than tall.
pub fn roomiest_axis(width: f32, height: f32) -> Axis {
    if width >= height {
        Axis::Horizontal
    } else {
        Axis::Vertical
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// A on the left, B over C on the right.
    fn three() -> PaneTree<char> {
        let mut tree = PaneTree::Leaf('a');
        assert!(tree.split(&'a', Axis::Horizontal, 'b'));
        assert!(tree.split(&'b', Axis::Vertical, 'c'));
        tree
    }

    #[test]
    fn splitting_puts_the_new_pane_right_or_below() {
        let tree = three();
        assert_eq!(tree.leaves(), vec!['a', 'b', 'c']);
        assert_eq!(tree.len(), 3);
        let rects = tree.rects();
        assert_eq!(
            rects[2].1,
            Rect {
                x: 0.5,
                y: 0.5,
                w: 0.5,
                h: 0.5
            }
        );
        let mut tree = three();
        assert!(!tree.split(&'z', Axis::Vertical, 'd'));
    }

    #[test]
    fn closing_a_pane_gives_its_place_to_its_sibling() {
        let mut tree = three();
        assert!(tree.remove(&'b'));
        assert_eq!(tree.leaves(), vec!['a', 'c']);
        assert!(tree.remove(&'a'));
        assert_eq!(tree, PaneTree::Leaf('c'));
        // The only pane stays.
        assert!(!tree.remove(&'c'));
    }

    #[test]
    fn the_focus_moves_to_the_touching_pane() {
        let tree = three();
        assert_eq!(tree.neighbor(&'a', Direction::Right), Some('b'));
        assert_eq!(tree.neighbor(&'c', Direction::Left), Some('a'));
        assert_eq!(tree.neighbor(&'b', Direction::Down), Some('c'));
        assert_eq!(tree.neighbor(&'c', Direction::Up), Some('b'));
        assert_eq!(tree.neighbor(&'a', Direction::Left), None);
        assert_eq!(tree.neighbor(&'b', Direction::Right), None);
    }

    #[test]
    fn resizing_moves_the_nearest_separator_of_that_axis() {
        let mut tree = three();
        // From C, up and down move the B/C separator; left moves the A/BC one.
        assert!(tree.resize(&'c', Direction::Up, 0.1));
        assert!(tree.resize(&'c', Direction::Left, 0.1));
        let rects = tree.rects();
        assert!((rects[0].1.w - 0.4).abs() < 1e-5);
        assert!((rects[1].1.h - 0.4).abs() < 1e-5);
        // Ratios stop at the minimum.
        for _ in 0..20 {
            tree.resize(&'a', Direction::Left, 0.1);
        }
        assert!((tree.rects()[0].1.w - MIN_RATIO).abs() < 1e-5);
        // A single pane has nothing to resize.
        assert!(!PaneTree::Leaf('a').resize(&'a', Direction::Left, 0.1));
    }

    #[test]
    fn a_dragged_separator_sets_its_split() {
        let mut tree = three();
        tree.set_ratio(&[], 0.7);
        tree.set_ratio(&[true], 0.25);
        let rects = tree.rects();
        assert!((rects[0].1.w - 0.7).abs() < 1e-5);
        assert!((rects[1].1.h - 0.25).abs() < 1e-5);
    }

    #[test]
    fn pane_shortcuts_follow_windows_terminal() {
        use PaneKey::*;
        assert_eq!(
            pane_shortcut("_", false, false),
            Some(Split(PaneSplit::Down))
        );
        assert_eq!(
            pane_shortcut("+", false, false),
            Some(Split(PaneSplit::Right))
        );
        assert_eq!(
            pane_shortcut("*", false, false),
            Some(Split(PaneSplit::Right))
        );
        assert_eq!(
            pane_shortcut("d", true, false),
            Some(Split(PaneSplit::Duplicate))
        );
        assert_eq!(pane_shortcut("d", false, false), None);
        assert_eq!(pane_shortcut("-", false, true), None);
        // Arrows only in a split tab: otherwise the program keeps them.
        assert_eq!(pane_shortcut("left", false, false), None);
        assert_eq!(
            pane_shortcut("left", false, true),
            Some(Focus(Direction::Left))
        );
        assert_eq!(pane_shortcut("up", true, true), Some(Resize(Direction::Up)));
    }

    #[test]
    fn duplicating_splits_along_the_longer_side() {
        assert_eq!(roomiest_axis(1200.0, 700.0), Axis::Horizontal);
        assert_eq!(roomiest_axis(500.0, 700.0), Axis::Vertical);
    }
}
