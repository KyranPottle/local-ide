use crate::InferenceError;

pub const MAX_LINE_BYTES: usize = 256 * 1024;

#[derive(Default)]
pub struct LineBuffer {
    bytes: Vec<u8>,
    scanned: usize,
}

impl LineBuffer {
    pub fn extend(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn next_line(&mut self) -> Result<Option<Vec<u8>>, InferenceError> {
        let Some(offset) = self.bytes[self.scanned..].iter().position(|b| *b == b'\n') else {
            self.scanned = self.bytes.len();
            return within_limit(self.bytes.len()).map(|()| None);
        };
        let end = self.scanned + offset;
        within_limit(end)?;
        let mut line: Vec<u8> = self.bytes.drain(..=end).collect();
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        self.scanned = 0;
        Ok(Some(line))
    }
}

fn within_limit(length: usize) -> Result<(), InferenceError> {
    if length > MAX_LINE_BYTES {
        return Err(InferenceError::LimitExceeded {
            limit: "SSE line bytes",
            max: MAX_LINE_BYTES,
        });
    }
    Ok(())
}
