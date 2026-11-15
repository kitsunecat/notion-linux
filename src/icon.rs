//! オリジナルのアプリアイコンをコードで描画する (Notion のロゴは使用しない)。
//! デザインは assets/notion-linux.svg と同じ: 紺色の角丸四角に、折り返しのある白いページ。

const BG: [u8; 3] = [59, 74, 107];
const PAGE: [u8; 3] = [255, 255, 255];
const FOLD: [u8; 3] = [196, 205, 224];

type P = (f32, f32);

fn in_convex(poly: &[P], p: P) -> bool {
    let n = poly.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cross != 0.0 {
            if sign == 0.0 {
                sign = cross.signum();
            } else if sign != cross.signum() {
                return false;
            }
        }
    }
    true
}

fn in_round_rect(p: P, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    if p.0 < x0 || p.0 > x1 || p.1 < y0 || p.1 > y1 {
        return false;
    }
    let cx = p.0.clamp(x0 + r, x1 - r);
    let cy = p.1.clamp(y0 + r, y1 - r);
    let (dx, dy) = (p.0 - cx, p.1 - cy);
    dx * dx + dy * dy <= r * r
}

/// 正規化座標 (0..1) の 1 点の色。透明なら None。
fn sample(p: P) -> Option<[u8; 3]> {
    if !in_round_rect(p, 0.04, 0.04, 0.96, 0.96, 0.2) {
        return None;
    }
    let fold = [(0.58, 0.20), (0.72, 0.34), (0.58, 0.34)];
    if in_convex(&fold, p) {
        return Some(FOLD);
    }
    let page = [
        (0.28, 0.20),
        (0.58, 0.20),
        (0.72, 0.34),
        (0.72, 0.80),
        (0.28, 0.80),
    ];
    if in_convex(&page, p) {
        let lines: [(f32, f32); 3] = [(0.44, 0.64), (0.54, 0.64), (0.64, 0.54)];
        for (y, x_end) in lines {
            if in_round_rect(p, 0.36, y, x_end, y + 0.045, 0.02) {
                return Some(BG);
            }
        }
        return Some(PAGE);
    }
    Some(BG)
}

/// size x size の RGBA バッファを返す (4x4 スーパーサンプリングでアンチエイリアス)。
pub fn rgba(size: u32) -> Vec<u8> {
    const SS: u32 = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut a) = (0u32, 0u32, 0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = (x as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let py = (y as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    if let Some(c) = sample((px, py)) {
                        r += c[0] as u32;
                        g += c[1] as u32;
                        b += c[2] as u32;
                        a += 255;
                    }
                }
            }
            let n = SS * SS;
            let covered = a / 255;
            let px = match (
                r.checked_div(covered),
                g.checked_div(covered),
                b.checked_div(covered),
            ) {
                (Some(r), Some(g), Some(b)) => [r as u8, g as u8, b as u8, (a / n) as u8],
                _ => [0, 0, 0, 0],
            };
            out.extend_from_slice(&px);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_has_expected_size_and_transparent_corner() {
        let s = 64;
        let buf = super::rgba(s);
        assert_eq!(buf.len(), (s * s * 4) as usize);
        assert_eq!(buf[3], 0, "左上の角は透明");
        let center = (((s / 2) * s + s / 2) * 4) as usize;
        assert_eq!(buf[center + 3], 255, "中央は不透明");
    }
}
