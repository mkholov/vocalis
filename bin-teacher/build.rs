fn main() {
    #[cfg(windows)]
    {
        winres::WindowsResource::new()
            .set_icon("../assets/vocalis-teacher.ico")
            .compile()
            .expect("failed to embed .exe icon resource");
    }
}
