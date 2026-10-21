//! Server-sent events: `data:` lines, one event per blank-line-separated block.

use std::io::{self, BufRead};

/// Calls `on_data` with the data of each event, in order, until the body ends.
pub fn for_each(body: &mut dyn BufRead, mut on_data: impl FnMut(&str) -> io::Result<()>) -> io::Result<()> {
    let mut data = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = body.read_line(&mut line)?;
        let l = line.trim_end_matches(['\r', '\n']);
        if n == 0 || l.is_empty() {
            if !data.is_empty() {
                on_data(&data)?;
                data.clear();
            }
            if n == 0 {
                return Ok(());
            }
        } else if let Some(d) = l.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(d.strip_prefix(' ').unwrap_or(d));
        }
        // `event:`, `id:` and `:` comment lines carry nothing we need.
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn splits_events() {
        let raw = b"event: a\ndata: {\"x\":1}\n\n: ping\n\ndata: [DONE]\n\n";
        let mut got = vec![];
        super::for_each(&mut &raw[..], |d| {
            got.push(d.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(got, ["{\"x\":1}", "[DONE]"]);
    }
}
