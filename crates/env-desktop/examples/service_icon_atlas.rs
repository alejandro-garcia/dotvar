//! Render the complete catalog through the same resvg alpha-mask path as GPUI.
//! cargo run -p env-desktop --example service_icon_atlas --no-default-features --
//!   /tmp/service-icons.png Turso Supabase
use env_desktop::service_logos::Service;
use resvg::{tiny_skia, usvg};
use std::{env, fs, path::PathBuf};

fn render(bytes: &[u8], size: u32, stretch: bool, mask: bool) -> tiny_skia::Pixmap {
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).unwrap();
    let mut pixmap = tiny_skia::Pixmap::new(size, size).unwrap();
    let (sx, sy) = if stretch {
        (
            size as f32 / tree.size().width(),
            size as f32 / tree.size().height(),
        )
    } else {
        let scale = size as f32 / tree.size().width().max(tree.size().height());
        (scale, scale)
    };
    let tx = (size as f32 - tree.size().width() * sx) / 2.;
    let ty = (size as f32 - tree.size().height() * sy) / 2.;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_row(sx, 0., 0., sy, tx, ty),
        &mut pixmap.as_mut(),
    );
    if mask {
        for pixel in pixmap.data_mut().as_chunks_mut::<4>().0 {
            let a = pixel[3] as u32;
            pixel[0] = (0x79 * a / 255) as u8;
            pixel[1] = (0x71 * a / 255) as u8;
            pixel[2] = (0x6b * a / 255) as u8;
        }
    }
    pixmap
}

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    let output = args.first().expect("output PNG path");
    let services: Vec<_> = Service::ALL
        .iter()
        .filter(|s| {
            args.len() == 1
                || args[1..]
                    .iter()
                    .any(|n| n == s.name() || n == &format!("{s:?}"))
        })
        .collect();
    assert!(!services.is_empty());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/services");
    let rows = services.len().div_ceil(4);
    let mut atlas = tiny_skia::Pixmap::new(1120, (rows * 132 + 30) as u32).unwrap();
    atlas.fill(tiny_skia::Color::from_rgba8(0x29, 0x25, 0x24, 255));
    let mut labels = String::new();
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    for (index, service) in services.iter().enumerate() {
        let x = (index % 4 * 280) as i32;
        let y = (index / 4 * 132 + 24) as i32;
        let filename = service.path().strip_prefix("services/").unwrap();
        let original = fs::read(root.join(filename)).unwrap();
        let corrected_path = root.join("monochrome").join(filename);
        let corrected = fs::read(&corrected_path).unwrap_or_else(|_| original.clone());
        for (dx, size, bytes, stretch, mask) in [
            (10, 48, &original, false, false),
            (76, 48, &original, true, true),
            (144, 48, &corrected, true, true),
            (218, 16, &corrected, true, true),
        ] {
            let rendered = render(bytes, size, stretch, mask);
            atlas.draw_pixmap(
                x + dx,
                y + 28,
                rendered.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                tiny_skia::Transform::identity(),
                None,
            );
        }
        labels.push_str(&format!("<text x='{}' y='{}' fill='#e7e5e4' font-size='13'>{}</text><text x='{}' y='{}' fill='#a6a09b' font-size='10'>SVGL       alfa plano     corregido      16px</text>", x + 10, y + 14, service.name(), x + 10, y + 96));
        let large = render(&corrected, 256, true, false);
        let mut bounds = (256, 256, 0, 0);
        for (i, pixel) in large.pixels().iter().enumerate() {
            if pixel.alpha() > 20 {
                let (px, py) = (i as u32 % 256, i as u32 / 256);
                bounds.0 = bounds.0.min(px);
                bounds.1 = bounds.1.min(py);
                bounds.2 = bounds.2.max(px);
                bounds.3 = bounds.3.max(py);
            }
        }
        println!(
            "{:?}: corrected={} alpha-bounds/256={bounds:?}",
            service,
            corrected_path.exists()
        );
    }
    let label_svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='1120' height='{}'><g font-family='Arial'>{labels}</g></svg>",
        atlas.height()
    );
    let labels = usvg::Tree::from_str(&label_svg, &options).unwrap();
    resvg::render(
        &labels,
        tiny_skia::Transform::identity(),
        &mut atlas.as_mut(),
    );
    atlas.save_png(output).unwrap();
    println!("{output}");
}
