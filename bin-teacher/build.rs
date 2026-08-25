fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winres::WindowsResource::new()
            .set_icon("../assets/vocalis-teacher.ico")
            .compile()
            .expect("failed to embed .exe icon resource");
    }
}
