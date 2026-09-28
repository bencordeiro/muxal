//! A quiet, procedural space scene under smoked glass. No image downloads,
//! animation timers, or compositor-specific desktop transparency required.

use gpui::{
    Bounds, Pixels, Window, fill, linear_color_stop, linear_gradient, point, px, rgb, rgba, size,
};

pub fn paint_spaceglass(bounds: Bounds<Pixels>, window: &mut Window) {
    window.paint_quad(fill(
        bounds,
        linear_gradient(
            135.0,
            linear_color_stop(rgb(0x102c40), 0.0),
            linear_color_stop(rgb(0x241b3b), 1.0),
        ),
    ));
    window.paint_quad(fill(
        bounds,
        linear_gradient(
            35.0,
            linear_color_stop(rgba(0x07101fe8), 0.0),
            linear_color_stop(rgba(0x326b7524), 1.0),
        ),
    ));

    // Stable window-relative positions: resizing or moving a pane reveals the
    // same sky, without random flicker. Sparse and dim enough for terminal text.
    let left = f32::from(bounds.left());
    let top = f32::from(bounds.top());
    let right = f32::from(bounds.right());
    let bottom = f32::from(bounds.bottom());
    for row in (top / 83.0).floor() as i32..=(bottom / 83.0).ceil() as i32 {
        for col in (left / 97.0).floor() as i32..=(right / 97.0).ceil() as i32 {
            let seed =
                (col as u32).wrapping_mul(73_856_093) ^ (row as u32).wrapping_mul(19_349_663);
            let x = col as f32 * 97.0 + (seed % 79) as f32;
            let y = row as f32 * 83.0 + ((seed >> 8) % 67) as f32;
            if x >= left && x + 1.0 <= right && y >= top && y + 1.0 <= bottom {
                window.paint_quad(fill(
                    Bounds::new(point(px(x), px(y)), size(px(1.0), px(1.0))),
                    rgba(if seed.is_multiple_of(3) {
                        0xafdce54a
                    } else {
                        0xafa5df2b
                    }),
                ));
            }
        }
    }
    // A translucent blue glaze holds text contrast over both nebula hues.
    window.paint_quad(fill(bounds, rgba(0x0a142438)));
    window.paint_quad(fill(
        Bounds::new(bounds.origin, size(bounds.size.width, px(1.0))),
        linear_gradient(
            90.0,
            linear_color_stop(rgba(0x9debe650), 0.0),
            linear_color_stop(rgba(0xc5adff08), 1.0),
        ),
    ));
}
