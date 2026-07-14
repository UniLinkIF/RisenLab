// RisenLab GUI — a real windowed application (not a CLI/script) for the texture remaster
// workflow, plus mesh/material tools. Long operations run on a background thread so the
// window stays responsive; results/log lines come back through a shared, mutex-protected log.

use eframe::egui;
use risenlab::{batch, content, gamepath};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Default)]
struct Shared {
    log: Vec<String>,
    busy: bool,
}

fn log(shared: &Arc<Mutex<Shared>>, msg: impl Into<String>) {
    shared.lock().unwrap().log.push(msg.into());
}

fn set_busy(shared: &Arc<Mutex<Shared>>, busy: bool) {
    shared.lock().unwrap().busy = busy;
}

fn open_in_explorer(path: &str) {
    let _ = Command::new("explorer").arg(path).spawn();
}

struct App {
    game_path: String,
    textures_dir: String,
    patch_out_dir: String,
    review_html: String,
    mesh_in: String,
    mesh_out: String,
    mat_in: String,
    mat_out: String,
    shared: Arc<Mutex<Shared>>,
}

impl Default for App {
    fn default() -> Self {
        let desktop = std::env::var("USERPROFILE")
            .map(|h| format!("{h}\\Desktop"))
            .unwrap_or_else(|_| ".".to_string());
        Self {
            game_path: format!("{desktop}\\Risen.lnk"),
            textures_dir: format!("{desktop}\\RisenLab-Textures"),
            patch_out_dir: format!("{desktop}\\RisenLab-Patch"),
            review_html: format!("{desktop}\\RisenLab-Review.html"),
            mesh_in: String::new(),
            mesh_out: String::new(),
            mat_in: String::new(),
            mat_out: String::new(),
            shared: Arc::new(Mutex::new(Shared::default())),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        let busy = self.shared.lock().unwrap().busy;

        {
            ui.heading("RisenLab — AI-ремастер Risen 1");
            ui.separator();

            ui.label("Гра (Risen.exe або ярлик .lnk):");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.game_path);
                if ui.button("Огляд…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Risen.exe / ярлик", &["exe", "lnk"])
                        .pick_file()
                    {
                        self.game_path = path.display().to_string();
                    }
                }
                if ui.add_enabled(!busy, egui::Button::new("🔍 Перевірити гру")).clicked() {
                    let game_path = PathBuf::from(&self.game_path);
                    let shared = self.shared.clone();
                    set_busy(&shared, true);
                    thread::spawn(move || {
                        match gamepath::resolve_shortcut(&game_path)
                            .ok()
                            .and_then(|exe| gamepath::discover_game_root(&exe).map(|root| (exe, root)))
                        {
                            Some((exe, root)) => {
                                log(&shared, format!("Знайдено: {} -> {}", exe.display(), root.display()));
                                if let Ok(archives) = gamepath::discover_archives(&root) {
                                    log(&shared, format!("Архівів: {}", archives.len()));
                                }
                            }
                            None => log(&shared, "Не знайшов гру за цим шляхом.".to_string()),
                        }
                        set_busy(&shared, false);
                    });
                }
            });

            ui.separator();
            ui.heading("Текстури");
            ui.label("Папка для розпакованих/редагованих текстур:");
            ui.text_edit_singleline(&mut self.textures_dir);

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("📤 1. Витягнути всі текстури у фото"))
                    .clicked()
                {
                    let game_path = PathBuf::from(&self.game_path);
                    let out_dir = PathBuf::from(&self.textures_dir);
                    let shared = self.shared.clone();
                    set_busy(&shared, true);
                    thread::spawn(move || {
                        log(&shared, "Витягую текстури з усієї гри (може тривати ~1 хв)...".to_string());
                        match batch::extract_all(&game_path, &out_dir) {
                            Ok(count) => log(&shared, format!("✅ Готово: {count} текстур у {}", out_dir.display())),
                            Err(e) => log(&shared, format!("❌ Помилка: {e}")),
                        }
                        set_busy(&shared, false);
                    });
                }
                if ui.button("📂 Відкрити папку").clicked() {
                    open_in_explorer(&self.textures_dir);
                }
            });

            ui.label("Тепер відредагуй фото в цій папці звичайним фоторедактором (або згенеруй нові).");

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("👁 2. Показати, що зміниться (огляд)"))
                    .clicked()
                {
                    let out_dir = PathBuf::from(&self.textures_dir);
                    let manifest = out_dir.join("manifest.tsv");
                    let review_html = PathBuf::from(&self.review_html);
                    let shared = self.shared.clone();
                    set_busy(&shared, true);
                    thread::spawn(move || {
                        match batch::build_review_html(&manifest, &out_dir, &review_html) {
                            Ok(count) => {
                                log(&shared, format!("✅ Змінено {count} текстур(и) — {}", review_html.display()));
                                let _ = Command::new("cmd").args(["/C", "start", "", review_html.to_str().unwrap_or("")]).spawn();
                            }
                            Err(e) => log(&shared, format!("❌ Помилка: {e}")),
                        }
                        set_busy(&shared, false);
                    });
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("📦 3. Зібрати патч (.pXX)"))
                    .clicked()
                {
                    let out_dir = PathBuf::from(&self.textures_dir);
                    let manifest = out_dir.join("manifest.tsv");
                    let patch_out = PathBuf::from(&self.patch_out_dir);
                    let shared = self.shared.clone();
                    set_busy(&shared, true);
                    thread::spawn(move || {
                        match batch::apply(&manifest, &out_dir, &patch_out) {
                            Ok(written) if written.is_empty() => {
                                log(&shared, "Немає змінених текстур — нічого патчити.".to_string())
                            }
                            Ok(written) => {
                                log(&shared, format!("✅ Записано {} патч(-ів):", written.len()));
                                for p in &written {
                                    log(&shared, format!("   {}", p.display()));
                                }
                            }
                            Err(e) => log(&shared, format!("❌ Помилка: {e}")),
                        }
                        set_busy(&shared, false);
                    });
                }
                if ui.button("📂 Відкрити папку патчу").clicked() {
                    open_in_explorer(&self.patch_out_dir);
                }
            });

            ui.separator();
            ui.heading("Меші та матеріали");

            ui.label("Меш (.xmsh) → .obj (для Blender):");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.mesh_in);
                if ui.button("Файл…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("xmsh", &["_xmsh", "xmsh"]).pick_file() {
                        self.mesh_in = p.display().to_string();
                        self.mesh_out = p.with_extension("obj").display().to_string();
                    }
                }
                ui.label("→");
                ui.text_edit_singleline(&mut self.mesh_out);
                if ui.add_enabled(!busy, egui::Button::new("Конвертувати")).clicked() {
                    let (input, output) = (PathBuf::from(&self.mesh_in), PathBuf::from(&self.mesh_out));
                    let shared = self.shared.clone();
                    set_busy(&shared, true);
                    thread::spawn(move || {
                        match content::mesh_to_obj(&input, &output) {
                            Ok(()) => log(&shared, format!("✅ Записано {}", output.display())),
                            Err(e) => log(&shared, format!("❌ Помилка: {e}")),
                        }
                        set_busy(&shared, false);
                    });
                }
            });

            ui.label("Матеріал (.xmat) → властивості (.txt):");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.mat_in);
                if ui.button("Файл…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("xmat", &["_xmat", "xmat"]).pick_file() {
                        self.mat_in = p.display().to_string();
                        self.mat_out = p.with_extension("txt").display().to_string();
                    }
                }
                ui.label("→");
                ui.text_edit_singleline(&mut self.mat_out);
                if ui.add_enabled(!busy, egui::Button::new("Дамп")).clicked() {
                    let (input, output) = (PathBuf::from(&self.mat_in), PathBuf::from(&self.mat_out));
                    let shared = self.shared.clone();
                    set_busy(&shared, true);
                    thread::spawn(move || {
                        match content::material_dump(&input, &output) {
                            Ok(()) => {
                                log(&shared, format!("✅ Записано {}", output.display()));
                                let _ = Command::new("notepad").arg(&output).spawn();
                            }
                            Err(e) => log(&shared, format!("❌ Помилка: {e}")),
                        }
                        set_busy(&shared, false);
                    });
                }
            });

            ui.separator();
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Виконується...");
                });
            }
            ui.heading("Журнал");
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                let shared = self.shared.lock().unwrap();
                for line in &shared.log {
                    ui.label(line);
                }
            });
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([720.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RisenLab",
        options,
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
}
