//! Bounded display replay while a history request is outstanding. Recording is backend-owned.
use super::types::TelemetryRow;
use std::collections::VecDeque;
const MAX_ROWS: usize = 8192;
const MAX_BYTES: usize = 8 * 1024 * 1024;
#[derive(Default)]
pub(super) struct ReseedBuffer {
    rows: VecDeque<(TelemetryRow, usize)>,
    bytes: usize,
    dropped: u64,
}
impl ReseedBuffer {
    pub fn push(&mut self, row: TelemetryRow) {
        let bytes = std::mem::size_of::<(TelemetryRow, usize)>()
            .saturating_add(row.data_type.capacity())
            .saturating_add(row.sender_id.capacity())
            .saturating_add(
                row.values
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Option<f32>>()),
            );
        if bytes > MAX_BYTES {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        while self.rows.len() >= MAX_ROWS || self.bytes.saturating_add(bytes) > MAX_BYTES {
            let Some((_, old_bytes)) = self.rows.pop_front() else {
                break;
            };
            self.bytes -= old_bytes;
            self.dropped = self.dropped.saturating_add(1);
        }
        self.bytes += bytes;
        self.rows.push_back((row, bytes));
    }
    pub fn clear(&mut self) {
        self.rows.clear();
        self.bytes = 0;
        self.dropped = 0;
    }
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
    pub fn take_rows(&mut self) -> Vec<TelemetryRow> {
        self.bytes = 0;
        self.rows.drain(..).map(|(row, _)| row).collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row(index: i64, values: usize) -> TelemetryRow {
        TelemetryRow {
            timestamp_ms: index,
            received_timestamp_ms: index,
            data_type: "LOADCELL".into(),
            sender_id: "DAQ".into(),
            data_type_id: Default::default(),
            sender_id_id: Default::default(),
            values: vec![Some(1.0); values],
        }
    }
    #[test]
    fn five_minute_stalled_history_keeps_latest_rows_with_bounded_memory() {
        let mut buffer = ReseedBuffer::default();
        for i in 0..1_500 * 300 {
            buffer.push(row(i, 1));
        }
        assert_eq!(buffer.rows.len(), MAX_ROWS);
        assert!(buffer.bytes <= MAX_BYTES);
        assert_eq!(buffer.dropped(), 450_000 - MAX_ROWS as u64);
        let rows = buffer.take_rows();
        assert_eq!(rows.last().unwrap().timestamp_ms, 449_999);
        assert_eq!(rows[0].timestamp_ms, 450_000 - MAX_ROWS as i64);
        assert!(buffer.rows.is_empty());
        assert_eq!(buffer.bytes, 0);
    }
    #[test]
    fn wide_channels_and_oversized_rows_obey_byte_budget() {
        let mut buffer = ReseedBuffer::default();
        for i in 0..1000 {
            buffer.push(row(i, 4096));
        }
        assert!(buffer.bytes <= MAX_BYTES && buffer.rows.len() < MAX_ROWS);
        let prior = buffer.rows.len();
        let drops = buffer.dropped();
        buffer.push(row(1001, MAX_BYTES));
        assert_eq!(buffer.rows.len(), prior);
        assert_eq!(buffer.dropped(), drops + 1);
        buffer.clear();
        assert_eq!(buffer.dropped(), 0);
        assert_eq!(buffer.bytes, 0);
        buffer.push(row(2000, 1));
        assert_eq!(buffer.take_rows()[0].timestamp_ms, 2000);
    }
}
