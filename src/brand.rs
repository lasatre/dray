//! The Dray mark, drawn from the path in `brand/logo/mark-*.svg`:
//!
//! `M10 10H24L30 17H39.5A18.5 18.5 0 0 1 39.5 54H10A4 4 0 0 1 6 50V14A4 4 0 0 1 10 10Z
//!  M25 26H31L41 35.5L31 45H25L35 35.5Z` (64-unit box, even-odd fill).
//!
//! The mark is rasterized here in the active palette's `brand` ink, which is `mark-copper.svg`
//! in the light theme and the dark-theme copper the logo README asks for on dark grounds.
//! The texture is cached per size and color.

use crate::theme;
use eframe::egui::{self, Color32, ColorImage, Pos2, TextureHandle, TextureOptions, Vec2, pos2};
use std::f32::consts::PI;

const VIEWBOX: f32 = 64.0;

fn arc(out: &mut Vec<Pos2>, c: (f32, f32), r: f32, a0: f32, a1: f32) {
    let n = 24;
    for i in 1..=n {
        let a = a0 + (a1 - a0) * i as f32 / n as f32;
        out.push(pos2(c.0 + r * a.cos(), c.1 + r * a.sin()));
    }
}

/// The folder-D outline and the chevron cut out of it.
fn outlines() -> [Vec<Pos2>; 2] {
    let mut body = vec![pos2(10.0, 10.0), pos2(24.0, 10.0), pos2(30.0, 17.0), pos2(39.5, 17.0)];
    arc(&mut body, (39.5, 35.5), 18.5, -PI / 2.0, PI / 2.0); // the D, to (39.5, 54)
    body.push(pos2(10.0, 54.0));
    arc(&mut body, (10.0, 50.0), 4.0, PI / 2.0, PI); // to (6, 50)
    body.push(pos2(6.0, 14.0));
    arc(&mut body, (10.0, 14.0), 4.0, PI, 1.5 * PI); // back to (10, 10)
    let chevron = [(25.0, 26.0), (31.0, 26.0), (41.0, 35.5), (31.0, 45.0), (25.0, 45.0), (35.0, 35.5)]
        .map(|(x, y)| pos2(x, y))
        .to_vec();
    [body, chevron]
}

fn inside(poly: &[Pos2], p: Pos2) -> bool {
    let mut odd = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            odd = !odd;
        }
        j = i;
    }
    odd
}

/// Rasterize the mark into a `px`-square image (4x4 supersampling, even-odd fill).
pub fn rasterize(px: usize, color: Color32) -> ColorImage {
    let shapes = outlines();
    let ss = 4;
    let scale = VIEWBOX / px as f32;
    let mut pixels = Vec::with_capacity(px * px);
    for y in 0..px {
        for x in 0..px {
            let mut hits = 0;
            for sy in 0..ss {
                for sx in 0..ss {
                    let p = pos2(
                        (x as f32 + (sx as f32 + 0.5) / ss as f32) * scale,
                        (y as f32 + (sy as f32 + 0.5) / ss as f32) * scale,
                    );
                    if shapes.iter().filter(|s| inside(s, p)).count() % 2 == 1 {
                        hits += 1;
                    }
                }
            }
            let a = hits as f32 / (ss * ss) as f32;
            pixels.push(color.gamma_multiply(a));
        }
    }
    ColorImage::new([px, px], pixels)
}

/// The mark at `size` points, in the palette's brand ink.
pub fn mark(ui: &mut egui::Ui, size: f32) -> egui::Response {
    let color = theme::of(ui).brand;
    let px = (size * ui.ctx().pixels_per_point()).round().max(16.0) as usize;
    let id = egui::Id::unique("dray-mark").with((px, color.to_array()));
    let tex = ui.ctx().data_mut(|d| d.get_temp::<TextureHandle>(id)).unwrap_or_else(|| {
        let tex = ui.ctx().load_texture("dray-mark", rasterize(px, color), TextureOptions::LINEAR);
        ui.ctx().data_mut(|d| d.insert_temp(id, tex.clone()));
        tex
    });
    ui.add(egui::Image::new((tex.id(), Vec2::splat(size))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_has_a_body_and_a_chevron_hole() {
        let img = rasterize(64, theme::LIGHT.brand);
        let alpha = |x: usize, y: usize| img.pixels[y * 64 + x].a();
        assert_eq!(alpha(12, 40), 255, "body is filled");
        assert_eq!(alpha(37, 35), 0, "chevron is cut out");
        assert_eq!(alpha(28, 35), 255, "body left of the chevron");
        assert_eq!(alpha(2, 2), 0, "outside is empty");
        assert_eq!(alpha(56, 35), 255, "the D's round end");
        assert_eq!(alpha(28, 13), 0, "notch beside the tab");
    }
}
