fn main() {
    #[cfg(windows)]
    {
        winres::WindowsResource::new()
            .set_icon("../assets/vocalis.ico")
            .compile()
            .expect("failed to embed .exe icon resource");
    }
}
