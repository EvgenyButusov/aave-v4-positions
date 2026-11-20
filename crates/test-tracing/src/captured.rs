use std::sync::mpsc;

/// What a subscriber wrote, where a case can read it back.
#[derive(Clone)]
pub struct Captured(mpsc::Sender<Vec<u8>>);

impl Captured {
    #[must_use]
    pub fn new() -> (Self, Lines) {
        let (sender, received) = mpsc::channel();

        (Self(sender), Lines(received))
    }
}

impl std::io::Write for Captured {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(buffer.to_vec());

        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// The other end, consumed by [`Lines::read`] because a channel is drained rather than re-read.
pub struct Lines(mpsc::Receiver<Vec<u8>>);

impl Lines {
    #[must_use]
    pub fn read(self) -> String {
        self.0
            .try_iter()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .collect()
    }
}
