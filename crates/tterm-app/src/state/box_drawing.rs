//! Block elements and box drawing lines, drawn as shapes that fill their cell instead of with
//! the font, as kitty and Ghostty do. A font's glyphs stop at its own line height and rarely
//! line up across cells, so logos made of blocks (Claude Code's) show stripes and the frames of
//! btop show gaps.
//!
//! Sizes are in pixels from the cell's top left corner. The paint step snaps the edges to
//! device pixels, so neighboring cells meet exactly.

/// A filled rectangle, with the opacity of the shades (U+2591 to U+2593).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub alpha: f32,
}

/// A rounded corner (U+256D to U+2570): a straight line from `start` to `from`, a curve
/// through `ctrl` to `to`, and a straight line to `end`, stroked `thickness` wide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    pub start: (f32, f32),
    pub from: (f32, f32),
    pub ctrl: (f32, f32),
    pub to: (f32, f32),
    pub end: (f32, f32),
    pub thickness: f32,
}

/// What a drawn character is made of.
#[derive(Debug, Clone, PartialEq)]
pub enum Shapes {
    Rects(Vec<Rect>),
    Corner(Corner),
}

/// Whether `ch` is drawn here instead of by the font.
pub fn is_drawn(ch: char) -> bool {
    matches!(u32::from(ch), 0x2580..=0x259F) || arms(ch).is_some() || corner_arms(ch).is_some()
}

/// The shapes of `ch` in a cell of `width` by `height`, with lines `light` pixels thick (heavy
/// lines are twice that), or `None` when the font draws it.
pub fn shapes(ch: char, width: f32, height: f32, light: f32) -> Option<Shapes> {
    if let Some(rects) = block_rects(ch, width, height) {
        return Some(Shapes::Rects(rects));
    }
    if let Some(arms) = arms(ch) {
        return Some(Shapes::Rects(line_rects(arms, width, height, light)));
    }
    let (right, down) = corner_arms(ch)?;
    let (cx, cy) = (width / 2.0, height / 2.0);
    let radius = cx.min(cy);
    let (x_end, x_from) = if right {
        (width, cx + radius)
    } else {
        (0.0, cx - radius)
    };
    let (y_end, y_to) = if down {
        (height, cy + radius)
    } else {
        (0.0, cy - radius)
    };
    Some(Shapes::Corner(Corner {
        start: (x_end, cy),
        from: (x_from, cy),
        ctrl: (cx, cy),
        to: (cx, y_to),
        end: (cx, y_end),
        thickness: light,
    }))
}

/// The block elements, U+2580 to U+259F.
fn block_rects(ch: char, w: f32, h: f32) -> Option<Vec<Rect>> {
    let rect = |x: f32, y: f32, rw: f32, rh: f32| Rect {
        x: x * w,
        y: y * h,
        w: rw * w,
        h: rh * h,
        alpha: 1.0,
    };
    let shade = |alpha: f32| Rect {
        alpha,
        ..rect(0.0, 0.0, 1.0, 1.0)
    };
    let eighth = |n: u32| n as f32 / 8.0;
    let code = u32::from(ch);
    let one = match code {
        0x2580 => rect(0.0, 0.0, 1.0, 0.5),
        // Lower one eighth to lower seven eighths.
        0x2581..=0x2587 => {
            let part = eighth(code - 0x2580);
            rect(0.0, 1.0 - part, 1.0, part)
        }
        0x2588 => rect(0.0, 0.0, 1.0, 1.0),
        // Left seven eighths down to left one eighth.
        0x2589..=0x258F => rect(0.0, 0.0, eighth(0x2590 - code), 1.0),
        0x2590 => rect(0.5, 0.0, 0.5, 1.0),
        0x2591 => shade(0.25),
        0x2592 => shade(0.5),
        0x2593 => shade(0.75),
        0x2594 => rect(0.0, 0.0, 1.0, eighth(1)),
        0x2595 => rect(1.0 - eighth(1), 0.0, eighth(1), 1.0),
        0x2596..=0x259F => return Some(quadrants(code, w, h)),
        _ => return None,
    };
    Some(vec![one])
}

/// The quadrant blocks, U+2596 to U+259F: which of upper left, upper right, lower left and
/// lower right each one fills.
fn quadrants(code: u32, w: f32, h: f32) -> Vec<Rect> {
    let [ul, ur, ll, lr] = match code {
        0x2596 => [false, false, true, false],
        0x2597 => [false, false, false, true],
        0x2598 => [true, false, false, false],
        0x2599 => [true, false, true, true],
        0x259A => [true, false, false, true],
        0x259B => [true, true, true, false],
        0x259C => [true, true, false, true],
        0x259D => [false, true, false, false],
        0x259E => [false, true, true, false],
        _ => [false, true, true, true],
    };
    let (hw, hh) = (w / 2.0, h / 2.0);
    [(ul, 0.0, 0.0), (ur, hw, 0.0), (ll, 0.0, hh), (lr, hw, hh)]
        .into_iter()
        .filter(|(on, _, _)| *on)
        .map(|(_, x, y)| Rect {
            x,
            y,
            w: hw,
            h: hh,
            alpha: 1.0,
        })
        .collect()
}

/// The weight of each arm of a box drawing line: up, right, down and left, 0 for none, 1 light,
/// 2 heavy.
type Arms = [u8; 4];

/// The straight box drawing lines: U+2500 to U+254B (but the dashed ones) and U+2574 to U+257F.
fn arms(ch: char) -> Option<Arms> {
    const LINES: [Arms; 76] = [
        [0, 1, 0, 1], // ─
        [0, 2, 0, 2], // ━
        [1, 0, 1, 0], // │
        [2, 0, 2, 0], // ┃
        [0; 4],       // dashed, U+2504 to U+250B: drawn by the font
        [0; 4],
        [0; 4],
        [0; 4],
        [0; 4],
        [0; 4],
        [0; 4],
        [0; 4],
        [0, 1, 1, 0], // ┌
        [0, 2, 1, 0], // ┍
        [0, 1, 2, 0], // ┎
        [0, 2, 2, 0], // ┏
        [0, 0, 1, 1], // ┐
        [0, 0, 1, 2], // ┑
        [0, 0, 2, 1], // ┒
        [0, 0, 2, 2], // ┓
        [1, 1, 0, 0], // └
        [1, 2, 0, 0], // ┕
        [2, 1, 0, 0], // ┖
        [2, 2, 0, 0], // ┗
        [1, 0, 0, 1], // ┘
        [1, 0, 0, 2], // ┙
        [2, 0, 0, 1], // ┚
        [2, 0, 0, 2], // ┛
        [1, 1, 1, 0], // ├
        [1, 2, 1, 0], // ┝
        [2, 1, 1, 0], // ┞
        [1, 1, 2, 0], // ┟
        [2, 1, 2, 0], // ┠
        [2, 2, 1, 0], // ┡
        [1, 2, 2, 0], // ┢
        [2, 2, 2, 0], // ┣
        [1, 0, 1, 1], // ┤
        [1, 0, 1, 2], // ┥
        [2, 0, 1, 1], // ┦
        [1, 0, 2, 1], // ┧
        [2, 0, 2, 1], // ┨
        [2, 0, 1, 2], // ┩
        [1, 0, 2, 2], // ┪
        [2, 0, 2, 2], // ┫
        [0, 1, 1, 1], // ┬
        [0, 1, 1, 2], // ┭
        [0, 2, 1, 1], // ┮
        [0, 2, 1, 2], // ┯
        [0, 1, 2, 1], // ┰
        [0, 1, 2, 2], // ┱
        [0, 2, 2, 1], // ┲
        [0, 2, 2, 2], // ┳
        [1, 1, 0, 1], // ┴
        [1, 1, 0, 2], // ┵
        [1, 2, 0, 1], // ┶
        [1, 2, 0, 2], // ┷
        [2, 1, 0, 1], // ┸
        [2, 1, 0, 2], // ┹
        [2, 2, 0, 1], // ┺
        [2, 2, 0, 2], // ┻
        [1, 1, 1, 1], // ┼
        [1, 1, 1, 2], // ┽
        [1, 2, 1, 1], // ┾
        [1, 2, 1, 2], // ┿
        [2, 1, 1, 1], // ╀
        [1, 1, 2, 1], // ╁
        [2, 1, 2, 1], // ╂
        [2, 1, 1, 2], // ╃
        [2, 2, 1, 1], // ╄
        [1, 1, 2, 2], // ╅
        [1, 2, 2, 1], // ╆
        [2, 2, 1, 2], // ╇
        [1, 2, 2, 2], // ╈
        [2, 1, 2, 2], // ╉
        [2, 2, 2, 1], // ╊
        [2, 2, 2, 2], // ╋
    ];
    const HALVES: [Arms; 12] = [
        [0, 0, 0, 1], // ╴
        [1, 0, 0, 0], // ╵
        [0, 1, 0, 0], // ╶
        [0, 0, 1, 0], // ╷
        [0, 0, 0, 2], // ╸
        [2, 0, 0, 0], // ╹
        [0, 2, 0, 0], // ╺
        [0, 0, 2, 0], // ╻
        [0, 2, 0, 1], // ╼
        [1, 0, 2, 0], // ╽
        [0, 1, 0, 2], // ╾
        [2, 0, 1, 0], // ╿
    ];
    let code = u32::from(ch);
    let arms = match code {
        0x2500..=0x254B => LINES[(code - 0x2500) as usize],
        0x2574..=0x257F => HALVES[(code - 0x2574) as usize],
        _ => return None,
    };
    (arms != [0; 4]).then_some(arms)
}

/// The rounded corners, U+256D to U+2570: whether each goes right (else left) and down (else up).
fn corner_arms(ch: char) -> Option<(bool, bool)> {
    match ch {
        '╭' => Some((true, true)),
        '╮' => Some((false, true)),
        '╯' => Some((false, false)),
        '╰' => Some((true, false)),
        _ => None,
    }
}

/// The rectangles of a straight line: each arm from the cell's edge to its center, long enough
/// to cover the lines it crosses.
fn line_rects([up, right, down, left]: Arms, w: f32, h: f32, light: f32) -> Vec<Rect> {
    let thick = |weight: u8| light * f32::from(weight);
    let (cx, cy) = (w / 2.0, h / 2.0);
    // How far an arm reaches past the center: half the thickest line, to cover the joint.
    let reach = thick(up.max(down).max(left).max(right)) / 2.0;
    let mut rects = Vec::new();
    let mut push = |x: f32, y: f32, rw: f32, rh: f32| {
        rects.push(Rect {
            x,
            y,
            w: rw,
            h: rh,
            alpha: 1.0,
        })
    };
    if up > 0 {
        let t = thick(up);
        push(cx - t / 2.0, 0.0, t, cy + reach);
    }
    if down > 0 {
        let t = thick(down);
        push(cx - t / 2.0, cy - reach, t, h - cy + reach);
    }
    if left > 0 {
        let t = thick(left);
        push(0.0, cy - t / 2.0, cx + reach, t);
    }
    if right > 0 {
        let t = thick(right);
        push(cx - reach, cy - t / 2.0, w - cx + reach, t);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects(ch: char) -> Vec<Rect> {
        match shapes(ch, 10.0, 20.0, 1.0) {
            Some(Shapes::Rects(rects)) => rects,
            other => panic!("{ch:?} gave {other:?}"),
        }
    }

    fn bounds(rect: &Rect) -> (f32, f32, f32, f32) {
        (rect.x, rect.y, rect.w, rect.h)
    }

    #[test]
    fn full_and_half_blocks_fill_their_part_of_the_cell() {
        assert_eq!(bounds(&rects('█')[0]), (0.0, 0.0, 10.0, 20.0));
        assert_eq!(bounds(&rects('▀')[0]), (0.0, 0.0, 10.0, 10.0));
        assert_eq!(bounds(&rects('▄')[0]), (0.0, 10.0, 10.0, 10.0));
        assert_eq!(bounds(&rects('▌')[0]), (0.0, 0.0, 5.0, 20.0));
        assert_eq!(bounds(&rects('▐')[0]), (5.0, 0.0, 5.0, 20.0));
    }

    #[test]
    fn eighths_grow_from_their_edge() {
        assert_eq!(bounds(&rects('▁')[0]), (0.0, 17.5, 10.0, 2.5));
        assert_eq!(bounds(&rects('▇')[0]), (0.0, 2.5, 10.0, 17.5));
        assert_eq!(bounds(&rects('▉')[0]), (0.0, 0.0, 8.75, 20.0));
        assert_eq!(bounds(&rects('▏')[0]), (0.0, 0.0, 1.25, 20.0));
        assert_eq!(bounds(&rects('▔')[0]), (0.0, 0.0, 10.0, 2.5));
        assert_eq!(bounds(&rects('▕')[0]), (8.75, 0.0, 1.25, 20.0));
    }

    #[test]
    fn quadrants_of_the_claude_logo() {
        // ▛ is upper left, upper right and lower left.
        let parts: Vec<_> = rects('▛').iter().map(bounds).collect();
        assert_eq!(
            parts,
            [
                (0.0, 0.0, 5.0, 10.0),
                (5.0, 0.0, 5.0, 10.0),
                (0.0, 10.0, 5.0, 10.0)
            ]
        );
        assert_eq!(bounds(&rects('▝')[0]), (5.0, 0.0, 5.0, 10.0));
        assert_eq!(rects('▚').len(), 2);
    }

    #[test]
    fn shades_cover_the_cell_with_their_opacity() {
        assert_eq!(rects('░')[0].alpha, 0.25);
        assert_eq!(rects('▒')[0].alpha, 0.5);
        assert_eq!(rects('▓')[0].alpha, 0.75);
    }

    #[test]
    fn lines_reach_the_edges_of_the_cell() {
        let parts = |ch| rects(ch).iter().map(bounds).collect::<Vec<_>>();
        // One arm from each edge, overlapping at the center.
        assert_eq!(parts('─'), [(0.0, 9.5, 5.5, 1.0), (4.5, 9.5, 5.5, 1.0)]);
        assert_eq!(parts('│'), [(4.5, 0.0, 1.0, 10.5), (4.5, 9.5, 1.0, 10.5)]);
        assert_eq!(parts('━'), [(0.0, 9.0, 6.0, 2.0), (4.0, 9.0, 6.0, 2.0)]);
    }

    #[test]
    fn a_corner_is_two_arms_meeting_at_the_center() {
        // ┌ goes down and right; both arms cover the joint.
        let parts: Vec<_> = rects('┌').iter().map(bounds).collect();
        assert_eq!(parts, [(4.5, 9.5, 1.0, 10.5), (4.5, 9.5, 5.5, 1.0)]);
        assert_eq!(rects('┼').len(), 4);
        assert_eq!(rects('╴').len(), 1);
    }

    #[test]
    fn rounded_corners_curve_through_the_center() {
        let Some(Shapes::Corner(corner)) = shapes('╭', 10.0, 20.0, 1.0) else {
            panic!("╭ is a corner");
        };
        assert_eq!(corner.start, (10.0, 10.0));
        assert_eq!(corner.ctrl, (5.0, 10.0));
        assert_eq!(corner.end, (5.0, 20.0));
    }

    #[test]
    fn text_and_dashed_lines_stay_with_the_font() {
        for ch in ['a', ' ', '┄', '═', '⣿', '╱'] {
            assert!(!is_drawn(ch), "{ch:?}");
            assert!(shapes(ch, 10.0, 20.0, 1.0).is_none());
        }
        for ch in ['█', '▛', '│', '╭', '┼', '╿'] {
            assert!(is_drawn(ch), "{ch:?}");
        }
    }
}
