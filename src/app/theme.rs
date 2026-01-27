// Copyright (C) 2026 SpruceOS Team
// Licensed under GPL-3.0-or-later

use super::InstallerApp;
use egui_thematic::ThemeConfig;

impl InstallerApp {
    pub(super) fn get_theme_config(&self) -> ThemeConfig {
        ThemeConfig {
            name: "SpruceOS".to_string(),
            dark_mode: true,
            override_text_color: Some([251, 241, 199, 255]),
            override_weak_text_color: Some([124, 111, 100, 255]),
            override_hyperlink_color: Some([131, 165, 152, 255]),
            override_faint_bg_color: Some([48, 48, 48, 255]),
            override_extreme_bg_color: Some([29, 32, 33, 255]),
            override_code_bg_color: Some([60, 56, 54, 255]),
            override_warn_fg_color: Some([214, 93, 14, 255]),
            override_error_fg_color: Some([204, 36, 29, 255]),
            override_window_fill: Some([40, 40, 40, 255]),
            override_window_stroke_color: None,
            override_window_stroke_width: None,
            override_window_corner_radius: None,
            override_window_shadow_size: None,
            override_panel_fill: Some([40, 40, 40, 255]),
            override_popup_shadow_size: None,
            override_selection_bg: Some([215, 180, 95, 255]),
            override_selection_stroke_color: None,
            override_selection_stroke_width: None,
            override_widget_noninteractive_bg_fill: None,
            override_widget_noninteractive_weak_bg_fill: None,
            override_widget_noninteractive_bg_stroke_color: None,
            override_widget_noninteractive_bg_stroke_width: None,
            override_widget_noninteractive_corner_radius: None,
            override_widget_noninteractive_fg_stroke_color: None,
            override_widget_noninteractive_fg_stroke_width: None,
            override_widget_noninteractive_expansion: None,
            override_widget_inactive_bg_fill: None, // No fill for unchecked checkboxes - just outline
            override_widget_inactive_weak_bg_fill: None,
            override_widget_inactive_bg_stroke_color: Some([124, 111, 100, 200]), // Border color for unchecked boxes
            override_widget_inactive_bg_stroke_width: Some(1.5), // Border width for checkbox outline
            override_widget_inactive_corner_radius: None,
            override_widget_inactive_fg_stroke_color: Some([104, 157, 106, 255]),
            override_widget_inactive_fg_stroke_width: None,
            override_widget_inactive_expansion: None,
            override_widget_hovered_bg_fill: Some([215, 180, 95, 60]),
            override_widget_hovered_weak_bg_fill: None,
            override_widget_hovered_bg_stroke_color: Some([215, 180, 95, 255]),
            override_widget_hovered_bg_stroke_width: None,
            override_widget_hovered_corner_radius: None,
            override_widget_hovered_fg_stroke_color: None,
            override_widget_hovered_fg_stroke_width: None,
            override_widget_hovered_expansion: None,
            override_widget_active_bg_fill: Some([215, 180, 95, 100]), // Checked checkbox background
            override_widget_active_weak_bg_fill: None,
            override_widget_active_bg_stroke_color: Some([215, 180, 95, 255]), // Border when checked
            override_widget_active_bg_stroke_width: Some(1.5), // Border width when checked
            override_widget_active_corner_radius: None,
            override_widget_active_fg_stroke_color: Some([104, 157, 106, 255]), // Checkmark color
            override_widget_active_fg_stroke_width: Some(2.0), // Thicker checkmark
            override_widget_active_expansion: None,
            override_widget_open_bg_fill: None,
            override_widget_open_weak_bg_fill: None,
            override_widget_open_bg_stroke_color: None,
            override_widget_open_bg_stroke_width: None,
            override_widget_open_corner_radius: None,
            override_widget_open_fg_stroke_color: None,
            override_widget_open_fg_stroke_width: None,
            override_widget_open_expansion: None,
            override_resize_corner_size: None,
            override_text_cursor_width: None,
            override_clip_rect_margin: None,
            override_button_frame: None,
            override_collapsing_header_frame: None,
            override_indent_has_left_vline: None,
            override_striped: None,
            override_slider_trailing_fill: None,
        }
    }
}
