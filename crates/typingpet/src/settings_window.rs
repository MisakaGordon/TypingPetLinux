//! 设置窗口：一般 / 图库 / 键反应 三个标签页。
//!
//! 与原版 macOS 版的信息架构一致，但用纯 GTK4 控件（GtkNotebook / Scale / Switch / DropDown /
//! ListBox），不引入 libadwaita，依赖更少。所有改动即时生效并落盘。

use crate::gui::{self, PetUi};
use gtk::prelude::*;
use gtk4 as gtk;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use typingpet_core::keys::KeyStroke;
use typingpet_core::library::PetImageSet;

pub const SETTINGS_WINDOW_TITLE: &str = "TypingPet 设置";

const LABEL_WIDTH: i32 = 168;
const THUMB_SIZE: i32 = 44;
const GALLERY_THUMB: i32 = 72;

pub struct SettingsView {
    ui: Rc<PetUi>,
    window: gtk::Window,
    loading: RefCell<bool>,

    // 一般
    scale: gtk::Scale,
    scale_value: gtk::Label,
    resting: gtk::Scale,
    resting_value: gtk::Label,
    hover: gtk::Scale,
    hover_value: gtk::Label,
    shake: gtk::DropDown,
    always_on_top: gtk::Switch,
    position_locked: gtk::Switch,
    avoid_pointer: gtk::Switch,
    autostart: gtk::Switch,
    system_status: gtk::Label,
    pointer_status: gtk::Label,

    // 图库
    sets_list: gtk::ListBox,
    gallery_summary: gtk::Label,
    apply_button: gtk::Button,
    rename_button: gtk::Button,
    delete_button: gtk::Button,
    selected_set: RefCell<Option<String>>,

    // 键反应
    rules_list: gtk::ListBox,
    rules_empty: gtk::Label,
}

impl SettingsView {
    pub fn build(ui: &Rc<PetUi>, initial_tab: u32) -> Rc<Self> {
        let window = gtk::Window::builder()
            .title(SETTINGS_WINDOW_TITLE)
            .default_width(780)
            .default_height(640)
            .resizable(true)
            .build();

        // ---- 一般 ----
        let (scale_row, scale, scale_value) = slider_row("大小", 0.35, 1.25);
        let (resting_row, resting, resting_value) = slider_row("常驻透明度", 0.0, 1.0);
        let (hover_row, hover, hover_value) = slider_row("悬停透明度", 0.0, 1.0);
        let shake = gtk::DropDown::from_strings(&["关", "弱", "中", "强"]);
        let shake_row = row("弹跳强度", &shake);
        let (always_row, always_on_top) = switch_row("始终显示在其他窗口之上");
        let (lock_row, position_locked) = switch_row("位置锁定（鼠标点击穿透）");
        let (avoid_row, avoid_pointer) = switch_row("位置锁定中躲避光标");
        let (autostart_row, autostart) = switch_row("登录时自动启动");

        let pointer_status = gtk::Label::new(None);
        pointer_status.add_css_class("dim-label");
        pointer_status.set_wrap(true);
        pointer_status.set_xalign(0.0);
        let system_status = gtk::Label::new(None);
        system_status.add_css_class("dim-label");
        system_status.set_wrap(true);
        system_status.set_xalign(0.0);

        let reset_button = gtk::Button::with_label("位置重置");
        let reset_row = row("宠物位置", &reset_button);

        let general_page = page();

        let pet_section = section("宠物");
        pet_section.append(&scale_row);
        pet_section.append(&resting_row);
        pet_section.append(&hover_row);
        pet_section.append(&note("100% 为完全不透明，0% 为完全透明；悬停时使用第二个值。"));
        pet_section.append(&shake_row);

        let window_section = section("窗口");
        window_section.append(&always_row);
        window_section.append(&lock_row);
        window_section.append(&avoid_row);
        window_section.append(&pointer_status);
        window_section.append(&reset_row);

        let system_section = section("系统");
        system_section.append(&autostart_row);
        system_section.append(&system_status);

        general_page.append(&pet_section);
        general_page.append(&window_section);
        general_page.append(&system_section);

        // ---- 图库 ----
        let sets_list = gtk::ListBox::new();
        sets_list.set_selection_mode(gtk::SelectionMode::Single);
        sets_list.add_css_class("boxed-list");
        let sets_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&sets_list)
            .build();

        let import_button = gtk::Button::with_label("导入文件夹…");
        let apply_button = gtk::Button::with_label("应用");
        let rename_button = gtk::Button::with_label("重命名…");
        let delete_button = gtk::Button::with_label("删除");
        let gallery_summary = gtk::Label::new(None);
        gallery_summary.add_css_class("dim-label");
        gallery_summary.set_xalign(0.0);

        let gallery_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let gallery_title = gtk::Label::new(Some("图片集"));
        gallery_title.add_css_class("section-title");
        gallery_header.append(&gallery_title);
        let gallery_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        gallery_spacer.set_hexpand(true);
        gallery_header.append(&gallery_spacer);
        gallery_header.append(&import_button);

        let gallery_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        gallery_actions.append(&gallery_summary);
        let actions_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        actions_spacer.set_hexpand(true);
        gallery_actions.append(&actions_spacer);
        gallery_actions.append(&rename_button);
        gallery_actions.append(&delete_button);
        gallery_actions.append(&apply_button);

        let gallery_note = note(
            "文件夹规则：idle.* 或 pet-idle.* 作为待机图，其余支持格式作为按键反应图；\
             没有待机图时使用排序第一张。支持 png/jpg/gif/webp/tiff 等。",
        );

        let gallery_page = page();
        gallery_page.append(&gallery_header);
        gallery_page.append(&sets_scroll);
        gallery_page.append(&gallery_note);
        gallery_page.append(&gallery_actions);

        // ---- 键反应 ----
        let rules_list = gtk::ListBox::new();
        rules_list.set_selection_mode(gtk::SelectionMode::None);
        rules_list.add_css_class("boxed-list");
        let rules_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&rules_list)
            .build();

        let add_rule_button = gtk::Button::with_label("添加规则…");
        let rules_empty = gtk::Label::new(Some(
            "还没有规则。点「添加规则…」先选图片，再按一下要绑定的键或组合键。",
        ));
        rules_empty.add_css_class("dim-label");
        rules_empty.set_wrap(true);
        rules_empty.set_xalign(0.0);

        let rules_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let rules_title = gtk::Label::new(Some("特定键反应"));
        rules_title.add_css_class("section-title");
        rules_header.append(&rules_title);
        let rules_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        rules_spacer.set_hexpand(true);
        rules_header.append(&rules_spacer);
        rules_header.append(&add_rule_button);

        let rules_page = page();
        rules_page.append(&rules_header);
        rules_page.append(&note(
            "精确匹配的规则优先于随机反应图。键位使用 evdev 键码，与 macOS 版配置不互通。",
        ));
        rules_page.append(&rules_empty);
        rules_page.append(&rules_scroll);

        let notebook = gtk::Notebook::new();
        notebook.append_page(&general_page, Some(&gtk::Label::new(Some("一般"))));
        notebook.append_page(&gallery_page, Some(&gtk::Label::new(Some("图库"))));
        notebook.append_page(&rules_page, Some(&gtk::Label::new(Some("键反应"))));
        notebook.set_margin_top(12);
        notebook.set_margin_bottom(12);
        notebook.set_margin_start(12);
        notebook.set_margin_end(12);
        notebook.set_current_page(Some(initial_tab.min(2)));
        window.set_child(Some(&notebook));

        let view = Rc::new(Self {
            ui: ui.clone(),
            window,
            loading: RefCell::new(false),
            scale,
            scale_value,
            resting,
            resting_value,
            hover,
            hover_value,
            shake,
            always_on_top,
            position_locked,
            avoid_pointer,
            autostart,
            system_status,
            pointer_status,
            sets_list,
            gallery_summary,
            apply_button,
            rename_button,
            delete_button,
            selected_set: RefCell::new(None),
            rules_list,
            rules_empty,
        });

        view.connect_signals(ui, &reset_button, &import_button, &add_rule_button);
        view.refresh();
        view
    }

    pub fn window(&self) -> &gtk::Window {
        &self.window
    }

    fn connect_signals(
        self: &Rc<Self>,
        ui: &Rc<PetUi>,
        reset_button: &gtk::Button,
        import_button: &gtk::Button,
        add_rule_button: &gtk::Button,
    ) {
        {
            let view = self.clone();
            let value_label = self.scale_value.clone();
            self.scale.connect_value_changed(move |scale| {
                value_label.set_text(&format!("{:.0}%", scale.value() * 100.0));
                if *view.loading.borrow() {
                    return;
                }
                view.ui.set_scale(scale.value());
            });
        }
        {
            let view = self.clone();
            let value_label = self.resting_value.clone();
            self.resting.connect_value_changed(move |scale| {
                value_label.set_text(&format!("{:.0}%", scale.value() * 100.0));
                if *view.loading.borrow() {
                    return;
                }
                view.ui.set_resting_opacity(scale.value());
            });
        }
        {
            let view = self.clone();
            let value_label = self.hover_value.clone();
            self.hover.connect_value_changed(move |scale| {
                value_label.set_text(&format!("{:.0}%", scale.value() * 100.0));
                if *view.loading.borrow() {
                    return;
                }
                view.ui.set_hover_opacity(scale.value());
            });
        }
        {
            let view = self.clone();
            self.shake.connect_selected_notify(move |dropdown| {
                if *view.loading.borrow() {
                    return;
                }
                view.ui.set_shake_level(dropdown.selected().min(3) as u8);
            });
        }
        {
            let view = self.clone();
            self.always_on_top.connect_active_notify(move |switch| {
                if *view.loading.borrow() {
                    return;
                }
                let value = switch.is_active();
                view.ui.set_always_on_top(value);
                view.refresh();
            });
        }
        {
            let view = self.clone();
            self.position_locked.connect_active_notify(move |switch| {
                if *view.loading.borrow() {
                    return;
                }
                let value = switch.is_active();
                view.ui.set_position_locked(value);
                view.refresh();
            });
        }
        {
            let view = self.clone();
            self.avoid_pointer.connect_active_notify(move |switch| {
                if *view.loading.borrow() {
                    return;
                }
                view.ui.set_avoid_pointer(switch.is_active());
            });
        }
        {
            let view = self.clone();
            self.autostart.connect_active_notify(move |switch| {
                if *view.loading.borrow() {
                    return;
                }
                if let Err(error) = view.ui.set_autostart(switch.is_active()) {
                    eprintln!("autostart: {error}");
                }
                view.refresh();
            });
        }
        {
            let view = self.clone();
            reset_button.connect_clicked(move |_| view.ui.reset_position());
        }

        // 图库
        {
            let view = self.clone();
            import_button.connect_clicked(move |_| view.import_folder_dialog());
        }
        {
            let view = self.clone();
            self.sets_list.connect_row_selected(move |_, row| {
                let id = row.and_then(set_id_of_row);
                *view.selected_set.borrow_mut() = id;
                view.update_gallery_buttons();
            });
        }
        {
            let view = self.clone();
            self.apply_button.connect_clicked(move |_| {
                let Some(id) = view.selected_set.borrow().clone() else {
                    return;
                };
                if let Err(error) = view.ui.activate_set(&id) {
                    view.show_error("无法应用该图片集", &error);
                }
                view.refresh();
            });
        }
        {
            let view = self.clone();
            self.rename_button.connect_clicked(move |_| view.rename_selected_set());
        }
        {
            let view = self.clone();
            self.delete_button.connect_clicked(move |_| view.delete_selected_set());
        }

        // 键反应
        {
            let view = self.clone();
            add_rule_button.connect_clicked(move |_| view.add_rule_dialog());
        }

        let _ = ui;
    }

    pub fn refresh(self: &Rc<Self>) {
        *self.loading.borrow_mut() = true;
        let settings = self.ui.settings();

        self.scale.set_value(settings.scale);
        self.scale_value.set_text(&format!("{:.0}%", settings.scale * 100.0));
        self.resting.set_value(settings.resting_opacity);
        self.resting_value
            .set_text(&format!("{:.0}%", settings.resting_opacity * 100.0));
        self.hover.set_value(settings.hover_opacity);
        self.hover_value
            .set_text(&format!("{:.0}%", settings.hover_opacity * 100.0));
        self.shake.set_selected(u32::from(settings.shake_level.min(3)));
        self.always_on_top.set_active(settings.always_on_top);
        self.position_locked.set_active(settings.position_locked);
        self.avoid_pointer.set_active(settings.avoids_pointer_when_locked);
        self.avoid_pointer
            .set_sensitive(settings.position_locked);
        self.autostart.set_active(self.ui.is_autostart_enabled());

        self.system_status.set_text(&format!(
            "键输入：{}　·　会话：{}　·　配置文件：{}",
            self.ui.input_status(),
            self.ui.session(),
            self.ui.runtime.borrow().options.config_path.display()
        ));
        self.pointer_status.set_text(if self.ui.pointer_available() {
            "躲避光标：X11 全局光标可用，为完整检测（约 90px 提前量）。"
        } else {
            "躲避光标：Wayland 不允许查询全局光标，改为局部检测 —— 只在宠物周围 40px 的检测环内生效；\
             开启期间该环形区域会占用鼠标点击，关闭躲避即可完全穿透。"
        });

        self.rebuild_sets();
        self.rebuild_rules();
        *self.loading.borrow_mut() = false;
    }

    // ---------- 图库 ----------

    fn rebuild_sets(&self) {
        while let Some(child) = self.sets_list.first_child() {
            self.sets_list.remove(&child);
        }
        let active_id = self.ui.active_set_id();
        // 默认选中"使用中"的图片集，而不是列表第一项
        if self.selected_set.borrow().is_none() {
            *self.selected_set.borrow_mut() = Some(active_id.clone());
        }
        let sets = self.ui.image_sets();
        let mut selected_row: Option<gtk::ListBoxRow> = None;

        for set in &sets {
            let row = self.build_set_row(set, &active_id);
            set_id_store(&row, &set.id);
            if Some(&set.id) == self.selected_set.borrow().as_ref() {
                selected_row = Some(row.clone());
            }
            self.sets_list.append(&row);
        }

        match selected_row {
            Some(row) => self.sets_list.select_row(Some(&row)),
            None => {
                if let Some(first) = self.sets_list.row_at_index(0) {
                    self.sets_list.select_row(Some(&first));
                    *self.selected_set.borrow_mut() = set_id_of_row(&first);
                }
            }
        }

        self.gallery_summary.set_text(&format!("共 {} 个图片集", sets.len()));
        self.update_gallery_buttons();
    }

    fn build_set_row(&self, set: &PetImageSet, active_id: &str) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        let container = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        container.set_margin_top(8);
        container.set_margin_bottom(8);
        container.set_margin_start(8);
        container.set_margin_end(8);

        let idle = self.ui.idle_url_for(set);
        let pixbuf = gui::load_thumbnail(idle.as_deref(), GALLERY_THUMB);
        let image = gtk::Image::new();
        if let Some(pixbuf) = pixbuf {
            let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
            image.set_paintable(Some(&texture));
        } else {
            image.set_icon_name(Some("image-missing"));
        }
        image.set_pixel_size(GALLERY_THUMB);
        container.append(&image);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let name = gtk::Label::new(Some(&set.name));
        name.set_xalign(0.0);
        name.add_css_class("section-title");
        text.append(&name);
        let detail = gtk::Label::new(Some(&format!(
            "反应图片 {} 张{}",
            self.ui.reaction_count_for(set),
            if set.is_built_in { "　·　内置" } else { "" }
        )));
        detail.set_xalign(0.0);
        detail.add_css_class("dim-label");
        text.append(&detail);
        container.append(&text);

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        container.append(&spacer);

        if set.id == active_id {
            let badge = gtk::Label::new(Some("使用中"));
            badge.add_css_class("active-badge");
            container.append(&badge);
        }

        row.set_child(Some(&container));
        row
    }

    fn update_gallery_buttons(&self) {
        let selected = self.selected_set.borrow().clone();
        let active = self.ui.active_set_id();
        let is_built_in = selected.as_deref() == Some(typingpet_core::library::BUILT_IN_SET_ID);
        let has_selection = selected.is_some();

        self.apply_button
            .set_sensitive(has_selection && selected.as_deref() != Some(active.as_str()));
        self.rename_button.set_sensitive(has_selection && !is_built_in);
        self.delete_button.set_sensitive(has_selection && !is_built_in);
    }

    fn import_folder_dialog(self: &Rc<Self>) {
        // FileChooserNative 是 GTK 4.0 的 API（FileDialog 要 4.10，会把 GTK 下限抬到 4.10）。
        // 它在 Wayland 下同样走 xdg-desktop-portal。
        let dialog = gtk::FileChooserNative::new(
            Some("选择图片文件夹"),
            Some(self.window()),
            gtk::FileChooserAction::SelectFolder,
            Some("选择"),
            Some("取消"),
        );
        let ui = self.ui.clone();
        let view = self.clone();
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(path) = dialog.file().and_then(|file| file.path()) {
                    match ui.import_folder(&path) {
                        Ok(set) => {
                            *view.selected_set.borrow_mut() = Some(set.id.clone());
                            view.refresh();
                        }
                        Err(error) => view.show_error("导入失败", &error),
                    }
                }
            }
            dialog.destroy();
        });
        dialog.show();
    }

    fn rename_selected_set(self: &Rc<Self>) {
        let Some(id) = self.selected_set.borrow().clone() else {
            return;
        };
        if id == typingpet_core::library::BUILT_IN_SET_ID {
            return;
        }
        let current = self
            .ui
            .image_sets()
            .into_iter()
            .find(|set| set.id == id)
            .map(|set| set.name)
            .unwrap_or_default();

        let dialog = gtk::Window::builder()
            .title("重命名图片集")
            .modal(true)
            .resizable(false)
            .default_width(360)
            .build();
        dialog.set_transient_for(Some(self.window()));

        let entry = gtk::Entry::new();
        entry.set_text(&current);
        let confirm = gtk::Button::with_label("确定");
        let cancel = gtk::Button::with_label("取消");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&cancel);
        buttons.append(&confirm);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.append(&entry);
        content.append(&buttons);
        dialog.set_child(Some(&content));

        {
            let view = self.clone();
            let dialog = dialog.clone();
            let entry = entry.clone();
            let rename_id = id.clone();
            confirm.connect_clicked(move |_| {
                let name = entry.text().to_string();
                if let Err(error) = view.ui.rename_set(&rename_id, &name) {
                    view.show_error("重命名失败", &error);
                }
                dialog.close();
                view.refresh();
            });
        }
        {
            let dialog = dialog.clone();
            cancel.connect_clicked(move |_| dialog.close());
        }
        dialog.present();
    }

    fn delete_selected_set(self: &Rc<Self>) {
        let Some(id) = self.selected_set.borrow().clone() else {
            return;
        };
        if id == typingpet_core::library::BUILT_IN_SET_ID {
            return;
        }
        let name = self
            .ui
            .image_sets()
            .into_iter()
            .find(|set| set.id == id)
            .map(|set| set.name)
            .unwrap_or_else(|| "该图片集".to_string());

        let dialog = gtk::MessageDialog::builder()
            .message_type(gtk::MessageType::Question)
            .buttons(gtk::ButtonsType::None)
            .text(format!("从列表中移除「{name}」？"))
            .secondary_text("原始图片文件不会被删除。")
            .modal(true)
            .transient_for(self.window())
            .build();
        dialog.add_button("取消", gtk::ResponseType::Cancel);
        dialog.add_button("移除", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Cancel);
        {
            let view = self.clone();
            dialog.connect_response(move |dialog, response| {
                if response == gtk::ResponseType::Accept {
                    if let Err(error) = view.ui.delete_set(&id) {
                        view.show_error("删除失败", &error);
                    }
                    *view.selected_set.borrow_mut() = None;
                    view.refresh();
                }
                dialog.destroy();
            });
        }
        dialog.present();
    }

    // ---------- 键反应 ----------
    fn rebuild_rules(self: &Rc<Self>) {
        while let Some(child) = self.rules_list.first_child() {
            self.rules_list.remove(&child);
        }
        let rules = self.ui.rules();
        self.rules_empty.set_visible(rules.is_empty());
        for rule in rules {
            let row = gtk::ListBoxRow::new();
            let container = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            container.set_margin_top(6);
            container.set_margin_bottom(6);
            container.set_margin_start(8);
            container.set_margin_end(8);

            let image_path = self.ui.rule_image_path(&rule);
            let pixbuf = gui::load_thumbnail(image_path.as_deref(), THUMB_SIZE);
            let image = gtk::Image::new();
            if let Some(pixbuf) = pixbuf {
                let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
                image.set_paintable(Some(&texture));
            } else {
                image.set_icon_name(Some("image-missing"));
            }
            image.set_pixel_size(THUMB_SIZE);
            container.append(&image);

            let name = gtk::Label::new(Some(&rule.stroke.display_name()));
            name.set_xalign(0.0);
            name.add_css_class("section-title");
            container.append(&name);

            let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            spacer.set_hexpand(true);
            container.append(&spacer);

            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.set_tooltip_text(Some("删除该规则"));
            remove.add_css_class("flat");
            {
                let view = self.clone();
                let rule_id = rule.id.clone();
                remove.connect_clicked(move |_| {
                    view.ui.remove_rule(&rule_id);
                    view.refresh();
                });
            }
            container.append(&remove);

            row.set_child(Some(&container));
            self.rules_list.append(&row);
        }
    }

    fn add_rule_dialog(self: &Rc<Self>) {
        let dialog = gtk::FileChooserNative::new(
            Some("选择该按键要显示的图片"),
            Some(self.window()),
            gtk::FileChooserAction::Open,
            Some("选择"),
            Some("取消"),
        );
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("图片"));
        filter.add_mime_type("image/*");
        dialog.add_filter(&filter);

        let ui = self.ui.clone();
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(path) = dialog.file().and_then(|file| file.path()) {
                    ui.begin_capture(path);
                }
            }
            dialog.destroy();
        });
        dialog.show();
    }

    fn show_error(&self, title: &str, detail: &str) {
        let dialog = gtk::MessageDialog::builder()
            .message_type(gtk::MessageType::Error)
            .buttons(gtk::ButtonsType::Close)
            .text(title)
            .secondary_text(detail)
            .modal(true)
            .transient_for(&self.window)
            .build();
        dialog.connect_response(|dialog, _| dialog.destroy());
        dialog.present();
    }
}

// ---------- 小工具 ----------

fn page() -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 18);
    container.set_margin_top(16);
    container.set_margin_bottom(16);
    container.set_margin_start(16);
    container.set_margin_end(16);
    container
}

fn section(title: &str) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.add_css_class("section-title");
    container.append(&label);
    container
}

fn row(label: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_size_request(LABEL_WIDTH, -1);
    container.append(&text);
    container.append(control);
    container
}

fn note(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_margin_start(LABEL_WIDTH + 12);
    label.add_css_class("dim-label");
    label
}

fn slider_row(label: &str, min: f64, max: f64) -> (gtk::Box, gtk::Scale, gtk::Label) {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, 0.01);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    let value = gtk::Label::new(Some("0%"));
    value.set_size_request(52, -1);
    value.set_xalign(1.0);
    value.add_css_class("dim-label");

    let container = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_size_request(LABEL_WIDTH, -1);
    container.append(&text);
    container.append(&scale);
    container.append(&value);
    (container, scale, value)
}

fn switch_row(label: &str) -> (gtk::Box, gtk::Switch) {
    let switch = gtk::Switch::new();
    switch.set_halign(gtk::Align::Start);

    let container = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_size_request(LABEL_WIDTH, -1);
    container.append(&text);
    container.append(&switch);
    (container, switch)
}

/// 把图片集 id 挂在行上（用 widget 名字存，避免额外的映射表）。
fn set_id_store(row: &gtk::ListBoxRow, id: &str) {
    row.set_widget_name(id);
}

fn set_id_of_row(row: &gtk::ListBoxRow) -> Option<String> {
    let name = row.widget_name().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// 让 `KeyStroke` 在本模块可见（按键捕获由 `PetUi` 完成，这里只用于类型标注）。
#[allow(dead_code)]
type CapturedStroke = KeyStroke;

#[allow(dead_code)]
fn unused_path_marker(_: PathBuf) {}
