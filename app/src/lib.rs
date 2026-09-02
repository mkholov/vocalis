pub mod audio_devices;
pub mod screen_capture;
pub mod settings;
pub mod student;
pub mod teacher;
pub mod theme;
pub mod video;

use eframe::egui;

/// Backs the role-picker binary (`vocalis`): starts on a launcher screen and switches
/// into the chosen role in the same window. The dedicated `vocalis-teacher` /
/// `vocalis-student` binaries skip this entirely via [`run_teacher`] / [`run_student`].
enum VocalisApp {
    Launcher { teacher_name: String },
    TeacherClassPicker { teacher_name: String, screen: teacher::class_picker::ClassPickerScreen },
    Teacher(Box<teacher::app::TeacherApp>),
    Student(Box<student::app::StudentApp>),
}

impl eframe::App for VocalisApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        match self {
            VocalisApp::Launcher { teacher_name } => {
                let mut pick: Option<VocalisApp> = None;
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.heading(egui::RichText::new("Vocalis").size(40.0).color(theme::ACCENT));
                        ui.label("Лингафонный кабинет");
                        ui.add_space(40.0);

                        ui.group(|ui| {
                            ui.set_width(340.0);
                            ui.label("Имя преподавателя:");
                            ui.text_edit_singleline(teacher_name);
                            ui.add_space(6.0);
                            if ui
                                .add_sized([300.0, 44.0], egui::Button::new("🧑‍🏫 Я преподаватель"))
                                .clicked()
                            {
                                let name = if teacher_name.trim().is_empty() {
                                    "Преподаватель".to_string()
                                } else {
                                    teacher_name.trim().to_string()
                                };
                                pick = Some(VocalisApp::TeacherClassPicker {
                                    teacher_name: name,
                                    screen: teacher::class_picker::ClassPickerScreen::new(),
                                });
                            }
                        });

                        ui.add_space(16.0);

                        if ui
                            .add_sized([340.0, 44.0], egui::Button::new("🎓 Я ученик"))
                            .clicked()
                        {
                            pick = Some(VocalisApp::Student(Box::new(student::app::StudentApp::launch())));
                        }
                    });
                });
                if let Some(app) = pick {
                    *self = app;
                }
            }
            VocalisApp::TeacherClassPicker { teacher_name, screen } => {
                if let Some((class_id, class_name)) = screen.update(ctx) {
                    *self = VocalisApp::Teacher(Box::new(teacher::app::TeacherApp::launch(
                        teacher_name.clone(),
                        class_id,
                        class_name,
                    )));
                }
            }
            VocalisApp::Teacher(app) => app.update(ctx, frame),
            VocalisApp::Student(app) => app.update(ctx, frame),
        }
    }
}

/// Backs the dedicated `vocalis-teacher` binary: starts on the local password
/// gate (see `teacher::auth`) and only switches into the real console once
/// authenticated. Deliberately separate from `VocalisApp` — the role-picker
/// binary (`run_launcher`, "for your own machine or ad-hoc testing") skips this
/// gate entirely, since it's a dev/testing convenience rather than the console a
/// school actually deploys.
enum TeacherEntry {
    Auth(teacher::auth::AuthScreen),
    ClassPicker { teacher_name: String, screen: teacher::class_picker::ClassPickerScreen },
    App(Box<teacher::app::TeacherApp>),
}

impl eframe::App for TeacherEntry {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        match self {
            TeacherEntry::Auth(screen) => {
                if let Some(name) = screen.update(ctx) {
                    *self = TeacherEntry::ClassPicker {
                        teacher_name: name,
                        screen: teacher::class_picker::ClassPickerScreen::new(),
                    };
                }
            }
            TeacherEntry::ClassPicker { teacher_name, screen } => {
                if let Some((class_id, class_name)) = screen.update(ctx) {
                    *self = TeacherEntry::App(Box::new(teacher::app::TeacherApp::launch(
                        teacher_name.clone(),
                        class_id,
                        class_name,
                    )));
                }
            }
            TeacherEntry::App(app) => app.update(ctx, frame),
        }
    }
}

fn init_tracing() {
    // Multiple binaries share this crate; each runs in its own process, so a plain
    // `try_init` (rather than `init`) just avoids a panic if something upstream
    // already installed a subscriber.
    let _ = tracing_subscriber::fmt::try_init();
}

/// Decodes an embedded PNG into the RGBA buffer eframe wants for a window icon. This
/// is the *runtime* icon (title bar / taskbar while the app is running, and the only
/// icon on non-Windows platforms); the compiled `.exe`'s own file icon — what
/// Explorer shows before you even launch it — is a separate thing, baked in by each
/// `bin-*` package's `build.rs` via `winres` from the matching `.ico`.
fn load_icon(png_bytes: &[u8]) -> egui::IconData {
    let image = image::load_from_memory(png_bytes)
        .expect("embedded icon PNG is corrupt")
        .to_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

fn native_options(icon_png: &[u8]) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_icon(load_icon(icon_png))
            // eframe's own default start size reads cramped for a grid-heavy,
            // classroom-facing UI — open big enough that the seat grid has room to
            // lay out properly from the first frame, on ordinary and larger monitors.
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    }
}

/// `vocalis.exe`: shows the role picker, for your own machine or ad-hoc testing.
pub fn run_launcher() -> eframe::Result<()> {
    init_tracing();
    let default_teacher_name =
        std::env::var("VOCALIS_TEACHER_NAME").unwrap_or_else(|_| "Преподаватель".to_string());

    eframe::run_native(
        "Vocalis — лингафонный кабинет",
        native_options(include_bytes!("../../assets/vocalis-logo.png")),
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, settings::Settings::load().theme.is_light());
            Ok(Box::new(VocalisApp::Launcher {
                teacher_name: default_teacher_name,
            }))
        }),
    )
}

/// `vocalis-teacher.exe`: opens on the local password gate first (see
/// `TeacherEntry`/`teacher::auth`), then straight into the teacher console — for
/// the teacher's own machine, no picker, no way to accidentally end up in student
/// mode. The teacher's name now comes from the authenticated profile rather than
/// `VOCALIS_TEACHER_NAME`, since a stored profile makes that env var a bypass of
/// the very check it'd be sitting next to.
pub fn run_teacher() -> eframe::Result<()> {
    init_tracing();

    eframe::run_native(
        "Vocalis — консоль преподавателя",
        native_options(include_bytes!("../../assets/vocalis-logo-teacher.png")),
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, settings::Settings::load().theme.is_light());
            Ok(Box::new(TeacherEntry::Auth(teacher::auth::AuthScreen::new())))
        }),
    )
}

/// `vocalis-student.exe`: opens straight into the student client — the binary meant
/// for classroom lab machines.
pub fn run_student() -> eframe::Result<()> {
    init_tracing();
    eframe::run_native(
        "Vocalis — клиент ученика",
        native_options(include_bytes!("../../assets/vocalis-logo-student.png")),
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx, settings::Settings::load().theme.is_light());
            Ok(Box::new(student::app::StudentApp::launch()))
        }),
    )
}
