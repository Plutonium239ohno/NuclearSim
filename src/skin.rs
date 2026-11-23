//! Dark megaui skin shared by the apps.

use macroquad::prelude::*;
use macroquad::ui::{Skin, root_ui};

/// Bundled UI font (Adwaita Sans, SIL OFL 1.1; see assets/fonts): has the
/// arrows, Greek letters and superscripts the event logs use, which
/// megaui's built-in font lacks.
pub const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/AdwaitaSans-Regular.ttf");

// ---------------------------------------------------------------- skin

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba(r, g, b, a)
}

/// 3x3 nine-slice: 1px border around a flat fill.
fn framed(border: [u8; 4], fill: [u8; 4]) -> Image {
    let mut bytes = Vec::with_capacity(36);
    for i in 0..9 {
        bytes.extend_from_slice(if i == 4 { &fill } else { &border });
    }
    Image { width: 3, height: 3, bytes }
}

/// Combobox background with a small down-arrow on the right.
fn combo_image() -> Image {
    let (w, h) = (16usize, 30usize);
    let mut bytes = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let border = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            // Arrow: downward triangle centered in the right part.
            let ay = y as i32 - 12;
            let arrow = (0..5).contains(&ay) && (x as i32 - 8).abs() <= 4 - ay;
            let c: [u8; 4] = if border {
                [85, 90, 110, 255]
            } else if arrow {
                [200, 205, 220, 255]
            } else {
                [42, 45, 56, 255]
            };
            bytes[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&c);
        }
    }
    Image { width: w as u16, height: h as u16, bytes }
}

pub fn dark_skin() -> Skin {
    let ui = root_ui();
    let font = load_ttf_font_from_bytes(FONT_BYTES).expect("bundled font");
    let text = rgba(214, 218, 230, 255);
    let m = RectOffset::new(2.0, 2.0, 2.0, 2.0);

    let window_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .background(framed([70, 74, 92, 255], [18, 19, 25, 242]))
        .background_margin(RectOffset::new(1.0, 1.0, 1.0, 1.0))
        .color_inactive(rgba(255, 255, 255, 200))
        .text_color(text)
        .build();
    let window_titlebar_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .color(rgba(34, 36, 48, 255))
        .color_inactive(rgba(30, 31, 40, 230))
        .text_color(rgba(170, 180, 255, 255))
        .build();
    let label_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .margin(m)
        .text_color(text)
        .color_inactive(rgba(214, 218, 230, 140))
        .build();
    let button_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .margin(m)
        .color(rgba(44, 47, 60, 255))
        .color_hovered(rgba(60, 65, 84, 255))
        .color_clicked(rgba(85, 92, 125, 255))
        .text_color(text)
        .text_color_hovered(rgba(255, 255, 255, 255))
        .text_color_clicked(rgba(255, 255, 255, 255))
        .build();
    let checkbox_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .font_size(16)
        .color(rgba(48, 52, 66, 255))
        .color_hovered(rgba(66, 71, 92, 255))
        .color_clicked(rgba(90, 100, 140, 255))
        .color_selected(rgba(95, 125, 230, 255))
        .color_selected_hovered(rgba(115, 145, 245, 255))
        .text_color(text)
        .build();
    let editbox_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .background(framed([62, 66, 82, 255], [30, 32, 42, 255]))
        .background_margin(RectOffset::new(1.0, 1.0, 1.0, 1.0))
        .color(rgba(255, 255, 255, 255))
        .color_hovered(rgba(255, 255, 255, 255))
        .color_clicked(rgba(255, 255, 255, 255))
        .text_color(text)
        .color_selected(rgba(70, 90, 150, 255))
        .build();
    let combobox_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .background(combo_image())
        .background_margin(RectOffset::new(1.0, 14.0, 1.0, 1.0))
        .color(rgba(255, 255, 255, 255))
        .color_inactive(rgba(255, 255, 255, 160))
        .text_color(text)
        .build();
    let tabbar_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .margin(m)
        .color(rgba(40, 43, 55, 255))
        .color_hovered(rgba(60, 65, 84, 255))
        .color_clicked(rgba(85, 92, 125, 255))
        .color_selected(rgba(70, 90, 150, 255))
        .color_selected_hovered(rgba(85, 105, 170, 255))
        .text_color(text)
        .build();
    let scrollbar_style = ui.style_builder()
        .with_font(&font)
        .unwrap().color(rgba(28, 30, 38, 255)).build();
    let scrollbar_handle_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .color(rgba(72, 78, 100, 255))
        .color_hovered(rgba(92, 100, 130, 255))
        .color_clicked(rgba(110, 120, 160, 255))
        .color_inactive(rgba(72, 78, 100, 140))
        .build();
    let group_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .color(rgba(30, 32, 42, 120))
        .color_hovered(rgba(40, 44, 58, 120))
        .color_selected(rgba(70, 90, 150, 255))
        .color_selected_hovered(rgba(80, 100, 160, 255))
        .build();
    let progress_bar_style = ui
        .style_builder()
        .with_font(&font)
        .unwrap()
        .margin(m)
        .color(rgba(48, 52, 66, 255))
        .color_hovered(rgba(95, 125, 230, 255))
        .text_color(text)
        .build();

    Skin {
        window_style,
        window_titlebar_style,
        label_style,
        button_style,
        checkbox_style,
        editbox_style,
        combobox_style,
        tabbar_style,
        scrollbar_style,
        scrollbar_handle_style,
        group_style,
        progress_bar_style,
        title_height: 18.0,
        ..ui.default_skin()
    }
}

