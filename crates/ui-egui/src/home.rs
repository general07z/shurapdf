//! Home tab: recommended tools, open card, pinned folders and recent files (local only, never
//! another app's list).

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, vec2};
use pdfcraft_engine::catalog;

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, panels::human_size, widgets};

pub mod folders_ui;

const RECOMMENDED: [&str; 5] = ["organize", "comment", "form", "edit", "protect"];

/// A soft shadow that rises on hover and settles when the card is pressed.
pub(crate) fn card_lift(ui: &egui::Ui, t: &Tokens, rect: Rect, press: theme::Press) {
    let lift = press.hover * (1.0 - press.down);
    if lift > 0.04 {
        let shadow = egui::Shadow { offset: [0, 3], blur: (14.0 * lift) as u8, spread: 0, color: t.page_shadow.gamma_multiply(0.85 * lift) };
        ui.painter().add(shadow.as_shape(rect, CornerRadius::same(10)));
    }
}

/// Draw the Home page with the welcome text, community card and recent documents.
pub fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::NONE.inner_margin(egui::Margin { left: 36, right: 36, top: 28, bottom: 28 }).show(ui, |ui| {
            widgets::at_reading_edge(ui, |ui| {
                ui.label(egui::RichText::new(tl!("Welcome to PdfCraft").replace("PdfCraft", crate::PRODUCT_NAME)).font(theme::semibold(24.0)));
                ui.label(
                    egui::RichText::new(tl!("An open-source PDF workbench — local, private, and scriptable."))
                        .color(t.text_muted)
                        .font(theme::regular(14.0)),
                );
            });
            ui.add_space(14.0);
            if crate::COMMUNITY_PROMPTS && !app.community_card_closed {
                egui::Frame::NONE
                    .fill(t.card)
                    .stroke(Stroke::new(1.0, t.border))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            // A community icon where upstream shows the ArtCraft mark (a trademark
                            // forks may not show).
                            let (mark, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                            icons::paint(ui, mark, "messages-square", 26.0, t.accent_text);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(tl!("Join the ArtCraft community")).font(theme::semibold(15.0)));
                                ui.label(egui::RichText::new(tl!("Get help, share feedback and follow development on Discord.")).color(t.text_muted));
                            });
                            // Closed for this run only: the card is back on the next launch.
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                if icons::button(ui, "x", 24.0, false, tl!("Close")).clicked() {
                                    app.community_card_closed = true;
                                }
                            });
                        });
                        ui.add_space(8.0);
                        if let Some(cmd) = widgets::community_links(ui) {
                            app.execute(cmd);
                        }
                    });
                ui.add_space(22.0);
            }

            egui::Frame::NONE
                .fill(t.card)
                .stroke(Stroke::new(1.0, t.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    widgets::at_reading_edge(ui, |ui| ui.label(egui::RichText::new(tl!("Recommended tools")).font(theme::semibold(15.0))));
                    ui.add_space(10.0);
                    // The cards start at the reading edge: the right one in a right-to-left interface.
                    let row = if crate::i18n::current().rtl() {
                        egui::Layout::right_to_left(egui::Align::Min).with_main_wrap(true)
                    } else {
                        egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true)
                    };
                    ui.with_layout(row, |ui| {
                        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
                        for id in RECOMMENDED {
                            let Some(g) = catalog::group(id) else { continue };
                            let (rect, resp) = ui.allocate_exact_size(vec2(190.0, 104.0), Sense::click());
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!(g.label)));
                            let press = theme::Press::track(ui, &resp);
                            card_lift(ui, &t, rect, press);
                            let fill = press.fill(t.card, t.hover, t.pressed);
                            ui.painter().rect(rect, CornerRadius::same(10), fill, Stroke::new(1.0, t.divider), egui::StrokeKind::Inside);
                            let body = rect.translate(press.offset());
                            let color = egui::Color32::from_rgb(g.hue[0], g.hue[1], g.hue[2]);
                            // A right-to-left interface mirrors the card: icon, title, blurb and action
                            // start at its right edge.
                            let rtl = crate::i18n::current().rtl();
                            let at = |inset: f32, width: f32, y: f32| {
                                let x = if rtl { body.right() - inset - width } else { body.left() + inset };
                                egui::pos2(x, body.top() + y)
                            };
                            icons::paint(ui, Rect::from_min_size(at(14.0, 22.0, 14.0), vec2(22.0, 22.0)), g.icon, 21.0, color);
                            let mut title_job = egui::text::LayoutJob::simple_singleline(tl!(g.label).to_owned(), theme::semibold(13.5), t.text);
                            title_job.wrap =
                                egui::text::TextWrapping { max_width: body.width() - 58.0, max_rows: 1, break_anywhere: true, ..Default::default() };
                            let title = ui.fonts_mut(|f| f.layout_job(title_job));
                            let title_elided = title.elided;
                            ui.painter().galley(at(44.0, title.size().x, 25.0 - title.size().y * 0.5), title, t.text);
                            let blurb = g
                                .sections
                                .first()
                                .map(|s| s.items.iter().take(3).map(|i| tl!(i.label)).collect::<Vec<_>>().join(" · "))
                                .unwrap_or_default();
                            // Fixed-height cards must leave room for their action in every language:
                            // the blurb takes the rows that fit above it (scripts with tall lines, such
                            // as Arabic, get one row where Latin gets two).
                            let action = ui.fonts_mut(|f| f.layout_no_wrap(tl!("Use now").to_owned(), theme::medium(12.0), t.accent_text));
                            let action_top = body.height() - 14.0 - action.size().y;
                            let blurb_layout = |rows: usize| {
                                let mut job = egui::text::LayoutJob::simple(blurb.clone(), theme::regular(11.5), t.text_muted, body.width() - 28.0);
                                job.wrap.max_rows = rows;
                                if rtl {
                                    job.halign = egui::Align::RIGHT;
                                }
                                ui.fonts_mut(|f| f.layout_job(job))
                            };
                            const GAP: f32 = 4.0;
                            let mut galley = blurb_layout(2);
                            if 46.0 + galley.size().y + GAP > action_top {
                                galley = blurb_layout(1);
                            }
                            // A tall single row moves up into the space under the title (not into it).
                            let blurb_top = (action_top - GAP - galley.size().y).clamp(36.0, 46.0);
                            let blurb_elided = galley.elided;
                            // A right-aligned job lays out around x = 0, so its anchor is its right end.
                            let blurb_pos = if rtl { egui::pos2(body.right() - 14.0, body.top() + blurb_top) } else { at(14.0, 0.0, blurb_top) };
                            ui.painter().galley(blurb_pos, galley, t.text_muted);
                            ui.painter().galley(at(14.0, action.size().x, action_top), action, t.accent_text);
                            let resp = if title_elided || blurb_elided { resp.on_hover_text(format!("{}\n{blurb}", tl!(g.label))) } else { resp };
                            if theme::hand(resp).clicked() {
                                app.select_tool(g.id);
                            }
                        }
                        let (rect, resp) = ui.allocate_exact_size(vec2(170.0, 104.0), Sense::click());
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!("Open file")));
                        let press = theme::Press::track(ui, &resp);
                        card_lift(ui, &t, rect, press);
                        ui.painter().rect(
                            rect,
                            CornerRadius::same(10),
                            press.fill(t.pasteboard, t.hover, t.pressed),
                            Stroke::new(1.0, t.divider),
                            egui::StrokeKind::Inside,
                        );
                        let body = rect.translate(press.offset());
                        icons::paint(ui, Rect::from_center_size(body.center() - vec2(0.0, 16.0), vec2(28.0, 28.0)), "folder-open", 26.0, t.icon);
                        ui.painter().text(body.center() + vec2(0.0, 22.0), Align2::CENTER_CENTER, tl!("Open file"), theme::semibold(13.0), t.text);
                        if theme::hand(resp).clicked() {
                            app.open_dialog();
                        }
                    });
                });

            #[cfg(not(target_arch = "wasm32"))]
            {
                ui.add_space(26.0);
                crate::folders_ui::section(app, ui);
            }

            ui.add_space(26.0);
            let mut clear = false;
            widgets::reading_row(ui, |ui| {
                ui.label(egui::RichText::new(tl!("Recent")).font(theme::semibold(17.0)));
                if !app.recent.is_empty() {
                    ui.with_layout(widgets::trailing_layout(), |ui| {
                        let resp = widgets::ghost_button(ui, "trash-2", tl!("Clear"));
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!("Clear Recent Files")));
                        clear = resp.on_hover_text(tl!("Clear Recent Files")).clicked();
                    });
                }
            });
            ui.add_space(8.0);
            if app.recent.is_empty() {
                widgets::at_reading_edge(ui, |ui| {
                    ui.label(egui::RichText::new(tl!("Files you open in PdfCraft appear here. Drop a PDF anywhere to open it.")).color(t.text_muted))
                });
            }
            let mut open = None;
            let mut remove = None;
            for r in &app.recent {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &r.name));
                let press = theme::Press::track(ui, &resp);
                // The row, not its response: the pointer stays "in" the row over its remove button.
                let hovered = ui.rect_contains_pointer(rect);
                if hovered && press.hover < 0.04 && press.down < 0.04 {
                    // The remove button sits in the row, so the row response may not be hovered.
                    ui.painter().rect_filled(rect, CornerRadius::same(8), t.hover);
                } else {
                    press.wash(ui, rect, 8, false);
                }
                if hovered {
                    // Remove just this file from the list (#430); the file itself is untouched.
                    let x_rect = Rect::from_center_size(rect.right_center() - vec2(26.0, 0.0), vec2(28.0, 28.0));
                    let x = ui.interact(x_rect, ui.scope_id().with(("remove-recent", &r.path)), Sense::click());
                    let label = crate::i18n::fmt(tl!("Remove {name} from Recent"), &[("name", &r.name)]);
                    x.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if x.hovered() {
                        ui.painter().rect_filled(x_rect, CornerRadius::same(6), t.pressed);
                    }
                    icons::paint(ui, x_rect.shrink(6.0), "x", 16.0, t.icon);
                    if x.on_hover_text(tl!("Remove from Recent")).clicked() {
                        remove = Some(r.path.clone());
                    }
                }
                let body = rect.translate(press.offset());
                icons::paint(
                    ui,
                    Rect::from_min_size(body.min + vec2(10.0, 11.0), vec2(24.0, 24.0)),
                    "file-text",
                    22.0,
                    egui::Color32::from_rgb(0xE0, 0x3E, 0x3E),
                );
                // Room on the right for the remove button.
                let detail = ui.painter().text(
                    body.right_center() - vec2(48.0, 0.0),
                    Align2::RIGHT_CENTER,
                    format!("{}  ·  {}", crate::i18n::trn(crate::i18n::current(), r.pages as u64, "{n} page", "{n} pages"), human_size(r.size)),
                    theme::regular(12.0),
                    t.text_muted,
                );
                // The name and path stop short of the page count; hovering shows the whole path.
                let width = detail.left() - 16.0 - (body.left() + 46.0);
                widgets::row_text(ui, body.min + vec2(46.0, 15.0), r.name.as_str(), theme::medium(13.5), t.text, width);
                widgets::row_text(ui, body.min + vec2(46.0, 32.0), r.path.as_str(), theme::regular(11.0), t.text_faint, width);
                if theme::hand(resp).on_hover_text(&r.path).clicked() {
                    open = Some(r.path.clone());
                }
            }
            if let Some(p) = open {
                app.open_recent(&p);
            }
            if let Some(p) = remove {
                app.recent.retain(|r| r.path != p);
            }
            if clear {
                app.execute("file.clear_recent");
            }
            ui.add_space(20.0);
            widgets::at_reading_edge(ui, |ui| {
                widgets::section_title(ui, tl!("Privacy"));
                ui.label(
                    egui::RichText::new(tl!("PdfCraft works offline. No telemetry, no account, and no cloud processing unless you add a provider."))
                        .color(t.text_muted),
                );
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed-height tool cards leave room for "Use now" in long translations (German) and in
    /// scripts with tall lines (Arabic, #home-cards), and a right-to-left interface lines the
    /// description and the action up at the card's right edge.
    #[test]
    fn translated_card_description_leaves_room_for_the_action() {
        for code in ["de", "ar"] {
            let lang = crate::i18n::Lang::from_code(code).unwrap();
            crate::i18n::set_current(lang);
            let ctx = egui::Context::default();
            ctx.set_fonts(theme::font_definitions());
            let mut app = PdfCraftApp::new();
            let blurb = catalog::group("form")
                .unwrap()
                .sections
                .first()
                .unwrap()
                .items
                .iter()
                .take(3)
                .map(|item| crate::i18n::tr(lang, item.label))
                .collect::<Vec<_>>()
                .join(" · ");
            // Two frames: fonts load on the first.
            let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1440.0, 900.0));
            ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| show(&mut app, ui)).drop_without_applying_deltas();
            let output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| show(&mut app, ui));
            crate::i18n::set_current(crate::i18n::Lang::EN);
            let text_shapes: Vec<_> =
                output.shapes.iter().filter_map(|shape| if let egui::Shape::Text(text) = &shape.shape { Some(text) } else { None }).collect();
            let description = text_shapes.iter().find(|text| text.galley.job.text == blurb).unwrap();
            // The reading edge of a text: its right end in Arabic, its left end otherwise.
            let edge = |text: &egui::epaint::TextShape| text.pos.x + if code == "ar" { text.galley.rect.max.x } else { text.galley.rect.min.x };
            let action = text_shapes
                .iter()
                .find(|text| text.galley.job.text == crate::i18n::tr(lang, "Use now") && (edge(text) - edge(description)).abs() < 0.5)
                .unwrap_or_else(|| panic!("{code}: the action is lined up with the description"));
            assert!(description.galley.rows.len() <= 2, "{code}");
            assert!(
                description.pos.y + description.galley.size().y + 4.0 <= action.pos.y,
                "{code}: the description must leave a visible gap before its action: {} + {} vs {}",
                description.pos.y,
                description.galley.size().y,
                action.pos.y
            );
            output.drop_without_applying_deltas();
        }
    }
}
