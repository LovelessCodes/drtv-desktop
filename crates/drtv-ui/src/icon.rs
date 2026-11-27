//! The app icon, drawn rather than shipped as a bitmap.
//!
//! A red rounded tile with a white play mark: no DR wordmark, no DR logo, nothing that could be
//! mistaken for the broadcaster's own badge. It is generated at whatever size the caller asks for,
//! so the window icon, the macOS `.icns` and a future Windows `.ico` are all the same drawing and
//! there is no 1024px PNG in the repository to keep in sync with it.
//!
//! Every pixel is the average of a 4×4 sample grid, which is enough to make the corners of the tile
//! and the tip of the triangle look drawn rather than stepped.

use egui::Color32;

/// Top and bottom of the tile's vertical gradient; both are the brand red, one lit and one sunk.
const TILE_TOP: [f32; 3] = [236.0, 24.0, 56.0];
const TILE_BOTTOM: [f32; 3] = [169.0, 0.0, 32.0];

/// Share of the canvas the tile covers, per side of its inset — macOS and Windows both leave a
/// margin around an icon rather than running it to the edge.
const INSET: f32 = 0.075;
/// Corner radius as a share of the tile's side.
const RADIUS: f32 = 0.225;

/// The play mark, in unit coordinates, inset from the tile so it reads at 16px.
const MARK: [(f32, f32); 3] = [(0.385, 0.275), (0.385, 0.725), (0.745, 0.5)];

/// The icon as RGBA8 rows, `size` pixels square.
pub fn rgba(size: u32) -> Vec<u8> {
    let size = size.max(16) as f32;
    let mut out = Vec::with_capacity((size * size * 4.0) as usize);
    let samples = 4;
    let half = 0.5 - INSET;
    let radius = RADIUS * (2.0 * half);

    for y in 0..size as u32 {
        for x in 0..size as u32 {
            let (mut tile, mut mark) = (0.0, 0.0);
            for sy in 0..samples {
                for sx in 0..samples {
                    let px = (x as f32 + (sx as f32 + 0.5) / samples as f32) / size - 0.5;
                    let py = (y as f32 + (sy as f32 + 0.5) / samples as f32) / size - 0.5;
                    if rounded_rect(px, py, half, radius) <= 0.0 {
                        tile += 1.0;
                        if in_mark(px, py) {
                            mark += 1.0;
                        }
                    }
                }
            }
            let total = (samples * samples) as f32;
            let (tile, mark) = (tile / total, mark / total);
            let shade = (y as f32 + 0.5) / size;
            let base = lerp3(TILE_TOP, TILE_BOTTOM, shade);
            let rgb = lerp3(base, [255.0, 255.0, 255.0], mark);
            out.extend_from_slice(&[
                rgb[0] as u8,
                rgb[1] as u8,
                rgb[2] as u8,
                (tile * 255.0).round() as u8,
            ]);
        }
    }
    out
}

/// The icon in the shape `eframe` wants for the window and the dock.
pub fn icon_data(size: u32) -> egui::IconData {
    egui::IconData { rgba: rgba(size), width: size, height: size }
}

/// Signed distance to a rounded square centred on the origin; negative inside.
fn rounded_rect(x: f32, y: f32, half: f32, radius: f32) -> f32 {
    let qx = x.abs() - half + radius;
    let qy = y.abs() - half + radius;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

/// Point in triangle, by the sign of the three edge cross-products. The mark is a solid triangle,
/// so this is enough — its edges are antialiased by the sample grid.
fn in_mark(x: f32, y: f32) -> bool {
    // The mark is authored in 0..1 canvas coordinates; the samples arrive centred on the tile.
    let [(ax, ay), (bx, by), (cx, cy)] = MARK.map(|(x, y)| (x - 0.5, y - 0.5));
    let edges = [
        (bx - ax, by - ay, x - ax, y - ay),
        (cx - bx, cy - by, x - bx, y - by),
        (ax - cx, ay - cy, x - cx, y - cy),
    ];
    let mut positive = 0;
    let mut negative = 0;
    for (ex, ey, px, py) in edges {
        let cross = ex * py - ey * px;
        if cross > 0.0 {
            positive += 1;
        } else if cross < 0.0 {
            negative += 1;
        }
    }
    positive == 3 || negative == 3
}

fn lerp3(from: [f32; 3], to: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        from[0] + (to[0] - from[0]) * t,
        from[1] + (to[1] - from[1]) * t,
        from[2] + (to[2] - from[2]) * t,
    ]
}

/// The colour at the centre of the tile, for tests and for anyone painting the mark elsewhere.
pub fn tile_colour() -> Color32 {
    let mid = lerp3(TILE_TOP, TILE_BOTTOM, 0.5);
    Color32::from_rgb(mid[0] as u8, mid[1] as u8, mid[2] as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(data: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    }

    #[test]
    fn the_corners_are_clear_and_the_middle_of_the_tile_is_red() {
        let size = 64;
        let data = rgba(size);
        assert_eq!(data.len(), (size * size * 4) as usize);
        // Outside the rounded corner: transparent, not a white square on a dark dock.
        assert_eq!(pixel(&data, size, 0, 0)[3], 0);
        assert_eq!(pixel(&data, size, size - 1, size - 1)[3], 0);
        // Inside the tile, away from the mark: red, opaque.
        let [r, g, b, a] = pixel(&data, size, 8, size / 2);
        assert_eq!(a, 255);
        assert!(r > 180 && g < 40 && b < 80, "tile should be red, got {r},{g},{b}");
    }

    #[test]
    fn the_mark_is_white_and_sits_inside_the_tile() {
        let size = 64;
        let data = rgba(size);
        let [r, g, b, _] = pixel(&data, size, (size as f32 * 0.45) as u32, size / 2);
        assert!(r > 240 && g > 240 && b > 240, "mark should be white, got {r},{g},{b}");
    }

    #[test]
    fn an_icon_smaller_than_the_mark_still_renders() {
        // 16px is the smallest a dock or taskbar will ask for; it must not panic or come back empty.
        let data = rgba(4);
        assert_eq!(data.len(), 16 * 16 * 4);
    }
}
