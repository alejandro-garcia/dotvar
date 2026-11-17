//! Native equivalent of transitions.dev's icon swap: 250ms ease-in-out,
//! opacity, 0.25→1 scale and 2→0px Gaussian blur. Original SVGs stay unchanged.
use crate::design;
use gpui_kit::{
    App, Div, ElementId, InteractiveElement, ParentElement, Styled, Transformation, Window,
    base::motion::{self, Easing, Transition},
    component::button::{Button, ButtonCustomVariant, ButtonVariants},
    div, px, rgb, size, svg,
};
use std::{sync::OnceLock, time::Duration};

#[derive(Clone, Copy)]
pub enum ActionIcon {
    Duplicate,
    Check,
    Eye,
    EyeSlash,
}

const SOURCES: [&str; 4] = [
    include_str!("../assets/actions/duplicate.svg"),
    include_str!("../assets/actions/check-2.svg"),
    include_str!("../assets/actions/eye.svg"),
    include_str!("../assets/actions/eye-slash.svg"),
];
const BLUR_STEPS: usize = 16;
const HOVER_GROUP: &str = "variable-action";
const HOVER_ICON: u32 = 0xa6a09b;

/// Shared 32px hit area and hover surface from Figma 1:902.
pub fn button(id: impl Into<ElementId>, cx: &App) -> Button {
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(rgb(design::SECONDARY).into())
                .hover(rgb(design::SELECTOR_BG).into())
                .active(rgb(design::SELECTOR_BG).into()),
        )
        .group(HOVER_GROUP)
        .w(px(32.))
        .h(px(32.))
        .p_0()
        .border_0()
        .rounded(px(8.))
}

// GPUI has no CSS filter. Its SVG renderer supports Gaussian filters, so keep
// a small, shared set of masks instead of rasterizing new SVGs on every frame.
fn artwork(icon: ActionIcon, blur: usize) -> &'static [u8] {
    static MASKS: OnceLock<[[String; BLUR_STEPS + 1]; 4]> = OnceLock::new();
    MASKS.get_or_init(|| {
        std::array::from_fn(|icon| {
            std::array::from_fn(|step| {
                let radius = 2. * step as f32 / BLUR_STEPS as f32;
                format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="30" viewBox="0 0 30 30"><defs><filter id="blur" x="0" y="0" width="30" height="30" filterUnits="userSpaceOnUse"><feGaussianBlur stdDeviation="{radius}"/></filter></defs><g filter="url(#blur)"><g transform="translate(6 6)">{}</g></g></svg>"#,
                    SOURCES[icon],
                )
            })
        })
    })[icon as usize][blur]
        .as_bytes()
}

pub fn swap(
    id: String,
    a: ActionIcon,
    b: ActionIcon,
    active: bool,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let progress = motion::transition(
        ElementId::Name(id.into()),
        if active { 1_f32 } else { 0_f32 },
        Transition::new(Duration::from_millis(250)).easing(Easing::EaseInOut),
        window,
        cx,
    );
    // Base honors the system's Reduce Motion preference and adopts the target
    // immediately. The first render also adopts its target without animating.
    let mut slot = div().relative().size(px(18.)).flex_shrink_0();
    for (icon, opacity) in [(a, 1. - progress), (b, progress)] {
        if opacity <= 0. {
            continue;
        }
        let scale = 0.25 + 0.75 * opacity;
        let blur = ((1. - opacity) * BLUR_STEPS as f32).round() as usize;
        slot = slot.child(
            svg()
                .data(artwork(icon, blur))
                .absolute()
                .left(px(-6.))
                .top(px(-6.))
                .size(px(30.))
                .text_color(rgb(design::SECONDARY).opacity(opacity))
                .group_hover(HOVER_GROUP, |this| {
                    this.text_color(rgb(HOVER_ICON).opacity(opacity))
                })
                .with_transformation(Transformation::scale(size(scale, scale))),
        );
    }
    slot
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(icon: ActionIcon, blur: usize) -> resvg::tiny_skia::Pixmap {
        let tree =
            resvg::usvg::Tree::from_data(artwork(icon, blur), &resvg::usvg::Options::default())
                .unwrap();
        let mut pixmap = resvg::tiny_skia::Pixmap::new(60, 60).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(2., 2.),
            &mut pixmap.as_mut(),
        );
        pixmap
    }

    #[test]
    fn supplied_icons_render_and_blur_without_an_opaque_or_clipped_background() {
        for icon in [
            ActionIcon::Duplicate,
            ActionIcon::Check,
            ActionIcon::Eye,
            ActionIcon::EyeSlash,
        ] {
            let sharp = render(icon, 0);
            let blurred = render(icon, BLUR_STEPS);
            let coverage = |image: &resvg::tiny_skia::Pixmap| {
                image.pixels().iter().filter(|p| p.alpha() > 5).count()
            };
            assert!(coverage(&sharp) > 50, "missing icon");
            assert!(coverage(&blurred) > coverage(&sharp), "filter was lost");
            for y in 0..60 {
                for x in 0..60 {
                    if x == 0 || y == 0 || x == 59 || y == 59 {
                        assert!(blurred.pixel(x, y).unwrap().alpha() <= 1, "clipped blur");
                    }
                }
            }
        }
    }
}
