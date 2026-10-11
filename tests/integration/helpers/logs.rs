use std::cell::RefCell;
use std::io::Write;
use std::sync::Once;

thread_local! {
    static BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

struct ThreadLocalWriter;

impl Write for ThreadLocalWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        BUFFER.with(|buffer| buffer.borrow_mut().extend_from_slice(buf));
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct CapturedLogs;

impl CapturedLogs {
    pub fn start() -> Self {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .with_writer(|| ThreadLocalWriter)
                .with_ansi(false)
                .with_max_level(tracing::Level::INFO)
                .finish();
            tracing::subscriber::set_global_default(subscriber).unwrap();
        });
        BUFFER.with(|buffer| buffer.borrow_mut().clear());
        Self
    }

    pub fn line_containing(&self, needle: &str) -> String {
        let text = BUFFER.with(|buffer| String::from_utf8(buffer.borrow().clone()).unwrap());
        text.lines()
            .find(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("「{needle}」を含むログが無い:\n{text}"))
            .to_string()
    }
}
